//! Controller defaults for the native Windows build and Wine/Proton.
//! Steam Deck testing confirmed that the XInput profile receives all controls;
//! the default SDL driver selection only delivered the triggers in that run.

pub const PROTON_HINTS: [(&str, &str); 4] = [
    ("SDL_JOYSTICK_HIDAPI", "0"),
    ("SDL_JOYSTICK_WGI", "0"),
    ("SDL_JOYSTICK_RAWINPUT", "0"),
    ("SDL_XINPUT_ENABLED", "1"),
];

/// Detect Wine itself, rather than requiring a Steam shortcut environment.
/// Native Linux uses its own SDL drivers, and native Windows keeps HIDAPI.
#[cfg(windows)]
pub fn is_wine() -> bool {
    use std::ffi::{c_char, c_void};
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    let name: Vec<u16> = "ntdll.dll"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // Both strings are terminated; ntdll is already loaded in this process.
    // Only inspect the export. Never call it or unload the borrowed module.
    unsafe {
        let module = GetModuleHandleW(name.as_ptr());
        !module.is_null()
            && !GetProcAddress(module, b"wine_get_version\0".as_ptr().cast()).is_null()
    }
}

#[cfg(not(windows))]
pub fn is_wine() -> bool {
    false
}
