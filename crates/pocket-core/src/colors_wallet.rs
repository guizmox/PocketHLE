//! Separate Colors' solo wallet from its online wallet for one verified retail image.
//! The source EXE is never changed. Apply before mapping, so no translated code
//! or demand-paged backing can retain the unpatched calls.
use anyhow::{ensure, Context, Result};
use pocket_pe::LoadedImage;
use sha2::{Digest, Sha256};

const HASH: [u8; 32] = [
    0x06, 0x5c, 0x6f, 0x62, 0xe7, 0x93, 0x21, 0x0e, 0x78, 0x89, 0xca, 0xb0, 0x87, 0x49, 0x5a, 0x42,
    0xa8, 0x4c, 0x77, 0xb7, 0xb4, 0x18, 0x99, 0x41, 0x01, 0x56, 0x01, 0x7c, 0xb4, 0x3d, 0x7a, 0x92,
];
const STUB: u32 = 0x1288a8;
const STUB_SIZE: usize = 96;
const ORIGINAL_SIZE: u32 = 0x1178a8;
const SITES: [(u32, u32); 5] = [
    (0xf1c14, 0xebfc98b6), // first/forced user refresh
    (0xf1c34, 0x1a00001a), // setuserinfo response (mode 2)
    (0xf1c44, 0xebfc98aa), // purchase/mutation response (mode 1)
    (0xf1c7c, 0xebfc9815), // positive solo gains only, never solo spending
    (0xf1c90, 0xebfc9897), // regular getuserinfo merge (mode 0)
];
fn branch(at: u32, to: u32, opcode: u32) -> u32 {
    opcode | (((i64::from(to) - i64::from(at) - 8) / 4) as u32 & 0x00ff_ffff)
}
fn stub() -> [u32; 24] {
    [
        0xe92d4010, // push {r4,lr}
        0xe1a04000, // mov r4,r0
        0xe3a01000, // read the solo wallet, not the online response
        0xe3a02001,
        branch(STUB + 16, 0x17cd8, 0xeb000000),
        0xe1a01000, // r1 = unchanged solo Money
        0xe1a00004,
        0xe3a02001, // checkpoint ServerMoney to that same solo reference
        0xe8bd4010,
        branch(STUB + 36, 0x17ef4, 0xea000000), // original encrypted-save writer
        branch(STUB + 40, STUB, 0xeb000000),    // mode-2 acknowledged registration/update
        branch(STUB + 44, 0xf1ca4, 0xea000000),
        0xe92d4010, // merge helper: save server balance in r4
        0xe1a04001,
        0xe3a01000,
        0xe3a02000,
        branch(STUB + 64, 0x17cd8, 0xeb000000), // native read: Money - ServerMoney
        0xe3500000,
        0xb3a00000, // negative solo difference -> zero; independent spending
        0xe0900004, // adds r0,r0,r4
        0x63a00102, // signed overflow -> INT_MAX, never wrap into a debit
        0x62400001,
        0xe8bd4010,
        0xe12fff1e,
    ]
}

