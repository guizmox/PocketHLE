//! Only OS-supplied sharing destinations. No fixed app list or fallback actions.
#[cfg(not(windows))]
#[derive(Default)]
pub struct NativeShare;
#[cfg(not(windows))]
impl NativeShare {
    pub fn available(&mut self) -> bool {
        false
    }
    pub fn share(&mut self, _: &std::path::Path) -> Result<(), String> {
        Err("This system does not provide a native share panel".into())
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        cell::Cell,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };
    use windows::{
        core::{implement, Error, Interface, Result, HRESULT, HSTRING},
        ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager},
        Foundation::{
            Collections::{IIterable, IIterable_Impl, IIterator, IIterator_Impl},
            EventRegistrationToken, TypedEventHandler,
        },
        Storage::{IStorageItem, StorageFile},
        Win32::{
            System::WinRT::{
                RoGetActivationFactory, RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED,
                RO_INIT_SINGLETHREADED,
            },
            UI::{Input::KeyboardAndMouse::GetActiveWindow, Shell::IDataTransferManagerInterop},
        },
    };
    #[implement(IIterable<IStorageItem>)]
    struct Items(IStorageItem);
    impl IIterable_Impl<IStorageItem> for Items {
        fn First(&self) -> Result<IIterator<IStorageItem>> {
            Ok(ItemIterator {
                item: self.0.clone(),
                remaining: Cell::new(true),
            }
            .into())
        }
    }
    #[implement(IIterator<IStorageItem>)]
    struct ItemIterator {
        item: IStorageItem,
        remaining: Cell<bool>,
    }
    impl IIterator_Impl<IStorageItem> for ItemIterator {
        fn Current(&self) -> Result<IStorageItem> {
            if self.remaining.get() {
                Ok(self.item.clone())
            } else {
                Err(Error::from_hresult(HRESULT(0x8000000Bu32 as i32)))
            }
        }
        fn HasCurrent(&self) -> Result<bool> {
            Ok(self.remaining.get())
        }
        fn MoveNext(&self) -> Result<bool> {
            self.remaining.set(false);
            Ok(false)
        }
        fn GetMany(&self, items: &mut [Option<IStorageItem>]) -> Result<u32> {
            if !items.is_empty() && self.remaining.replace(false) {
                items[0] = Some(self.item.clone());
                Ok(1)
            } else {
                Ok(0)
            }
        }
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() }
        }
    }
    struct Backend {
        interop: IDataTransferManagerInterop,
        registration: Option<(DataTransferManager, EventRegistrationToken)>,
        path: Arc<Mutex<Option<PathBuf>>>,
        _apartment: Apartment,
    }
    impl Backend {
        fn new() -> Result<Self> {
            unsafe {
                RoInitialize(RO_INIT_SINGLETHREADED)?;
            }
            let apartment = Apartment;
            if !DataTransferManager::IsSupported()? {
                return Err(Error::from_hresult(HRESULT(0x80004001u32 as i32)));
            }
            let interop = unsafe {
                RoGetActivationFactory(&HSTRING::from(
                    "Windows.ApplicationModel.DataTransfer.DataTransferManager",
                ))?
            };
            Ok(Self {
                interop,
                registration: None,
                path: Arc::new(Mutex::new(None)),
                _apartment: apartment,
            })
        }
        fn share(&mut self, path: &Path) -> Result<()> {
            let hwnd = unsafe { GetActiveWindow() };
            if hwnd.0 == 0 {
                return Err(Error::from_hresult(HRESULT(0x80004005u32 as i32)));
            }
            if let Some((manager, token)) = self.registration.take() {
                let _ = manager.RemoveDataRequested(token);
            }
            let manager: DataTransferManager = unsafe { self.interop.GetForWindow(hwnd)? };
            *self.path.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.to_owned());
            let shared = self.path.clone();
            let token = manager.DataRequested(&TypedEventHandler::<
                DataTransferManager,
                DataRequestedEventArgs,
            >::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let request = args.Request()?;
                let Some(path) = shared.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
                    return Ok(());
                };
                request.Data()?.Properties()?.SetTitle(&HSTRING::from(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .as_ref(),
                ))?;
                let deferral = request.GetDeferral()?;
                std::thread::spawn(move || {
                    let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
                    let result = (|| -> Result<()> {
                        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(
                            path.to_string_lossy().as_ref(),
                        ))?
                        .get()?;
                        let items: IIterable<IStorageItem> = Items(file.cast()?).into();
                        request.Data()?.SetStorageItems(&items, true)
                    })();
                    if let Err(error) = result {
                        let _ = request.FailWithDisplayText(&HSTRING::from(error.to_string()));
                    }
                    let _ = deferral.Complete();
                    drop(deferral);
                    drop(request);
                    if initialized {
                        unsafe { RoUninitialize() };
                    }
                });
                Ok(())
            }))?;
            self.registration = Some((manager, token));
            unsafe { self.interop.ShowShareUIForWindow(hwnd) }
        }
    }
    impl Drop for Backend {
        fn drop(&mut self) {
            if let Some((manager, token)) = self.registration.take() {
                let _ = manager.RemoveDataRequested(token);
            }
        }
    }
    #[derive(Default)]
    pub struct NativeShare {
        backend: Option<Option<Backend>>,
    }
    impl NativeShare {
        pub fn available(&mut self) -> bool {
            self.backend
                .get_or_insert_with(|| Backend::new().ok())
                .is_some()
        }
        pub fn share(&mut self, path: &Path) -> std::result::Result<(), String> {
            if !self.available() {
                return Err("Native sharing is not available on this system".into());
            }
            self.backend
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .share(path)
                .map_err(|e| e.to_string())
        }
    }
}
#[cfg(windows)]
pub use platform::NativeShare;
