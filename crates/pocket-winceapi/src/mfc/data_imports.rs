//! Static CE data imports resolve to owned guest descriptors/maps.
use super::*;
use pocket_cpu::Cpu;
use pocket_kernel::KernelState;
use pocket_pe::ImportBinding;
// The retail CE3 data-export boundary also identifies unsupported ordinal
// data imports, so they cannot silently become executable function thunks.
const DATA: &[(u16, Option<&str>)] = &[
    (19, Some("?afxData@@3UAUX_DATA@@A")),
    (
        20,
        Some("?classCArchiveException@CArchiveException@@2UCRuntimeClass@@B"),
    ),
    (
        21,
        Some("?classCAsyncSocket@CAsyncSocket@@2UCRuntimeClass@@B"),
    ),
    (22, Some("?classCBitmap@CBitmap@@2UCRuntimeClass@@B")),
    (
        23,
        Some("?classCBitmapButton@CBitmapButton@@2UCRuntimeClass@@B"),
    ),
    (24, Some("?classCBrush@CBrush@@2UCRuntimeClass@@B")),
    (25, Some("?classCButton@CButton@@2UCRuntimeClass@@B")),
    (26, Some("?classCByteArray@CByteArray@@2UCRuntimeClass@@A")),
    (
        27,
        Some("?classCCeCommandBar@CCeCommandBar@@2UCRuntimeClass@@B"),
    ),
    (28, Some("?classCCeDocList@CCeDocList@@2UCRuntimeClass@@B")),
    (
        29,
        Some("?classCCeDocListDocTemplate@CCeDocListDocTemplate@@2UCRuntimeClass@@B"),
    ),
    (45, Some("?classCCeSocket@CCeSocket@@2UCRuntimeClass@@B")),
    (46, Some("?classCClientDC@CClientDC@@2UCRuntimeClass@@B")),
    (70, Some("?classCCmdTarget@CCmdTarget@@2UCRuntimeClass@@B")),
    (71, Some("?classCComboBox@CComboBox@@2UCRuntimeClass@@B")),
    (
        90,
        Some("?classCCommonDialog@CCommonDialog@@2UCRuntimeClass@@B"),
    ),
    (
        100,
        Some("?classCControlBar@CControlBar@@2UCRuntimeClass@@B"),
    ),
    (
        106,
        Some("?classCCriticalSection@CCriticalSection@@2UCRuntimeClass@@B"),
    ),
    (118, Some("?classCCtrlView@CCtrlView@@2UCRuntimeClass@@B")),
    (119, Some("?classCDC@CDC@@2UCRuntimeClass@@B")),
    (
        120,
        Some("?classCDWordArray@CDWordArray@@2UCRuntimeClass@@A"),
    ),
    (130, Some("?classCDialog@CDialog@@2UCRuntimeClass@@B")),
    (131, Some("?classCDialogBar@CDialogBar@@2UCRuntimeClass@@B")),
    (
        137,
        Some("?classCDocManager@CDocManager@@2UCRuntimeClass@@B"),
    ),
    (
        139,
        Some("?classCDocTemplate@CDocTemplate@@2UCRuntimeClass@@B"),
    ),
    (141, Some("?classCDocument@CDocument@@2UCRuntimeClass@@B")),
    (
        142,
        Some("?classCDynLinkLibrary@CDynLinkLibrary@@2UCRuntimeClass@@B"),
    ),
    (143, Some("?classCEdit@CEdit@@2UCRuntimeClass@@B")),
    (144, Some("?classCEditView@CEditView@@2UCRuntimeClass@@B")),
    (145, Some("?classCEvent@CEvent@@2UCRuntimeClass@@B")),
    (157, Some("?classCException@CException@@2UCRuntimeClass@@B")),
    (158, Some("?classCFile@CFile@@2UCRuntimeClass@@B")),
    (
        159,
        Some("?classCFileDialog@CFileDialog@@2UCRuntimeClass@@B"),
    ),
    (
        164,
        Some("?classCFileException@CFileException@@2UCRuntimeClass@@B"),
    ),
    (
        168,
        Some("?classCFindReplaceDialog@CFindReplaceDialog@@2UCRuntimeClass@@B"),
    ),
    (170, Some("?classCFont@CFont@@2UCRuntimeClass@@B")),
    (172, Some("?classCFormView@CFormView@@2UCRuntimeClass@@B")),
    (185, Some("?classCFrameWnd@CFrameWnd@@2UCRuntimeClass@@B")),
    (196, Some("?classCGdiObject@CGdiObject@@2UCRuntimeClass@@B")),
    (
        197,
        Some("?classCHeaderCtrl@CHeaderCtrl@@2UCRuntimeClass@@B"),
    ),
    (
        198,
        Some("?classCHttpConnection@CHttpConnection@@2UCRuntimeClass@@B"),
    ),
    (214, Some("?classCHttpFile@CHttpFile@@2UCRuntimeClass@@B")),
    (216, Some("?classCImageList@CImageList@@2UCRuntimeClass@@B")),
    (
        243,
        Some("?classCInternetConnection@CInternetConnection@@2UCRuntimeClass@@B"),
    ),
    (
        245,
        Some("?classCInternetException@CInternetException@@2UCRuntimeClass@@B"),
    ),
    (
        256,
        Some("?classCInternetFile@CInternetFile@@2UCRuntimeClass@@B"),
    ),
    (
        265,
        Some("?classCInternetSession@CInternetSession@@2UCRuntimeClass@@B"),
    ),
    (266, Some("?classCListBox@CListBox@@2UCRuntimeClass@@B")),
    (272, Some("?classCListCtrl@CListCtrl@@2UCRuntimeClass@@B")),
    (273, Some("?classCListView@CListView@@2UCRuntimeClass@@B")),
    (
        274,
        Some("?classCMapPtrToPtr@CMapPtrToPtr@@2UCRuntimeClass@@B"),
    ),
    (
        283,
        Some("?classCMapPtrToWord@CMapPtrToWord@@2UCRuntimeClass@@B"),
    ),
    (
        307,
        Some("?classCMapStringToOb@CMapStringToOb@@2UCRuntimeClass@@A"),
    ),
    (
        370,
        Some("?classCMapStringToPtr@CMapStringToPtr@@2UCRuntimeClass@@B"),
    ),
    (
        377,
        Some("?classCMapStringToString@CMapStringToString@@2UCRuntimeClass@@A"),
    ),
    (
        394,
        Some("?classCMapWordToOb@CMapWordToOb@@2UCRuntimeClass@@A"),
    ),
    (
        418,
        Some("?classCMapWordToPtr@CMapWordToPtr@@2UCRuntimeClass@@B"),
    ),
    (473, Some("?classCMemFile@CMemFile@@2UCRuntimeClass@@B")),
    (
        516,
        Some("?classCMemoryException@CMemoryException@@2UCRuntimeClass@@B"),
    ),
    (530, Some("?classCMenu@CMenu@@2UCRuntimeClass@@B")),
    (616, Some("?classCMutex@CMutex@@2UCRuntimeClass@@B")),
    (
        715,
        Some("?classCNotSupportedException@CNotSupportedException@@2UCRuntimeClass@@B"),
    ),
    (717, Some("?classCObArray@CObArray@@2UCRuntimeClass@@A")),
    (751, Some("?classCObList@CObList@@2UCRuntimeClass@@A")),
    (766, Some("?classCObject@CObject@@2UCRuntimeClass@@B")),
    (809, Some("?classCPaintDC@CPaintDC@@2UCRuntimeClass@@B")),
    (867, Some("?classCPalette@CPalette@@2UCRuntimeClass@@B")),
    (931, Some("?classCPen@CPen@@2UCRuntimeClass@@B")),
    (
        942,
        Some("?classCProgressCtrl@CProgressCtrl@@2UCRuntimeClass@@B"),
    ),
    (
        954,
        Some("?classCPropertyPage@CPropertyPage@@2UCRuntimeClass@@B"),
    ),
    (
        1052,
        Some("?classCPropertySheet@CPropertySheet@@2UCRuntimeClass@@B"),
    ),
    (1078, Some("?classCPtrArray@CPtrArray@@2UCRuntimeClass@@B")),
    (1081, Some("?classCPtrList@CPtrList@@2UCRuntimeClass@@B")),
    (1084, Some("?classCReBar@CReBar@@2UCRuntimeClass@@B")),
    (
        1085,
        Some("?classCReBarCtrl@CReBarCtrl@@2UCRuntimeClass@@B"),
    ),
    (
        1217,
        Some("?classCResourceException@CResourceException@@2UCRuntimeClass@@B"),
    ),
    (1252, Some("?classCRgn@CRgn@@2UCRuntimeClass@@B")),
    (
        1264,
        Some("?classCScrollBar@CScrollBar@@2UCRuntimeClass@@B"),
    ),
    (
        1266,
        Some("?classCScrollView@CScrollView@@2UCRuntimeClass@@B"),
    ),
    (
        1346,
        Some("?classCSingleDocTemplate@CSingleDocTemplate@@2UCRuntimeClass@@B"),
    ),
    (
        1348,
        Some("?classCSliderCtrl@CSliderCtrl@@2UCRuntimeClass@@B"),
    ),
    (1351, Some("?classCSocket@CSocket@@2UCRuntimeClass@@B")),
    (
        1365,
        Some("?classCSocketFile@CSocketFile@@2UCRuntimeClass@@B"),
    ),
    (
        1411,
        Some("?classCSpinButtonCtrl@CSpinButtonCtrl@@2UCRuntimeClass@@B"),
    ),
    (
        1417,
        Some("?classCSplitterWnd@CSplitterWnd@@2UCRuntimeClass@@B"),
    ),
    (1421, Some("?classCStatic@CStatic@@2UCRuntimeClass@@B")),
    (
        1451,
        Some("?classCStatusBar@CStatusBar@@2UCRuntimeClass@@B"),
    ),
    (
        1506,
        Some("?classCStatusBarCtrl@CStatusBarCtrl@@2UCRuntimeClass@@B"),
    ),
    (
        1546,
        Some("?classCStdioFile@CStdioFile@@2UCRuntimeClass@@B"),
    ),
    (
        1557,
        Some("?classCStringArray@CStringArray@@2UCRuntimeClass@@A"),
    ),
    (
        1593,
        Some("?classCStringList@CStringList@@2UCRuntimeClass@@A"),
    ),
    (
        1594,
        Some("?classCSyncObject@CSyncObject@@2UCRuntimeClass@@B"),
    ),
    (1802, Some("?classCTabCtrl@CTabCtrl@@2UCRuntimeClass@@B")),
    (1803, Some("?classCToolBar@CToolBar@@2UCRuntimeClass@@B")),
    (
        1807,
        Some("?classCToolBarCtrl@CToolBarCtrl@@2UCRuntimeClass@@B"),
    ),
    (1836, Some("?classCTreeCtrl@CTreeCtrl@@2UCRuntimeClass@@B")),
    (1856, Some("?classCTreeView@CTreeView@@2UCRuntimeClass@@B")),
    (
        1892,
        Some("?classCUIntArray@CUIntArray@@2UCRuntimeClass@@B"),
    ),
    (
        1893,
        Some("?classCUserException@CUserException@@2UCRuntimeClass@@B"),
    ),
    (1905, Some("?classCView@CView@@2UCRuntimeClass@@B")),
    (1941, Some("?classCWinApp@CWinApp@@2UCRuntimeClass@@B")),
    (
        1985,
        Some("?classCWinThread@CWinThread@@2UCRuntimeClass@@B"),
    ),
    (1986, Some("?classCWindowDC@CWindowDC@@2UCRuntimeClass@@B")),
    (1987, Some("?classCWnd@CWnd@@2UCRuntimeClass@@B")),
    (
        1988,
        Some("?classCWordArray@CWordArray@@2UCRuntimeClass@@A"),
    ),
    (
        1989,
        Some("?connectionMap@CCmdTarget@@1UAFX_CONNECTIONMAP@@B"),
    ),
    (1990, Some("?dispatchMap@CCmdTarget@@1UAFX_DISPMAP@@B")),
    (1991, Some("?dwStyleDefault@CEditView@@2KB")),
    (
        2074,
        Some("?eventsinkMap@CCmdTarget@@1UAFX_EVENTSINKMAP@@B"),
    ),
    (2112, Some("?g_bUseDST@@3HA")),
    (
        2393,
        Some("?interfaceMap@CCmdTarget@@1UAFX_INTERFACEMAP@@B"),
    ),
    (2454, Some("?messageMap@CCeCommandBar@@1UAFX_MSGMAP@@B")),
    (2520, Some("?messageMap@CCeDocList@@1UAFX_MSGMAP@@B")),
    (
        2537,
        Some("?messageMap@CCeSaveModifiedDlg@@1UAFX_MSGMAP@@B"),
    ),
    (2559, Some("?messageMap@CCmdTarget@@1UAFX_MSGMAP@@B")),
    (2560, Some("?messageMap@CComboBox@@1UAFX_MSGMAP@@B")),
    (2561, Some("?messageMap@CCommonDialog@@1UAFX_MSGMAP@@B")),
    (2562, Some("?messageMap@CControlBar@@1UAFX_MSGMAP@@B")),
    (2563, Some("?messageMap@CCtrlView@@1UAFX_MSGMAP@@B")),
    (2599, Some("?messageMap@CDialog@@1UAFX_MSGMAP@@B")),
    (2600, Some("?messageMap@CDialogBar@@1UAFX_MSGMAP@@B")),
    (2601, Some("?messageMap@CDocument@@1UAFX_MSGMAP@@B")),
    (2602, Some("?messageMap@CEditView@@1UAFX_MSGMAP@@B")),
    (
        2603,
        Some("?messageMap@CFindReplaceDialog@@1UAFX_MSGMAP@@B"),
    ),
    (2604, Some("?messageMap@CFormView@@1UAFX_MSGMAP@@B")),
    (2605, Some("?messageMap@CFrameWnd@@1UAFX_MSGMAP@@B")),
    (2606, Some("?messageMap@CListCtrl@@1UAFX_MSGMAP@@B")),
    (2607, Some("?messageMap@CListView@@1UAFX_MSGMAP@@B")),
    (2608, Some("?messageMap@CPropertyPage@@1UAFX_MSGMAP@@B")),
    (2609, Some("?messageMap@CPropertySheet@@1UAFX_MSGMAP@@B")),
    (2610, Some("?messageMap@CReBar@@1UAFX_MSGMAP@@B")),
    (2611, Some("?messageMap@CScrollView@@1UAFX_MSGMAP@@B")),
    (2612, Some("?messageMap@CSocketWnd@@1UAFX_MSGMAP@@B")),
    (2613, Some("?messageMap@CSplitterWnd@@1UAFX_MSGMAP@@B")),
    (2614, Some("?messageMap@CStatusBar@@1UAFX_MSGMAP@@B")),
    (2615, Some("?messageMap@CTabCtrl@@1UAFX_MSGMAP@@B")),
    (2616, Some("?messageMap@CToolBar@@1UAFX_MSGMAP@@B")),
    (2617, Some("?messageMap@CToolBarCtrl@@1UAFX_MSGMAP@@B")),
    (2618, Some("?messageMap@CTreeCtrl@@1UAFX_MSGMAP@@B")),
    (2627, Some("?messageMap@CTreeView@@1UAFX_MSGMAP@@B")),
    (2628, Some("?messageMap@CView@@1UAFX_MSGMAP@@B")),
    (2629, Some("?messageMap@CWinApp@@1UAFX_MSGMAP@@B")),
    (2630, Some("?messageMap@CWnd@@1UAFX_MSGMAP@@B")),
    (2631, Some("?nMaxSize@CEditView@@2IB")),
    (2632, Some("?rectDefault@CFrameWnd@@2VCRect@@B")),
    (2635, Some("?sizeDefault@CScrollView@@2UtagSIZE@@B")),
    (2636, Some("?wndBottom@CWnd@@2V1@B")),
    (2637, Some("?wndNoTopMost@CWnd@@2V1@B")),
    (2638, Some("?wndTop@CWnd@@2V1@B")),
    (2639, Some("?wndTopMost@CWnd@@2V1@B")),
    (16, None),
];

