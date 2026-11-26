/* Standalone ARM dynamic TB-key regression test. Build like
 * unicorn-optimizations.c. Exercise the same PC after changing CPU state. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <unicorn/unicorn.h>
#include <unicorn/arm.h>

static void ok(uc_err error) { assert(error == UC_ERR_OK); }
static void reg(uc_engine *uc, int id, uint32_t value) {
    ok(uc_reg_write(uc, id, &value));
}
static uint32_t read_reg(uc_engine *uc, int id) {
    uint32_t value;
    ok(uc_reg_read(uc, id, &value));
    return value;
}

int main(void) {
    const int models[] = {UC_CPU_ARM_1176, UC_CPU_ARM_CORTEX_A15};
    for (unsigned model = 0; model < 2; ++model) {
        uc_engine *uc;
        ok(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
        ok(uc_ctl_set_cpu_model(uc, models[model]));
        ok(uc_mem_map(uc, 0x1000, 0x2000, UC_PROT_ALL));
        const uint32_t arm[] = {0xee344a00, 0xe12fff1e}; /* vadd s8,s8,s0; bx lr */
        const uint16_t thumb[] = {0x2007, 0x4770}; /* movs r0,#7; bx lr */
        ok(uc_mem_write(uc, 0x1000, arm, sizeof(arm)));
        ok(uc_mem_write(uc, 0x2000, thumb, sizeof(thumb)));
        reg(uc, UC_ARM_REG_C1_C0_2, 0xf00000);
        for (unsigned repeat = 0; repeat < 3; ++repeat) {
            for (unsigned enabled = 0; enabled < 2; ++enabled) {
                /* Nonzero legacy strides are reserved on Cortex-A15. */
                for (unsigned stride = 0; stride < (model == 0 ? 4u : 1u); ++stride) {
                    reg(uc, UC_ARM_REG_CPSR, 0x13);
                    reg(uc, UC_ARM_REG_FPEXC, enabled ? 0x40000000 : 0);
                    reg(uc, UC_ARM_REG_FPSCR, (stride << 20) | 0x10);
                    reg(uc, UC_ARM_REG_LR, 0x1800);
                    reg(uc, UC_ARM_REG_S0, 0x3f000000); /* 0.5 */
                    reg(uc, UC_ARM_REG_S8, 0x41000000); /* 8.0 */
                    uc_err result = uc_emu_start(uc, 0x1000, 0x1800, 0, 0);
                    assert(result == (enabled ? UC_ERR_OK : UC_ERR_INSN_INVALID));
                    assert(read_reg(uc, UC_ARM_REG_S8) ==
                           (enabled ? 0x41080000u : 0x41000000u));
                }
            }
            for (unsigned condition = 0; condition < 3; ++condition) {
                for (unsigned z = 0; z < 2; ++z) {
                    uint32_t it = condition == 0 ? 0 : condition == 1 ? 8 : 0x18;
                    reg(uc, UC_ARM_REG_CPSR, 0x33 | (z << 30) |
                        ((it & 0xfc) << 8) | ((it & 3) << 25));
                    reg(uc, UC_ARM_REG_R0, 11);
                    reg(uc, UC_ARM_REG_LR, 0x1800);
                    ok(uc_emu_start(uc, 0x2001, 0x1800, 0, 0));
                    unsigned execute = condition == 0 ||
                        (condition == 1 ? z != 0 : z == 0);
                    assert(read_reg(uc, UC_ARM_REG_R0) == (execute ? 7u : 11u));
                }
            }
        }
        ok(uc_close(uc));
    }
    puts("ARM dynamic TB state: PASS");
    return 0;
}
