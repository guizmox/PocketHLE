//! WGL adapter policy belongs to Windows/the display driver, before GL startup.
#[cfg(windows)]
pub fn adapters() -> &'static [String] {
    static ADAPTERS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    ADAPTERS.get_or_init(|| unsafe {
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE,
        };
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return Vec::new();
        };
        let mut names = Vec::new();
        for index in 0..64 {
            let Ok(adapter) = factory.EnumAdapters1(index) else {
                break;
            };
            let mut desc = DXGI_ADAPTER_DESC1::default();
            if adapter.GetDesc1(&mut desc).is_ok() {
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                    continue;
                }
                let len = desc
                    .Description
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(desc.Description.len());
                names.push(String::from_utf16_lossy(&desc.Description[..len]));
            }
        }
        names
    })
}

#[cfg(windows)]
pub fn open_settings() {
    use windows::{
        core::w,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:display-advancedgraphics"),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        log::warn!(
            "Cannot open Windows graphics settings: {}",
            result.0 as isize
        );
    }
}