fn class_name<'a>(symbol: &'a str, prefix: &str, kind: &str) -> Option<&'a str> {
    let rest = symbol.strip_prefix(prefix)?;
    let (class, suffix) = rest.split_once('@')?;
    let suffix = suffix.strip_prefix(class)?.strip_prefix("@@")?;
    suffix.contains(kind).then_some(class)
}
fn resolve(ctx: &mut CallCtx<'_>, binding: &ImportBinding) -> Result<Option<u32>, KernelError> {
    let symbol = match binding {
        ImportBinding::Name(name) => {
            if !DATA
                .iter()
                .any(|(_, symbol)| *symbol == Some(name.as_str()))
            {
                if name.starts_with("?class") || name.starts_with("?messageMap@") {
                    return Err(KernelError::Loader(format!(
                        "Unsupported MFC data import: {name}"
                    )));
                }
                return Ok(None);
            }
            name.as_str()
        }
        ImportBinding::Ordinal(ordinal) => match DATA.iter().find(|(n, _)| n == ordinal) {
            Some((_, Some(name))) => name,
            Some((_, None)) => {
                return Err(KernelError::Loader(format!(
                    "Unsupported unnamed MFC data import: #{ordinal}"
                )))
            }
            None => return Ok(None),
        },
    };
    if matches!(
        symbol,
        "?dwStyleDefault@CEditView@@2KB" | "?nMaxSize@CEditView@@2IB"
    ) {
        let (pointer, fresh) =
            super::application::allocation(ctx, &format!("__PocketHLE_MFC_Constant:{symbol}"), 4)?;
        if fresh {
            put(
                ctx,
                pointer,
                if symbol.starts_with("?dwStyle") {
                    0x50b001c4
                } else {
                    0xfffff
                },
            )?;
        }
        return Ok(Some(pointer));
    }
    if symbol == "?rectDefault@CFrameWnd@@2VCRect@@B" {
        return Ok(Some(super::frame_loading::rect_default(ctx)?));
    }
    if let Some(class) = class_name(symbol, "?class", "UCRuntimeClass@@") {
        return Ok(Some(if class == "CFile" {
            super::file::class_data(ctx)? + 80
        } else {
            super::classes::runtime(ctx, class)?
        }));
    }
    if let Some(rest) = symbol.strip_prefix("?messageMap@") {
        if let Some((class, suffix)) = rest.split_once("@@") {
            if suffix.contains("UAFX_MSGMAP@@") {
                return Ok(Some(super::message_maps::address(ctx, class)?));
            }
        }
    }
    Err(KernelError::Loader(format!(
        "Unsupported MFC data import: {symbol}"
    )))
}
pub(super) fn bind(cpu: &mut dyn Cpu, kernel: &mut KernelState) -> Result<(), KernelError> {
    // Preserve the loader's whole-native-runtime preference. Mixing native
    // class data with HLE object layouts is never a supported fallback.
    if kernel.module_by_name("mfcce300.dll").is_some() {
        return Ok(());
    }
    let imports: Vec<_> = kernel
        .image_imports
        .iter()
        .filter(|import| import.dll.eq_ignore_ascii_case("mfcce300.dll"))
        .cloned()
        .collect();
    for import in imports {
        let thunk = pocket_kernel::Thunk {
            thunk_va: 0,
            iat_va: import.iat_va,
            dll: "mfcce300.dll".into(),
            binding: import.binding.clone(),
            friendly_name: None,
        };
        let mut ctx = CallCtx {
            cpu,
            kernel,
            thunk: &thunk,
        };
        if let Some(pointer) = resolve(&mut ctx, &import.binding)? {
            ctx.cpu.write_mem(import.iat_va, &pointer.to_le_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::LoadedModule;
    use pocket_pe::{ImportBinding, ImportSymbol};
    #[test]
    fn frame_default_rect_is_canonical_after_static_initialization() {
        let (mut cpu, mut k, _d) = framework_setup();
        for (address, binding) in [
            (0x1000, ImportBinding::Ordinal(2632)),
            (
                0x1004,
                ImportBinding::Name("?rectDefault@CFrameWnd@@2VCRect@@B".into()),
            ),
        ] {
            k.image_imports.push(ImportSymbol {
                dll: "mfcce300.dll".into(),
                binding,
                iat_va: address,
            });
        }
        super::bind(&mut cpu, &mut k).unwrap();
        let pointer = cpu.read_u32_le(0x1000).unwrap();
        assert_eq!(cpu.read_u32_le(0x1004).unwrap(), pointer);
        for (i, value) in [0u32, 0, 0x80000000, 0x80000000].into_iter().enumerate() {
            assert_eq!(cpu.read_u32_le(pointer + i as u32 * 4).unwrap(), value);
        }
        let remaining = k.heap.free_bytes();
        super::bind(&mut cpu, &mut k).unwrap();
        assert_eq!(k.heap.free_bytes(), remaining);
    }

    #[test]
    fn named_and_ordinal_data_imports_bind_canonical_descriptors_and_complete_message_maps() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let class = "?classCDialog@CDialog@@2UCRuntimeClass@@B";
        let map = "?messageMap@CFrameWnd@@1UAFX_MSGMAP@@B";
        for (address, binding) in [
            (0x1000, ImportBinding::Name(class.into())),
            (0x1004, ImportBinding::Ordinal(130)),
            (0x1008, ImportBinding::Name(map.into())),
            (0x100c, ImportBinding::Ordinal(201)),
        ] {
            cpu.write_mem(address, &0x70000000u32.to_le_bytes())
                .unwrap();
            k.image_imports.push(ImportSymbol {
                dll: "MFCce300.dll".into(),
                binding,
                iat_va: address,
            });
        }
        for (reg, value) in [
            (ArmReg::R0, 11),
            (ArmReg::R1, 22),
            (ArmReg::R2, 33),
            (ArmReg::R3, 44),
            (ArmReg::Lr, 0x6000),
        ] {
            cpu.write_reg(reg, value).unwrap();
        }
        super::bind(&mut cpu, &mut k).unwrap();
        assert_eq!(
            cpu.read_u32_le(0x1000).unwrap(),
            cpu.read_u32_le(0x1004).unwrap()
        );
        let descriptor = cpu.read_u32_le(0x1000).unwrap();
        assert_eq!(cpu.read_u32_le(descriptor + 4).unwrap(), 124);
        assert_eq!(cpu.read_u32_le(descriptor + 8).unwrap(), 0xffff);
        assert_eq!(cpu.read_u32_le(descriptor + 12).unwrap(), 0);
        let name = cpu.read_u32_le(descriptor).unwrap();
        assert_eq!(cpu.read_mem(name, 8).unwrap(), b"CDialog\0");
        let frame_map = cpu.read_u32_le(0x1008).unwrap();
        let getter = cpu.read_u32_le(frame_map).unwrap();
        assert_ne!(getter, 0);
        let entries = cpu.read_u32_le(frame_map + 4).unwrap();
        assert_eq!(cpu.read_u32_le(entries).unwrap(), 279);
        assert_eq!(cpu.read_u32_le(entries + 16).unwrap(), 22);
        assert_ne!(cpu.read_u32_le(entries + 20).unwrap(), 0);
        assert_eq!(cpu.read_mem(entries + 37 * 24, 24).unwrap(), [0; 24]);
        assert_eq!(cpu.read_u32_le(0x100c).unwrap(), 0x70000000);
        for (reg, value) in [
            (ArmReg::R0, 11),
            (ArmReg::R1, 22),
            (ArmReg::R2, 33),
            (ArmReg::R3, 44),
            (ArmReg::Lr, 0x6000),
        ] {
            assert_eq!(cpu.read_reg(reg).unwrap(), value);
        }
        let before = k.heap.free_bytes();
        super::bind(&mut cpu, &mut k).unwrap();
        assert_eq!(k.heap.free_bytes(), before);
        let base = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_MessageMapBase:CFrameWnd",
            &[],
        ));
        let wnd = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1380",
            &[],
        ));
        assert_eq!(base, wnd);
        let target = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_MessageMapBase:CWnd",
            &[],
        ));
        assert_eq!(
            target,
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "mfcce300.dll",
                "ord:1350",
                &[]
            ))
        );
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "__PocketHLE_MFC_MessageMapBase:CCmdTarget",
                &[]
            )),
            0
        );
    }
    #[test]
    fn whole_native_runtime_keeps_its_iat_and_unsupported_data_is_rejected_before_execution() {
        let (mut cpu, mut k, mut _d) = framework_setup();
        k.image_imports.push(ImportSymbol {
            dll: "mfcce300.dll".into(),
            binding: ImportBinding::Name("?classUnknown@Unknown@@2UCRuntimeClass@@B".into()),
            iat_va: 0x1000,
        });
        cpu.write_mem(0x1000, &0x31000000u32.to_le_bytes()).unwrap();
        assert!(super::bind(&mut cpu, &mut k).is_err());
        k.modules.push(LoadedModule {
            handle: 0x30000000,
            base: 0x30000000,
            name: "mfcce300.dll".into(),
            dependencies: vec![],
            attached: true,
            satellites: vec![],
            resident_regions: vec![],
            guest_path: String::new(),
            image_size: 4096,
            image_entry: 0,
            resources: vec![],
            refcount: 1,
        });
        super::bind(&mut cpu, &mut k).unwrap();
        assert_eq!(cpu.read_u32_le(0x1000).unwrap(), 0x31000000);
    }
}
