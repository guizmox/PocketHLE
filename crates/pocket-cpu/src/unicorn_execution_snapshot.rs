//! A guest dispatch continuation, including hooks that dispatch WinCE thunks.

use super::*;
use crate::arithmetic64::Arithmetic64;
use crate::image_pages::ImagePageBudget;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum HookKind {
    Dispatch,
    Trace,
    Arithmetic,
}

#[derive(Serialize, Deserialize)]
enum SavedHook {
    Dispatch {
        begin: u32,
        end: u32,
    },
    Arithmetic {
        address: u32,
        operation: Arithmetic64,
    },
}

#[derive(Serialize, Deserialize)]
pub struct ArmExecutionSnapshot {
    registers: ArmRegisterSnapshot,
    memory: ArmMemorySnapshot,
    hooks: Vec<SavedHook>,
}

impl UnicornCpu {
    /// Capture a WinCE dispatch boundary. The dispatcher owns the continuation
    /// address since Unicorn can still report the thunk where execution yielded.
    pub fn capture_arm_execution(
        &mut self,
        continuation: u32,
    ) -> Result<ArmExecutionSnapshot, CpuError> {
        let mut registers = self.capture_arm_registers()?;
        registers.set_continuation(continuation);
        let memory = self.capture_arm_memory()?;
        let hooks = self
            .code_hooks
            .iter()
            .filter_map(|&(begin, end, _, kind)| match kind {
                HookKind::Dispatch => Some(SavedHook::Dispatch { begin, end }),
                HookKind::Trace => None,
                HookKind::Arithmetic => {
                    self.inline_arithmetic
                        .get(&begin)
                        .map(|operation| SavedHook::Arithmetic {
                            address: begin,
                            operation: operation.get(),
                        })
                }
            })
            .collect();
        Ok(ArmExecutionSnapshot {
            registers,
            memory,
            hooks,
        })
    }
}

impl ArmExecutionSnapshot {
    /// Build a new Unicorn instance. No native hook IDs or JIT data cross the
    /// checkpoint. Reset all native execution caches through normal startup.
    pub fn restore(self, budget: Arc<dyn ImagePageBudget>) -> Result<(UnicornCpu, u32), CpuError> {
        let mut cpu = UnicornCpu::new()?;
        cpu.restore_arm_memory(self.memory, budget)?;
        for hook in self.hooks {
            match hook {
                SavedHook::Dispatch { begin, end } => {
                    if begin > end {
                        return Err(CpuError::Backend("invalid saved WinCE hook range".into()));
                    }
                    cpu.add_code_hook_range(begin, end)?;
                }
                SavedHook::Arithmetic { address, operation } => {
                    cpu.install_inline_arithmetic64(address, operation)?;
                }
            }
        }
        let continuation = cpu.restore_arm_registers(&self.registers)?;
        Ok((cpu, continuation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_snapshot_resumes_at_dispatch_continuation_with_hooks() {
        let mut cpu = UnicornCpu::new().unwrap();
        cpu.map_region(0x1000, 4096, Prot::ALL).unwrap();
        cpu.write_mem(0x1000, &[0x02, 0x30, 0xfe, 0xe7]).unwrap(); // adds r0, #2
        cpu.add_code_hook(0x1002).unwrap();
        cpu.write_reg(ArmReg::R0, 40).unwrap();
        // The previous API hook is still in the native PC register.
        cpu.write_reg(ArmReg::Pc, 0x3000).unwrap();
        let bytes = serde_json::to_vec(&cpu.capture_arm_execution(0x1001).unwrap()).unwrap();
        drop(cpu);

        let data: ArmExecutionSnapshot = serde_json::from_slice(&bytes).unwrap();
        let (mut restored, continuation) = data
            .restore(crate::image_pages::tests::Budget::new(0))
            .unwrap();
        assert_eq!(continuation, 0x1001);
        assert_eq!(
            restored.run_until_hook(continuation, 10).unwrap(),
            StopReason::Hook(0x1002)
        );
        assert_eq!(restored.read_reg(ArmReg::R0).unwrap(), 42);
    }
}
