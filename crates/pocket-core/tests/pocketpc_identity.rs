//! Per-game settings must change the guest view, never the shared device file.
use pocket_core::{
    kernel::{
        device_identity::DeviceIdentity,
        registry::{Registry, RegistryValue},
    },
    Emulator,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
struct Files(PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Vec<u8> {
    fn w(b: &mut [u8], p: usize, v: u32) {
        b[p..p + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn h(b: &mut [u8], p: usize, v: u16) {
        b[p..p + 2].copy_from_slice(&v.to_le_bytes());
    }
    let mut b = vec![0; 1024];
    b[..2].copy_from_slice(b"MZ");
    w(&mut b, 60, 0x80);
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    h(&mut b, 0x84, 0x1c0);
    h(&mut b, 0x86, 1);
    h(&mut b, 0x94, 224);
    h(&mut b, 0x96, 0x102);
    let o = 0x98;
    h(&mut b, o, 0x10b);
    w(&mut b, o + 16, 0x1000);
    w(&mut b, o + 28, 0x10000);
    w(&mut b, o + 32, 4096);
    w(&mut b, o + 36, 512);
    w(&mut b, o + 56, 0x2000);
    w(&mut b, o + 60, 512);
    h(&mut b, o + 68, 9);
    w(&mut b, o + 72, 0x10000);
    w(&mut b, o + 76, 0x1000);
    w(&mut b, o + 92, 16);
    b[0x178..0x17d].copy_from_slice(b".text");
    w(&mut b, 0x180, 4);
    w(&mut b, 0x184, 4096);
    w(&mut b, 0x188, 512);
    w(&mut b, 0x18c, 512);
    w(&mut b, 0x19c, 0x60000020);
    w(&mut b, 512, 0xe12fff1e);
    b
}
#[test]
fn settings_override_owner_and_hal_identity_but_device_registry_stays_unchanged() {
    let dir = Files(std::env::temp_dir().join(format!(
            "pockethle-owner-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir_all(&dir.0).unwrap();
    let pe = dir.0.join("test.exe");
    fs::write(&pe, fixture()).unwrap();
    let registry_path = dir.0.join("registry.json");
    let mut emu = Emulator::with_stub_cpu();
    emu.load_pe(&pe).unwrap();
    let r = &mut emu.process_mut().unwrap().state.registry;
    r.configure_persistence(&registry_path).unwrap();
    let original_owner = r.value(r"HKCU\ControlPanel\Owner", "Owner");
    let original_id =
        DeviceIdentity::configure(r, "4142", "00112233445566778899AABBCCDDEEFF").unwrap();
    emu.set_pocketpc_identity("Élodie", "MyPDA", "DEADBEEF")
        .unwrap();
    let r = &mut emu.process_mut().unwrap().state.registry;
    assert_eq!(
        r.value(r"HKCU\ControlPanel\Owner", "Owner"),
        Some(RegistryValue::Sz("Élodie".into()))
    );
    assert_eq!(
        r.value(r"HKLM\Ident", "Name"),
        Some(RegistryValue::Sz("MyPDA".into()))
    );
    assert!(r.open(r"HKLM\Ident").is_some());
    assert!(r
        .subkeys("HKLM")
        .iter()
        .any(|s| s.eq_ignore_ascii_case("Ident")));
    let id = DeviceIdentity::load_or_create(r).unwrap();
    assert_eq!(id.preset, b"MyPDA");
    assert_eq!(id.platform, [0xde, 0xad, 0xbe, 0xef]);
    r.flush().unwrap();
    emu.set_pocketpc_preset_hex("00112233445566778899").unwrap();
    let current =
        DeviceIdentity::load_or_create(&mut emu.process_mut().unwrap().state.registry).unwrap();
    assert_eq!(DeviceIdentity::hex(&current.preset), "00112233445566778899");
    assert_eq!(current.platform, [0xde, 0xad, 0xbe, 0xef]);
    assert!(emu.set_pocketpc_preset_hex("zz").is_err());
    emu.set_pocketpc_preset_hex("").unwrap();
    assert_eq!(
        DeviceIdentity::load_or_create(&mut emu.process_mut().unwrap().state.registry).unwrap(),
        current
    );

    assert!(emu.set_pocketpc_identity("wrong", "é", "").is_err());
    assert!(emu.set_pocketpc_identity("wrong", "ok", "z").is_err());
    assert_eq!(
        emu.process()
            .unwrap()
            .state
            .registry
            .value(r"HKCU\ControlPanel\Owner", "Owner"),
        Some(RegistryValue::Sz("Élodie".into()))
    );
    let mut other = Registry::with_device_defaults();
    other.configure_persistence(&registry_path).unwrap();
    assert_eq!(
        other.value(r"HKCU\ControlPanel\Owner", "Owner"),
        original_owner
    );
    assert!(!other.contains_key(r"HKLM\Ident"));
    assert_eq!(
        DeviceIdentity::load_or_create(&mut other).unwrap(),
        original_id
    );
    let mut next = Emulator::with_stub_cpu();
    next.load_pe(&pe).unwrap();
    next.process_mut()
        .unwrap()
        .state
        .registry
        .configure_persistence(&registry_path)
        .unwrap();
    next.set_pocketpc_identity("", "", "").unwrap();
    assert_eq!(
        next.process()
            .unwrap()
            .state
            .registry
            .value(r"HKCU\ControlPanel\Owner", "Owner"),
        original_owner
    );
}
