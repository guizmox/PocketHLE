//! OEM processor identity reported through WinCE IOCTL_PROCESSOR_INFORMATION.
//! Descriptive profiles do not change the CPU backend or enable extra ISA features.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PocketPcProcessor {
    #[default]
    Pxa270,
    Pxa255,
    Sa1110,
    GenericArm,
}
impl PocketPcProcessor {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "pxa270" => Some(Self::Pxa270),
            "pxa255" => Some(Self::Pxa255),
            "sa1110" => Some(Self::Sa1110),
            "arm" => Some(Self::GenericArm),
            _ => None,
        }
    }
    pub fn core_revision(self) -> u16 {
        if self == Self::Sa1110 {
            4
        } else {
            5
        }
    }
    pub fn encode(self, arch: pocket_cpu::Arch) -> [u8; 576] {
        let mut data = [0u8; 576];
        data[..2].copy_from_slice(&1u16.to_le_bytes());
        fn string(data: &mut [u8], offset: usize, capacity: usize, value: &str) {
            for (i, c) in value.encode_utf16().take(capacity - 1).enumerate() {
                data[offset + i * 2..offset + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
            }
        }
        let (core, name, vendor, revision) = if arch == pocket_cpu::Arch::Mips {
            ("MIPS", "MIPS", "", 0)
        } else {
            let (name, vendor) = match self {
                Self::Pxa270 => ("PXA270", "Intel Corporation"),
                Self::Pxa255 => ("PXA255", "Intel Corporation"),
                Self::Sa1110 => ("SA-1110", "Intel Corporation"),
                Self::GenericArm => ("ARM", ""),
            };
            ("ARM", name, vendor, self.core_revision())
        };
        string(&mut data, 2, 40, core);
        data[82..84].copy_from_slice(&revision.to_le_bytes());
        string(&mut data, 84, 40, name);
        string(&mut data, 366, 100, vendor);
        // Optional silicon revisions, catalogue and clock remain unspecified.
        // No DSP/FPU/MIPS16 capability is claimed merely by selecting an identity.
        data
    }
}
