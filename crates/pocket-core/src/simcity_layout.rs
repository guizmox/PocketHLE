//! Remove the absent Pocket PC shell's bottom command-bar reservation from
//! the verified SimCity 2000 image. This is a guest compatibility adjustment,
//! not a change to GetSystemMetrics or MoveWindow semantics. The CAB/EXE on
//! disk is unchanged; only the loaded image is adjusted.
use anyhow::{ensure, Context, Result};
use pocket_pe::LoadedImage;
use sha2::{Digest, Sha256};

const HASH: [u8; 32] = [
    0x04, 0x78, 0xcb, 0xbe, 0x32, 0x1f, 0x51, 0x79, 0xd1, 0xd3, 0x2f, 0xe8, 0x35, 0xcf, 0x89, 0xe9,
    0x66, 0x86, 0xc8, 0x4c, 0x5f, 0x87, 0x9c, 0xcb, 0x9c, 0xbf, 0xe1, 0x9d, 0x53, 0x06, 0xf8, 0x2b,
];
// Startup and return from the newspaper/options screen both calculate
// SM_CYSCREEN - 26 after hiding the taskbar. No shell command bar is hosted.
const SITES: [u32; 2] = [0x49340, 0x723dc];

pub(crate) fn apply(raw: &[u8], image: &mut LoadedImage) -> Result<bool> {
    if raw.len() != 3_507_200 || <[u8; 32]>::from(Sha256::digest(raw)) != HASH {
        return Ok(false);
    }
    patch_layout(image)?;
    log::info!("SimCity: use full display height without the Pocket PC shell command bar");
    Ok(true)
}

fn patch_layout(image: &mut LoadedImage) -> Result<()> {
    ensure!(image.image_base == 0x10000, "Unexpected SimCity image base");
    let text = image
        .sections
        .iter_mut()
        .find(|s| s.name == ".text")
        .context("Verified SimCity image has no text section")?;
    ensure!(text.is_executable(), "Unexpected SimCity text section");
    let start = image.image_base + text.virtual_address;
    let mut offsets = Vec::new();
    for address in SITES {
        let offset = address
            .checked_sub(start)
            .context("Unexpected SimCity text address")? as usize;
        ensure!(
            text.data.get(offset..offset + 4) == Some(0xe240301au32.to_le_bytes().as_slice()),
            "Unexpected SimCity height calculation at {address:08x}"
        );
        offsets.push(offset);
    }
    for offset in offsets {
        // sub r3,r0,#26 -> mov r3,r0, preserving the native resize path.
        text.data[offset..offset + 4].copy_from_slice(&0xe1a03000u32.to_le_bytes());
    }
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
            size_of_image: 0x400000,
            stack_reserve: 0x10000,
            entry_point: 0x8b048,
            sections: vec![pocket_pe::LoadedSection {
                name: ".text".into(),
                virtual_address: 0x1000,
                virtual_size: 0x90000,
                characteristics: 0x60000020,
                data: vec![0; 0x90000],
            }],
            imports: vec![],
            exports: Default::default(),
            resources: vec![],
            managed_runtime: None,
        };
        for address in SITES {
            let at = (address - 0x11000) as usize;
            image.sections[0].data[at..at + 4].copy_from_slice(&0xe240301au32.to_le_bytes());
        }
        image
    }
    #[test]
    fn other_images_are_untouched() {
        let mut image = fixture();
        let before = image.sections[0].data.clone();
        assert!(!apply(&vec![0; 3_507_200], &mut image).unwrap());
        assert_eq!(image.sections[0].data, before);
    }
    #[test]
    fn height_adjustment_is_atomic_and_only_changes_both_reservations() {
        let mut image = fixture();
        image.sections[0].data[(SITES[1] - 0x11000) as usize] ^= 1;
        let before = image.sections[0].data.clone();
        assert!(patch_layout(&mut image).is_err());
        assert_eq!(image.sections[0].data, before);
        let mut image = fixture();
        let mut expected = image.sections[0].data.clone();
        for address in SITES {
            let at = (address - 0x11000) as usize;
            expected[at..at + 4].copy_from_slice(&0xe1a03000u32.to_le_bytes());
        }
        patch_layout(&mut image).unwrap();
        assert_eq!(image.sections[0].data, expected);
    }
}