pub(crate) fn apply(raw: &[u8], image: &mut LoadedImage) -> Result<bool> {
    if raw.len() != 1_770_640 {
        return Ok(false);
    }
    let hash: [u8; 32] = Sha256::digest(raw).into();
    if hash != HASH {
        return Ok(false);
    }
    patch_layout(image)?;
    log::info!("Colors: separate solo/Turf Wars wallets enabled for verified retail image");
    Ok(true)
}
fn patch_layout(image: &mut LoadedImage) -> Result<()> {
    ensure!(image.image_base == 0x10000, "Unexpected Colors image base");
    let next_section = image
        .sections
        .iter()
        .filter(|s| s.virtual_address > 0x1000)
        .map(|s| s.virtual_address)
        .min()
        .unwrap_or(image.size_of_image);
    ensure!(
        0x1000 + ORIGINAL_SIZE + STUB_SIZE as u32 <= next_section,
        "Colors wallet stub would overlap another section"
    );
    let text = image
        .sections
        .iter_mut()
        .find(|s| s.name == ".text")
        .context("Verified Colors image has no text section")?;
    ensure!(
        text.virtual_address == 0x1000
            && text.virtual_size == ORIGINAL_SIZE
            && text.is_executable(),
        "Unexpected Colors text layout"
    );
    let start = 0x10000 + text.virtual_address;
    let offset = (STUB - start) as usize;
    ensure!(
        text.data.len() == ORIGINAL_SIZE as usize,
        "Unexpected Colors text data size"
    );
    for &(address, word) in &SITES {
        let at = (address - start) as usize;
        ensure!(
            text.data.get(at..at + 4) == Some(word.to_le_bytes().as_slice()),
            "Unexpected Colors wallet instruction at {address:08x}"
        );
    }
    // The loader truncates raw padding to VirtualSize. Extend its owned copy
    // into the verified unused gap, retaining the existing executable page.
    // Every precondition is checked before mutating any instruction.
    text.data.resize(offset + STUB_SIZE, 0);
    for (index, word) in stub().iter().enumerate() {
        text.data[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for &(address, _) in &SITES {
        let word = match address {
            0xf1c34 => branch(address, STUB + 40, 0x1a000000),
            0xf1c7c => branch(address, STUB + 48, 0xeb000000),
            _ => branch(address, STUB, 0xeb000000),
        };
        let at = (address - start) as usize;
        text.data[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    text.virtual_size = ORIGINAL_SIZE + STUB_SIZE as u32;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> LoadedImage {
        let mut image = pocket_pe::LoadedImage {
            source_path: "fixture".into(),
            machine: 0x1c2,
            subsystem: 9,
            image_base: 0x10000,
            size_of_image: 0x705000,
            stack_reserve: 0x10000,
            entry_point: 0x116628,
            sections: vec![pocket_pe::LoadedSection {
                name: ".text".into(),
                virtual_address: 0x1000,
                virtual_size: ORIGINAL_SIZE,
                characteristics: 0x60000020,
                data: vec![0; ORIGINAL_SIZE as usize],
            }],
            imports: vec![],
            exports: Default::default(),
            resources: vec![],
            managed_runtime: None,
        };
        for (address, word) in SITES {
            let at = (address - 0x11000) as usize;
            image.sections[0].data[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        image
    }
    /// Optional retail fixture: do not distribute the copyrighted executable.
    #[test]
    #[ignore = "requires POCKETHLE_COLORS_EXE"]
    fn supplied_retail_image_matches_and_exports_validation_patch() {
        let path = std::env::var("POCKETHLE_COLORS_EXE").expect("retail fixture path");
        let raw = std::fs::read(path).unwrap();
        let mut image = pocket_pe::load_bytes(&raw).unwrap();
        assert!(apply(&raw, &mut image).unwrap());
        if let Ok(path) = std::env::var("POCKETHLE_COLORS_PATCH_DUMP") {
            let text = image.sections.iter().find(|s| s.name == ".text").unwrap();
            let start = image.image_base + text.virtual_address;
            let at = (STUB - start) as usize;
            let mut dump = text.data[at..at + STUB_SIZE].to_vec();
            for (address, _) in SITES {
                let at = (address - start) as usize;
                dump.extend_from_slice(&address.to_le_bytes());
                dump.extend_from_slice(&text.data[at..at + 4]);
            }
            std::fs::write(path, dump).unwrap();
        }
    }
    #[test]
    fn unknown_binary_is_untouched() {
        let mut image = fixture();
        let before = image.sections[0].data.clone();
        assert!(!apply(&vec![0; 1_770_640], &mut image).unwrap());
        assert_eq!(image.sections[0].data, before);
    }
    #[test]
    fn checked_layout_is_atomic_and_stub_is_exact() {
        let mut image = fixture();
        let at = (SITES[3].0 - 0x11000) as usize;
        image.sections[0].data[at] ^= 1;
        let before = image.sections[0].data.clone();
        assert!(patch_layout(&mut image).is_err());
        assert_eq!(image.sections[0].data, before);
        let mut image = fixture();
        patch_layout(&mut image).unwrap();
        let at = (STUB - 0x11000) as usize;
        for (index, word) in stub().iter().enumerate() {
            assert_eq!(
                &image.sections[0].data[at + index * 4..at + index * 4 + 4],
                &word.to_le_bytes()
            );
        }
        assert_eq!(
            image.sections[0].virtual_size,
            ORIGINAL_SIZE + STUB_SIZE as u32
        );
        assert!(patch_layout(&mut image).is_err());
    }
}
