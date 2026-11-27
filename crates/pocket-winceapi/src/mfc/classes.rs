//! Declarative CE3 ARM class layout and guest-callable virtual method tables.
//! Layout metadata is shared by native-free imports and checkpointed objects.
use super::*;
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        961,
        "?CreateObject@CByteArray@@SAPAVCObject@@XZ",
        exported_metadata,
    ),
    (
        1444,
        "?GetRuntimeClass@CByteArray@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        411,
        "?_GetBaseClass@CByteArray@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        1442,
        "?GetRuntimeClass@CBrush@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        409,
        "?_GetBaseClass@CBrush@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        1514,
        "?GetRuntimeClass@CResourceException@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        481,
        "?_GetBaseClass@CResourceException@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        2738,
        "?GetRuntimeClass@CCeDocListDocTemplate@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        2703,
        "?_GetBaseClass@CCeDocListDocTemplate@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        2737,
        "?GetRuntimeClass@CCeDocListCB@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        2702,
        "?_GetBaseClass@CCeDocListCB@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        2736,
        "?GetRuntimeClass@CCeDocList@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        2701,
        "?_GetBaseClass@CCeDocList@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        964,
        "?CreateObject@CEditView@@SAPAVCObject@@XZ",
        exported_metadata,
    ),
    (
        1456,
        "?GetRuntimeClass@CCtrlView@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        1466,
        "?GetRuntimeClass@CEditView@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        423,
        "?_GetBaseClass@CCtrlView@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        433,
        "?_GetBaseClass@CEditView@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        963,
        "?CreateObject@CDWordArray@@SAPAVCObject@@XZ",
        exported_metadata,
    ),
    (
        970,
        "?CreateObject@CMapStringToString@@SAPAVCObject@@XZ",
        exported_metadata,
    ),
    (
        971,
        "?CreateObject@CMapWordToOb@@SAPAVCObject@@XZ",
        exported_metadata,
    ),
    (
        974,
        "?CreateObject@CObList@@SAPAVCObject@@XZ",
        exported_metadata,
    ),
    (
        977,
        "?CreateObject@CStringList@@SAPAVCObject@@XZ",
        exported_metadata,
    ),
    (
        1492,
        "?GetRuntimeClass@CMapStringToString@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        1493,
        "?GetRuntimeClass@CMapWordToOb@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        1502,
        "?GetRuntimeClass@CObList@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        1529,
        "?GetRuntimeClass@CStringList@@UBAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        430,
        "?_GetBaseClass@CDWordArray@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        459,
        "?_GetBaseClass@CMapStringToString@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        460,
        "?_GetBaseClass@CMapWordToOb@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        469,
        "?_GetBaseClass@CObList@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
    (
        496,
        "?_GetBaseClass@CStringList@@KAPAUCRuntimeClass@@XZ",
        exported_metadata,
    ),
];
const PREFIX: &str = "__PocketHLE_MFC_Class:";
struct Class {
    name: &'static str,
    base: Option<&'static str>,
    runtime_alias: Option<&'static str>,
    size: u32,
    schema: u32,
    factory: Option<&'static str>,
    methods: &'static [&'static str],
}
const CWINAPP_METHODS: &[&str] = &[
    "?GetRuntimeClass@CWinApp@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CWinApp@@UBAXXZ",
    "?Dump@CWinApp@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CCmdTarget@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CCmdTarget@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CWinApp@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?InitInstance@CWinApp@@UAAHXZ",
    "?Run@CWinApp@@UAAHXZ",
    "?PreTranslateMessage@CWinThread@@UAAHPAUtagMSG@@@Z",
    "?PumpMessage@CWinThread@@UAAHXZ",
    "?OnIdle@CWinApp@@UAAHJ@Z",
    "?IsIdleMessage@CWinThread@@UAAHPAUtagMSG@@@Z",
    "?ExitInstance@CWinApp@@UAAHXZ",
    "?ProcessWndProcException@CWinApp@@UAAJPAVCException@@PBUtagMSG@@@Z",
    "?GetMainWnd@CWinThread@@UAAPAVCWnd@@XZ",
    "?Delete@CWinThread@@UAAXXZ",
    "?OpenDocumentFile@CWinApp@@UAAPAVCDocument@@PBG@Z",
    "?AddToRecentFileList@CWinApp@@UAAXPBG@Z",
    "?InitApplication@CWinApp@@UAAHXZ",
    "?SaveAllModified@CWinApp@@UAAHXZ",
    "?DoMessageBox@CWinApp@@UAAHPBGII@Z",
    "?DoWaitCursor@CWinApp@@UAAXH@Z",
    "?OnDDECommand@CWinApp@@UAAHPAG@Z",
    "?WinHelp@CWinApp@@UAAXKI@Z",
];
const CWINTHREAD_METHODS: &[&str] = &[
    "?GetRuntimeClass@CWinThread@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CWinThread@@UBAXXZ",
    "?Dump@CWinThread@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CCmdTarget@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CCmdTarget@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CCmdTarget@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?InitInstance@CWinThread@@UAAHXZ",
    "?Run@CWinThread@@UAAHXZ",
    "?PreTranslateMessage@CWinThread@@UAAHPAUtagMSG@@@Z",
    "?PumpMessage@CWinThread@@UAAHXZ",
    "?OnIdle@CWinThread@@UAAHJ@Z",
    "?IsIdleMessage@CWinThread@@UAAHPAUtagMSG@@@Z",
    "?ExitInstance@CWinThread@@UAAHXZ",
    "?ProcessWndProcException@CWinThread@@UAAJPAVCException@@PBUtagMSG@@@Z",
    "?GetMainWnd@CWinThread@@UAAPAVCWnd@@XZ",
    "?Delete@CWinThread@@UAAXXZ",
];
const CCMDTARGET_METHODS: &[&str] = &[
    "?GetRuntimeClass@CCmdTarget@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CCmdTarget@@UBAXXZ",
    "?Dump@CCmdTarget@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CCmdTarget@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CCmdTarget@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CCmdTarget@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
];
const CSTRINGARRAY_METHODS: &[&str] = &[
    "?GetRuntimeClass@CStringArray@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CStringArray@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CStringArray@@UBAXXZ",
    "?Dump@CStringArray@@UBAXAAVCDumpContext@@@Z",
];
const COBJECT_METHODS: &[&str] = &[
    "?GetRuntimeClass@CObject@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
];
const CMAPSTRINGTOOB_METHODS: &[&str] = &[
    "?GetRuntimeClass@CMapStringToOb@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CMapStringToOb@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CMapStringToOb@@UBAXXZ",
    "?Dump@CMapStringToOb@@UBAXAAVCDumpContext@@@Z",
];

const CMAPWORDTOOB_METHODS: &[&str] = &[
    "?GetRuntimeClass@CMapWordToOb@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CMapWordToOb@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CMapWordToOb@@UBAXXZ",
    "?Dump@CMapWordToOb@@UBAXAAVCDumpContext@@@Z",
];

const CMAPSTRINGTOSTRING_METHODS: &[&str] = &[
    "?GetRuntimeClass@CMapStringToString@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CMapStringToString@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CMapStringToString@@UBAXXZ",
    "?Dump@CMapStringToString@@UBAXAAVCDumpContext@@@Z",
];
const CCOMMANDLINEINFO_METHODS: &[&str] = &[
    "?GetRuntimeClass@CObject@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?ParseParam@CCommandLineInfo@@UAAXPBDHH@Z",
    "?ParseParam@CCommandLineInfo@@UAAXPBGHH@Z",
];
const CDC_METHODS: &[&str] = &[
    "?GetRuntimeClass@CDC@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?SetAttribDC@CDC@@UAAXPAUHDC__@@@Z",
    "?SetOutputDC@CDC@@UAAXPAUHDC__@@@Z",
    "?ReleaseAttribDC@CDC@@UAAXXZ",
    "?ReleaseOutputDC@CDC@@UAAXXZ",
    "?SaveDC@CDC@@UAAHXZ",
    "?RestoreDC@CDC@@UAAHH@Z",
    "?SelectStockObject@CDC@@UAAPAVCGdiObject@@H@Z",
    "?SelectObject@CDC@@UAAPAVCFont@@PAV2@@Z",
    "?SetBkColor@CDC@@UAAKK@Z",
    "?SetTextColor@CDC@@UAAKK@Z",
    "?GetClipBox@CDC@@UBAHPAUtagRECT@@@Z",
    "?RectVisible@CDC@@UBAHPBUtagRECT@@@Z",
    "?ExtTextOutW@CDC@@UAAHHHIPBUtagRECT@@PBGIPAH@Z",
    "?DrawTextW@CDC@@UAAHPBGHPAUtagRECT@@I@Z",
];
const CBYTEARRAY_METHODS: &[&str] = &[
    "?GetRuntimeClass@CByteArray@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CByteArray@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
];
const COBARRAY_METHODS: &[&str] = &[
    "?GetRuntimeClass@CObArray@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObArray@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
];
const CWORDARRAY_METHODS: &[&str] = &[
    "?GetRuntimeClass@CWordArray@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CWordArray@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CWordArray@@UBAXXZ",
    "?Dump@CWordArray@@UBAXAAVCDumpContext@@@Z",
];
const CDWORDARRAY_METHODS: &[&str] = &[
    "?GetRuntimeClass@CDWordArray@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CDWordArray@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CDWordArray@@UBAXXZ",
    "?Dump@CDWordArray@@UBAXAAVCDumpContext@@@Z",
];
const CWND_METHODS: &[&str] = &[
    "?GetRuntimeClass@CWnd@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CWnd@@UBAXXZ",
    "?Dump@CWnd@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CCmdTarget@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CWnd@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CWnd@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?PreSubclassWindow@CWnd@@UAAXXZ",
    "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
    "?DestroyWindow@CWnd@@UAAHXZ",
    "?PreCreateWindow@CWnd@@UAAHAAUtagCREATESTRUCTW@@@Z",
    "?CalcWindowRect@CWnd@@UAAXPAUtagRECT@@I@Z",
    "?GetScrollBarCtrl@CWnd@@UBAPAVCScrollBar@@H@Z",
    "?WinHelp@CWnd@@UAAXKI@Z",
    "?ContinueModal@CWnd@@UAAHXZ",
    "?EndModalLoop@CWnd@@UAAXH@Z",
    "?OnCommand@CWnd@@MAAHIJ@Z",
    "?OnNotify@CWnd@@MAAHIJPAJ@Z",
    "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
    "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
    "?BeginModalState@CWnd@@UAAXXZ",
    "?EndModalState@CWnd@@UAAXXZ",
    "?PreTranslateMessage@CWnd@@UAAHPAUtagMSG@@@Z",
    "?OnAmbientProperty@CWnd@@UAAHPAVCOleControlSite@@JPAUtagVARIANT@@@Z",
    "?WindowProc@CWnd@@MAAJIIJ@Z",
    "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z",
    "?DefWindowProcW@CWnd@@MAAJIIJ@Z",
    "?PostNcDestroy@CWnd@@MAAXXZ",
    "?OnChildNotify@CWnd@@MAAHIIJPAJ@Z",
    "?CheckAutoCenter@CWnd@@UAAHXZ",
    "?IsFrameWnd@CWnd@@UBAHXZ",
    "?SetOccDialogInfo@CWnd@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
];
const CSPLITTER_METHODS: [&str; 72] = {
    let mut methods = [""; 72];
    let mut index = 0;
    while index < CWND_METHODS.len() {
        methods[index] = CWND_METHODS[index];
        index += 1;
    }
    methods[0] = "?GetRuntimeClass@CSplitterWnd@@UBAPAUCRuntimeClass@@XZ";
    methods[3] = "?AssertValid@CObject@@UBAXXZ";
    methods[4] = "?Dump@CObject@@UBAXAAVCDumpContext@@@Z";
    methods[12] = "?GetMessageMap@CSplitterWnd@@MBAPBUAFX_MSGMAP@@XZ";
    methods[30] = "?OnCommand@CSplitterWnd@@MAAHIJ@Z";
    methods[31] = "?OnNotify@CSplitterWnd@@MAAHIJPAJ@Z";
    methods[46] =
        "?CreateView@CSplitterWnd@@UAAHHHPAUCRuntimeClass@@UtagSIZE@@PAUCCreateContext@@@Z";
    methods[47] = "?RecalcLayout@CSplitterWnd@@UAAXXZ";
    methods[48] = "?OnDrawSplitter@CSplitterWnd@@MAAXPAVCDC@@W4ESplitType@1@ABVCRect@@@Z";
    methods[49] = "?OnInvertTracker@CSplitterWnd@@MAAXABVCRect@@@Z";
    methods[50] = "?CreateScrollBarCtrl@CSplitterWnd@@UAAHKI@Z";
    methods[51] = "?DeleteView@CSplitterWnd@@UAAXHH@Z";
    methods[52] = "?SplitRow@CSplitterWnd@@UAAHH@Z";
    methods[53] = "?SplitColumn@CSplitterWnd@@UAAHH@Z";
    methods[54] = "?DeleteRow@CSplitterWnd@@UAAXH@Z";
    methods[55] = "?DeleteColumn@CSplitterWnd@@UAAXH@Z";
    methods[56] = "?GetActivePane@CSplitterWnd@@UAAPAVCWnd@@PAH0@Z";
    methods[57] = "?SetActivePane@CSplitterWnd@@UAAXHHPAVCWnd@@@Z";
    methods[58] = "?CanActivateNext@CSplitterWnd@@UAAHH@Z";
    methods[59] = "?ActivateNext@CSplitterWnd@@UAAXH@Z";
    methods[60] = "?DoKeyboardSplit@CSplitterWnd@@UAAHXZ";
    methods[61] = "?DoScroll@CSplitterWnd@@UAAHPAVCView@@IH@Z";
    methods[62] = "?DoScrollBy@CSplitterWnd@@UAAHPAVCView@@VCSize@@H@Z";
    methods[63] = "?HitTest@CSplitterWnd@@MBAHVCPoint@@@Z";
    methods[64] = "?GetInsideRect@CSplitterWnd@@MBAXAAVCRect@@@Z";
    methods[65] = "?GetHitRect@CSplitterWnd@@MAAXHAAVCRect@@@Z";
    methods[66] = "?TrackRowSize@CSplitterWnd@@MAAXHH@Z";
    methods[67] = "?TrackColumnSize@CSplitterWnd@@MAAXHH@Z";
    methods[68] = "?DrawAllSplitBars@CSplitterWnd@@MAAXPAVCDC@@HH@Z";
    methods[69] = "?SetSplitCursor@CSplitterWnd@@MAAXH@Z";
    methods[70] = "?StartTracking@CSplitterWnd@@MAAXH@Z";
    methods[71] = "?StopTracking@CSplitterWnd@@MAAXH@Z";
    methods
};
const CVIEW_METHODS: [&str; 55] = {
    let mut methods = [""; 55];
    let mut i = 0;
    while i < 46 {
        methods[i] = CWND_METHODS[i];
        i += 1;
    }
    methods[0] = "?GetRuntimeClass@CView@@UBAPAUCRuntimeClass@@XZ";
    methods[3] = "?AssertValid@CObject@@UBAXXZ";
    methods[4] = "?Dump@CObject@@UBAXAAVCDumpContext@@@Z";
    methods[5] = "?OnCmdMsg@CView@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z";
    methods[12] = "?GetMessageMap@CView@@MBAPBUAFX_MSGMAP@@XZ";
    methods[24] = "?PreCreateWindow@CView@@MAAHAAUtagCREATESTRUCTW@@@Z";
    methods[25] = "?CalcWindowRect@CView@@UAAXPAUtagRECT@@I@Z";
    methods[26] = "?GetScrollBarCtrl@CView@@UBAPAVCScrollBar@@H@Z";
    methods[41] = "?PostNcDestroy@CView@@MAAXXZ";
    methods[46] = "?IsSelected@CView@@UBAHPBVCObject@@@Z";
    methods[47] = "?OnScroll@CView@@UAAHIIH@Z";
    methods[48] = "?OnScrollBy@CView@@UAAHVCSize@@H@Z";
    methods[49] = "?OnPrepareDC@CView@@UAAXPAVCDC@@PBX@Z";
    methods[50] = "?OnInitialUpdate@CView@@UAAXXZ";
    methods[51] = "?OnActivateView@CView@@MAAXHPAV1@0@Z";
    methods[52] = "?OnActivateFrame@CView@@MAAXIPAVCFrameWnd@@@Z";
    methods[53] = "?OnUpdate@CView@@MAAXPAV1@JPAVCObject@@@Z";
    methods[54] = "PureVirtualDraw";
    methods
};
const CCTRLVIEW_METHODS: &[&str] = &[
    "?GetRuntimeClass@CCtrlView@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CView@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CWnd@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CCtrlView@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?PreSubclassWindow@CWnd@@UAAXXZ",
    "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
    "?DestroyWindow@CWnd@@UAAHXZ",
    "?PreCreateWindow@CCtrlView@@MAAHAAUtagCREATESTRUCTW@@@Z",
    "?CalcWindowRect@CView@@UAAXPAUtagRECT@@I@Z",
    "?GetScrollBarCtrl@CView@@UBAPAVCScrollBar@@H@Z",
    "?WinHelp@CWnd@@UAAXKI@Z",
    "?ContinueModal@CWnd@@UAAHXZ",
    "?EndModalLoop@CWnd@@UAAXH@Z",
    "?OnCommand@CWnd@@MAAHIJ@Z",
    "?OnNotify@CWnd@@MAAHIJPAJ@Z",
    "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
    "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
    "?BeginModalState@CWnd@@UAAXXZ",
    "?EndModalState@CWnd@@UAAXXZ",
    "?PreTranslateMessage@CWnd@@UAAHPAUtagMSG@@@Z",
    "?OnAmbientProperty@CWnd@@UAAHPAVCOleControlSite@@JPAUtagVARIANT@@@Z",
    "?WindowProc@CWnd@@MAAJIIJ@Z",
    "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z",
    "?DefWindowProcW@CWnd@@MAAJIIJ@Z",
    "?PostNcDestroy@CView@@MAAXXZ",
    "?OnChildNotify@CWnd@@MAAHIIJPAJ@Z",
    "?CheckAutoCenter@CWnd@@UAAHXZ",
    "?IsFrameWnd@CWnd@@UBAHXZ",
    "?SetOccDialogInfo@CWnd@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
    "?IsSelected@CView@@UBAHPBVCObject@@@Z",
    "?OnScroll@CView@@UAAHIIH@Z",
    "?OnScrollBy@CView@@UAAHVCSize@@H@Z",
    "?OnPrepareDC@CView@@UAAXPAVCDC@@PBX@Z",
    "?OnInitialUpdate@CView@@UAAXXZ",
    "?OnActivateView@CView@@MAAXHPAV1@0@Z",
    "?OnActivateFrame@CView@@MAAXIPAVCFrameWnd@@@Z",
    "?OnUpdate@CView@@MAAXPAV1@JPAVCObject@@@Z",
    "?OnDraw@CCtrlView@@MAAXPAVCDC@@@Z",
];
const CEDITVIEW_METHODS: &[&str] = &[
    "?GetRuntimeClass@CEditView@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CEditView@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CView@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CWnd@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CEditView@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?PreSubclassWindow@CWnd@@UAAXXZ",
    "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
    "?DestroyWindow@CWnd@@UAAHXZ",
    "?PreCreateWindow@CEditView@@MAAHAAUtagCREATESTRUCTW@@@Z",
    "?CalcWindowRect@CEditView@@MAAXPAUtagRECT@@I@Z",
    "?GetScrollBarCtrl@CView@@UBAPAVCScrollBar@@H@Z",
    "?WinHelp@CWnd@@UAAXKI@Z",
    "?ContinueModal@CWnd@@UAAHXZ",
    "?EndModalLoop@CWnd@@UAAXH@Z",
    "?OnCommand@CWnd@@MAAHIJ@Z",
    "?OnNotify@CWnd@@MAAHIJPAJ@Z",
    "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
    "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
    "?BeginModalState@CWnd@@UAAXXZ",
    "?EndModalState@CWnd@@UAAXXZ",
    "?PreTranslateMessage@CWnd@@UAAHPAUtagMSG@@@Z",
    "?OnAmbientProperty@CWnd@@UAAHPAVCOleControlSite@@JPAUtagVARIANT@@@Z",
    "?WindowProc@CWnd@@MAAJIIJ@Z",
    "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z",
    "?DefWindowProcW@CWnd@@MAAJIIJ@Z",
    "?PostNcDestroy@CView@@MAAXXZ",
    "?OnChildNotify@CWnd@@MAAHIIJPAJ@Z",
    "?CheckAutoCenter@CWnd@@UAAHXZ",
    "?IsFrameWnd@CWnd@@UBAHXZ",
    "?SetOccDialogInfo@CWnd@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
    "?IsSelected@CView@@UBAHPBVCObject@@@Z",
    "?OnScroll@CView@@UAAHIIH@Z",
    "?OnScrollBy@CView@@UAAHVCSize@@H@Z",
    "?OnPrepareDC@CView@@UAAXPAVCDC@@PBX@Z",
    "?OnInitialUpdate@CView@@UAAXXZ",
    "?OnActivateView@CView@@MAAXHPAV1@0@Z",
    "?OnActivateFrame@CView@@MAAXIPAVCFrameWnd@@@Z",
    "?OnUpdate@CView@@MAAXPAV1@JPAVCObject@@@Z",
    "?OnDraw@CCtrlView@@MAAXPAVCDC@@@Z",
    "?OnFindNext@CEditView@@MAAXPBGHH@Z",
    "?OnReplaceSel@CEditView@@MAAXPBGHH0@Z",
    "?OnReplaceAll@CEditView@@MAAXPBG0H@Z",
    "?OnTextNotFound@CEditView@@MAAXPBG@Z",
    "?DeleteContents@CEditView@@UAAXXZ",
];
const CCEDOCLIST_METHODS: &[&str] = &[
    "?GetRuntimeClass@CCeDocList@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CCmdTarget@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CWnd@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CCeDocList@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?PreSubclassWindow@CWnd@@UAAXXZ",
    "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
    "?DestroyWindow@CWnd@@UAAHXZ",
    "?PreCreateWindow@CWnd@@UAAHAAUtagCREATESTRUCTW@@@Z",
    "?CalcWindowRect@CWnd@@UAAXPAUtagRECT@@I@Z",
    "?GetScrollBarCtrl@CWnd@@UBAPAVCScrollBar@@H@Z",
    "?WinHelp@CWnd@@UAAXKI@Z",
    "?ContinueModal@CWnd@@UAAHXZ",
    "?EndModalLoop@CWnd@@UAAXH@Z",
    "?OnCommand@CWnd@@MAAHIJ@Z",
    "?OnNotify@CWnd@@MAAHIJPAJ@Z",
    "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
    "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
    "?BeginModalState@CWnd@@UAAXXZ",
    "?EndModalState@CWnd@@UAAXXZ",
    "?PreTranslateMessage@CWnd@@UAAHPAUtagMSG@@@Z",
    "?OnAmbientProperty@CWnd@@UAAHPAVCOleControlSite@@JPAUtagVARIANT@@@Z",
    "?WindowProc@CWnd@@MAAJIIJ@Z",
    "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z",
    "?DefWindowProcW@CWnd@@MAAJIIJ@Z",
    "?PostNcDestroy@CWnd@@MAAXXZ",
    "?OnChildNotify@CWnd@@MAAHIIJPAJ@Z",
    "?CheckAutoCenter@CWnd@@UAAHXZ",
    "?IsFrameWnd@CWnd@@UBAHXZ",
    "?SetOccDialogInfo@CWnd@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
    "?OnCreate@CCeDocList@@UAAXXZ",
    "?OnClose@CCeDocList@@UAAXXZ",
];
const CDOCUMENT_METHODS: [&str; 45] = {
    let mut methods = [""; 45];
    let mut i = 0;
    while i < 21 {
        methods[i] = CCMDTARGET_METHODS[i];
        i += 1;
    }
    methods[0] = "?GetRuntimeClass@CDocument@@UBAPAUCRuntimeClass@@XZ";
    methods[3] = "?AssertValid@CObject@@UBAXXZ";
    methods[4] = "?Dump@CObject@@UBAXAAVCDumpContext@@@Z";
    methods[5] = "?OnCmdMsg@CDocument@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z";
    methods[6] = "?OnFinalRelease@CDocument@@UAAXXZ";
    methods[12] = "?GetMessageMap@CDocument@@MBAPBUAFX_MSGMAP@@XZ";
    methods[21] = "?SetTitle@CDocument@@UAAXPBG@Z";
    methods[22] = "?SetPathName@CDocument@@UAAXPBGH@Z";
    methods[23] = "?IsModified@CDocument@@UAAHXZ";
    methods[24] = "?SetModifiedFlag@CDocument@@UAAXH@Z";
    methods[25] = "?GetFirstViewPosition@CDocument@@UBAPAU__POSITION@@XZ";
    methods[26] = "?GetNextView@CDocument@@UBAPAVCView@@AAPAU__POSITION@@@Z";
    methods[27] = "?OnChangedViewList@CDocument@@UAAXXZ";
    methods[28] = "?DeleteContents@CDocument@@UAAXXZ";
    methods[29] = "?OnNewDocument@CDocument@@UAAHXZ";
    methods[30] = "?OnOpenDocument@CDocument@@UAAHPBG@Z";
    methods[31] = "?OnSaveDocument@CDocument@@UAAHPBG@Z";
    methods[32] = "?OnCloseDocument@CDocument@@UAAXXZ";
    methods[33] = "?ReportSaveLoadException@CDocument@@UAAXPBGPAVCException@@HI@Z";
    methods[34] = "?GetFile@CDocument@@UAAPAVCFile@@PBGIPAVCFileException@@@Z";
    methods[35] = "?ReleaseFile@CDocument@@UAAXPAVCFile@@H@Z";
    methods[36] = "?CanCloseFrame@CDocument@@UAAHPAVCFrameWnd@@@Z";
    methods[37] = "?SaveModified@CDocument@@UAAHXZ";
    methods[38] = "?PreCloseFrame@CDocument@@UAAXPAVCFrameWnd@@@Z";
    methods[39] = "?DoSave@CDocument@@UAAHPBGH@Z";
    methods[40] = "?DoFileSave@CDocument@@UAAHXZ";
    methods[41] = "?UpdateFrameCounts@CDocument@@UAAXXZ";
    methods[42] = "?GetDefaultMenu@CDocument@@UAAPAUHMENU__@@XZ";
    methods[43] = "?GetDefaultAccelerator@CDocument@@UAAPAUHACCEL__@@XZ";
    methods[44] = "?OnIdle@CDocument@@UAAXXZ";
    methods
};
const CFRAMEWND_METHODS: &[&str] = &[
    "?GetRuntimeClass@CFrameWnd@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CFrameWnd@@UBAXXZ",
    "?Dump@CFrameWnd@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CFrameWnd@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CWnd@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CFrameWnd@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?PreSubclassWindow@CWnd@@UAAXXZ",
    "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
    "?DestroyWindow@CWnd@@UAAHXZ",
    "?PreCreateWindow@CFrameWnd@@MAAHAAUtagCREATESTRUCTW@@@Z",
    "?CalcWindowRect@CWnd@@UAAXPAUtagRECT@@I@Z",
    "?GetScrollBarCtrl@CWnd@@UBAPAVCScrollBar@@H@Z",
    "?WinHelp@CWnd@@UAAXKI@Z",
    "?ContinueModal@CWnd@@UAAHXZ",
    "?EndModalLoop@CWnd@@UAAXH@Z",
    "?OnCommand@CFrameWnd@@MAAHIJ@Z",
    "?OnNotify@CWnd@@MAAHIJPAJ@Z",
    "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
    "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
    "?BeginModalState@CFrameWnd@@UAAXXZ",
    "?EndModalState@CFrameWnd@@UAAXXZ",
    "?PreTranslateMessage@CFrameWnd@@UAAHPAUtagMSG@@@Z",
    "?OnAmbientProperty@CWnd@@UAAHPAVCOleControlSite@@JPAUtagVARIANT@@@Z",
    "?WindowProc@CWnd@@MAAJIIJ@Z",
    "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z",
    "?DefWindowProcW@CWnd@@MAAJIIJ@Z",
    "?PostNcDestroy@CFrameWnd@@MAAXXZ",
    "?OnChildNotify@CWnd@@MAAHIIJPAJ@Z",
    "?CheckAutoCenter@CWnd@@UAAHXZ",
    "?IsFrameWnd@CFrameWnd@@UBAHXZ",
    "?SetOccDialogInfo@CWnd@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
    "?LoadFrame@CFrameWnd@@UAAHIKPAVCWnd@@PAUCCreateContext@@@Z",
    "?GetActiveDocument@CFrameWnd@@UAAPAVCDocument@@XZ",
    "?GetActiveFrame@CFrameWnd@@UAAPAV1@XZ",
    "?GetMessageString@CFrameWnd@@UBAXIAAVCString@@@Z",
    "?RecalcLayout@CFrameWnd@@UAAXH@Z",
    "?ActivateFrame@CFrameWnd@@UAAXH@Z",
    "?GetMessageBar@CFrameWnd@@UAAPAVCWnd@@XZ",
    "?NegotiateBorderSpace@CFrameWnd@@UAAHIPAUtagRECT@@@Z",
    "?OnCreateClient@CFrameWnd@@MAAHPAUtagCREATESTRUCTW@@PAUCCreateContext@@@Z",
    "?OnUpdateFrameTitle@CFrameWnd@@UAAXH@Z",
    "?OnUpdateFrameMenu@CFrameWnd@@UAAXPAUHMENU__@@@Z",
    "?GetDefaultAccelerator@CFrameWnd@@UAAPAUHACCEL__@@XZ",
    "?DelayUpdateFrameMenu@CFrameWnd@@UAAXPAUHMENU__@@@Z",
];
const CPTRLIST_METHODS: &[&str] = &[
    "?GetRuntimeClass@CPtrList@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CPtrList@@UBAXXZ",
    "?Dump@CPtrList@@UBAXAAVCDumpContext@@@Z",
];

const CSTRINGLIST_METHODS: &[&str] = &[
    "?GetRuntimeClass@CStringList@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CStringList@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CStringList@@UBAXXZ",
    "?Dump@CStringList@@UBAXAAVCDumpContext@@@Z",
];

const COBLIST_METHODS: &[&str] = &[
    "?GetRuntimeClass@CObList@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObList@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObList@@UBAXXZ",
    "?Dump@CObList@@UBAXAAVCDumpContext@@@Z",
];
const CSYNCOBJECT_METHODS: &[&str] = &[
    "?GetRuntimeClass@CSyncObject@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CSyncObject@@UBAXXZ",
    "?Dump@CSyncObject@@UBAXAAVCDumpContext@@@Z",
    "?Lock@CSyncObject@@UAAHK@Z",
    "?Unlock@CSyncObject@@UAAHJPAJ@Z",
    "PureVirtual",
];
const CEVENT_METHODS: &[&str] = &[
    "?GetRuntimeClass@CEvent@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?Lock@CSyncObject@@UAAHK@Z",
    "?Unlock@CSyncObject@@UAAHJPAJ@Z",
    "?Unlock@CEvent@@UAAHXZ",
];
const CDIALOG_METHODS: &[&str] = &[
    "?GetRuntimeClass@CDialog@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CDialog@@UBAXXZ",
    "?Dump@CDialog@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CDialog@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CWnd@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CDialog@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?PreSubclassWindow@CWnd@@UAAXXZ",
    "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
    "?DestroyWindow@CWnd@@UAAHXZ",
    "?PreCreateWindow@CWnd@@UAAHAAUtagCREATESTRUCTW@@@Z",
    "?CalcWindowRect@CWnd@@UAAXPAUtagRECT@@I@Z",
    "?GetScrollBarCtrl@CWnd@@UBAPAVCScrollBar@@H@Z",
    "?WinHelp@CWnd@@UAAXKI@Z",
    "?ContinueModal@CWnd@@UAAHXZ",
    "?EndModalLoop@CWnd@@UAAXH@Z",
    "?OnCommand@CWnd@@MAAHIJ@Z",
    "?OnNotify@CWnd@@MAAHIJPAJ@Z",
    "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
    "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
    "?BeginModalState@CWnd@@UAAXXZ",
    "?EndModalState@CWnd@@UAAXXZ",
    "?PreTranslateMessage@CDialog@@UAAHPAUtagMSG@@@Z",
    "?OnAmbientProperty@CWnd@@UAAHPAVCOleControlSite@@JPAUtagVARIANT@@@Z",
    "?WindowProc@CWnd@@MAAJIIJ@Z",
    "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z",
    "?DefWindowProcW@CWnd@@MAAJIIJ@Z",
    "?PostNcDestroy@CWnd@@MAAXXZ",
    "?OnChildNotify@CWnd@@MAAHIIJPAJ@Z",
    "?CheckAutoCenter@CDialog@@UAAHXZ",
    "?IsFrameWnd@CWnd@@UBAHXZ",
    "?SetOccDialogInfo@CDialog@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
    "?DoModal@CDialog@@UAAHXZ",
    "?OnInitDialog@CDialog@@UAAHXZ",
    "?OnSetFont@CDialog@@UAAXPAVCFont@@@Z",
    "?OnOK@CDialog@@MAAXXZ",
    "?OnCancel@CDialog@@MAAXXZ",
    "?PreInitDialog@CDialog@@MAAXXZ",
];
const CLISTCTRL_METHODS: &[&str] = &[
    "?GetRuntimeClass@CListCtrl@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CWnd@@UBAXXZ",
    "?Dump@CWnd@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CCmdTarget@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CWnd@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CListCtrl@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?PreSubclassWindow@CWnd@@UAAXXZ",
    "?Create@CWnd@@UAAHPBG0KABUtagRECT@@PAV1@IPAUCCreateContext@@@Z",
    "?DestroyWindow@CWnd@@UAAHXZ",
    "?PreCreateWindow@CWnd@@UAAHAAUtagCREATESTRUCTW@@@Z",
    "?CalcWindowRect@CWnd@@UAAXPAUtagRECT@@I@Z",
    "?GetScrollBarCtrl@CWnd@@UBAPAVCScrollBar@@H@Z",
    "?WinHelp@CWnd@@UAAXKI@Z",
    "?ContinueModal@CWnd@@UAAHXZ",
    "?EndModalLoop@CWnd@@UAAXH@Z",
    "?OnCommand@CWnd@@MAAHIJ@Z",
    "?OnNotify@CWnd@@MAAHIJPAJ@Z",
    "?GetSuperWndProcAddr@CWnd@@MAAPAP6AJPAUHWND__@@IIJ@ZXZ",
    "?DoDataExchange@CWnd@@MAAXPAVCDataExchange@@@Z",
    "?BeginModalState@CWnd@@UAAXXZ",
    "?EndModalState@CWnd@@UAAXXZ",
    "?PreTranslateMessage@CWnd@@UAAHPAUtagMSG@@@Z",
    "?OnAmbientProperty@CWnd@@UAAHPAVCOleControlSite@@JPAUtagVARIANT@@@Z",
    "?WindowProc@CWnd@@MAAJIIJ@Z",
    "?OnWndMsg@CWnd@@MAAHIIJPAJ@Z",
    "?DefWindowProcW@CWnd@@MAAJIIJ@Z",
    "?PostNcDestroy@CWnd@@MAAXXZ",
    "?OnChildNotify@CListCtrl@@MAAHIIJPAJ@Z",
    "?CheckAutoCenter@CWnd@@UAAHXZ",
    "?IsFrameWnd@CWnd@@UBAHXZ",
    "?SetOccDialogInfo@CWnd@@MAAHPAU_AFX_OCC_DIALOG_INFO@@@Z",
];
const CEXCEPTION_METHODS: &[&str] = &[
    "?GetRuntimeClass@CException@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?GetErrorMessage@CException@@UAAHPAGIPAI@Z",
    "?ReportError@CException@@UAAHII@Z",
];
const CDOCTEMPLATE_METHODS: [&str; 36] = {
    let mut m = [""; 36];
    let mut i = 0;
    while i < 21 {
        m[i] = CCMDTARGET_METHODS[i];
        i += 1;
    }
    m[0] = "?GetRuntimeClass@CDocTemplate@@UBAPAUCRuntimeClass@@XZ";
    m[3] = "?AssertValid@CObject@@UBAXXZ";
    m[4] = "?Dump@CObject@@UBAXAAVCDumpContext@@@Z";
    m[5] = "?OnCmdMsg@CDocTemplate@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z";
    m[21] = "?LoadTemplate@CDocTemplate@@UAAXXZ";
    m[22] = "PureVirtual";
    m[23] = "PureVirtual";
    m[24] = "?AddDocument@CDocTemplate@@UAAXPAVCDocument@@@Z";
    m[25] = "?RemoveDocument@CDocTemplate@@UAAXPAVCDocument@@@Z";
    m[26] = "?GetDocString@CDocTemplate@@UBAHAAVCString@@W4DocStringIndex@1@@Z";
    m[27] = "?MatchDocType@CDocTemplate@@UAA?AW4Confidence@1@PBGAAPAVCDocument@@@Z";
    m[28] = "?CreateNewDocument@CDocTemplate@@UAAPAVCDocument@@XZ";
    m[29] = "?CreateNewFrame@CDocTemplate@@UAAPAVCFrameWnd@@PAVCDocument@@PAV2@@Z";
    m[30] = "?InitialUpdateFrame@CDocTemplate@@UAAXPAVCFrameWnd@@PAVCDocument@@H@Z";
    m[31] = "?SaveAllModified@CDocTemplate@@UAAHXZ";
    m[32] = "?CloseAllDocuments@CDocTemplate@@UAAXH@Z";
    m[33] = "PureVirtual";
    m[34] = "PureVirtual";
    m[35] = "?OnIdle@CDocTemplate@@UAAXXZ";
    m
};
const CSINGLEDOCTEMPLATE_METHODS: [&str; 36] = {
    let mut m = [""; 36];
    let mut i = 0;
    while i < 36 {
        m[i] = CDOCTEMPLATE_METHODS[i];
        i += 1;
    }
    m[0] = "?GetRuntimeClass@CSingleDocTemplate@@UBAPAUCRuntimeClass@@XZ";
    m[22] = "?GetFirstDocPosition@CSingleDocTemplate@@UBAPAU__POSITION@@XZ";
    m[23] = "?GetNextDoc@CSingleDocTemplate@@UBAPAVCDocument@@AAPAU__POSITION@@@Z";
    m[24] = "?AddDocument@CSingleDocTemplate@@UAAXPAVCDocument@@@Z";
    m[25] = "?RemoveDocument@CSingleDocTemplate@@UAAXPAVCDocument@@@Z";
    m[33] = "?OpenDocumentFile@CSingleDocTemplate@@UAAPAVCDocument@@PBGH@Z";
    m[34] = "?SetDefaultTitle@CSingleDocTemplate@@UAAXPAVCDocument@@@Z";
    m
};
const CCEDOCLISTTEMPLATE_METHODS: &[&str] = &[
    "?GetRuntimeClass@CCeDocListDocTemplate@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?OnCmdMsg@CDocTemplate@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z",
    "?OnFinalRelease@CCmdTarget@@UAAXXZ",
    "?IsInvokeAllowed@CCmdTarget@@UAAHJ@Z",
    "?GetDispatchIID@CCmdTarget@@UAAHPAU_GUID@@@Z",
    "?GetTypeInfoCount@CCmdTarget@@UAAIXZ",
    "?GetTypeLibCache@CCmdTarget@@UAAPAVCTypeLibCache@@XZ",
    "?GetTypeLib@CCmdTarget@@UAAJKPAPAUITypeLib@@@Z",
    "?GetMessageMap@CCmdTarget@@MBAPBUAFX_MSGMAP@@XZ",
    "?GetDispatchMap@CCmdTarget@@MBAPBUAFX_DISPMAP@@XZ",
    "?GetConnectionMap@CCmdTarget@@MBAPBUAFX_CONNECTIONMAP@@XZ",
    "?GetInterfaceMap@CCmdTarget@@MBAPBUAFX_INTERFACEMAP@@XZ",
    "?GetEventSinkMap@CCmdTarget@@MBAPBUAFX_EVENTSINKMAP@@XZ",
    "?OnCreateAggregates@CCmdTarget@@UAAHXZ",
    "?GetInterfaceHook@CCmdTarget@@UAAPAUIUnknown@@PBX@Z",
    "?GetExtraConnectionPoints@CCmdTarget@@MAAHPAVCPtrArray@@@Z",
    "?GetConnectionHook@CCmdTarget@@MAAPAUIConnectionPoint@@ABU_GUID@@@Z",
    "?LoadTemplate@CDocTemplate@@UAAXXZ",
    "?GetFirstDocPosition@CSingleDocTemplate@@UBAPAU__POSITION@@XZ",
    "?GetNextDoc@CSingleDocTemplate@@UBAPAVCDocument@@AAPAU__POSITION@@@Z",
    "?AddDocument@CSingleDocTemplate@@UAAXPAVCDocument@@@Z",
    "?RemoveDocument@CCeDocListDocTemplate@@UAAXPAVCDocument@@@Z",
    "?GetDocString@CDocTemplate@@UBAHAAVCString@@W4DocStringIndex@1@@Z",
    "?MatchDocType@CDocTemplate@@UAA?AW4Confidence@1@PBGAAPAVCDocument@@@Z",
    "?CreateNewDocument@CDocTemplate@@UAAPAVCDocument@@XZ",
    "?CreateNewFrame@CCeDocListDocTemplate@@UAAPAVCFrameWnd@@PAVCDocument@@PAV2@@Z",
    "?InitialUpdateFrame@CDocTemplate@@UAAXPAVCFrameWnd@@PAVCDocument@@H@Z",
    "?SaveAllModified@CDocTemplate@@UAAHXZ",
    "?CloseAllDocuments@CCeDocListDocTemplate@@UAAXH@Z",
    "?OpenDocumentFile@CCeDocListDocTemplate@@UAAPAVCDocument@@PBGH@Z",
    "?SetDefaultTitle@CSingleDocTemplate@@UAAXPAVCDocument@@@Z",
    "?OnIdle@CDocTemplate@@UAAXXZ",
    "?ShowDocList@CCeDocListDocTemplate@@UAAXXZ",
];
const CDOCMANAGER_METHODS: &[&str] = &[
    "?GetRuntimeClass@CDocManager@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
    "?AddDocTemplate@CDocManager@@UAAXPAVCDocTemplate@@@Z",
    "?GetFirstDocTemplatePosition@CDocManager@@UBAPAU__POSITION@@XZ",
    "?GetNextDocTemplate@CDocManager@@UBAPAVCDocTemplate@@AAPAU__POSITION@@@Z",
    "?OpenDocumentFile@CDocManager@@UAAPAVCDocument@@PBG@Z",
    "?SaveAllModified@CDocManager@@UAAHXZ",
    "?CloseAllDocuments@CDocManager@@UAAXH@Z",
    "?GetOpenDocumentCount@CDocManager@@UAAHXZ",
    "?DoPromptFileName@CDocManager@@UAAHAAVCString@@IKHPAVCDocTemplate@@@Z",
    "?OnDDECommand@CDocManager@@UAAHPAG@Z",
    "?OnFileNew@CDocManager@@UAAXXZ",
    "?OnFileOpen@CDocManager@@UAAXXZ",
];
const CARCHIVE_EXCEPTION_METHODS: [&str; 7] = {
    let mut m = [""; 7];
    let mut i = 0;
    while i < 7 {
        m[i] = CEXCEPTION_METHODS[i];
        i += 1;
    }
    m[0] = "?GetRuntimeClass@CArchiveException@@UBAPAUCRuntimeClass@@XZ";
    m[5] = "?GetErrorMessage@CArchiveException@@UAAHPAGIPAI@Z";
    m
};
const CFILE_EXCEPTION_METHODS: [&str; 7] = {
    let mut m = [""; 7];
    let mut i = 0;
    while i < 7 {
        m[i] = CEXCEPTION_METHODS[i];
        i += 1;
    }
    m[0] = "?GetRuntimeClass@CFileException@@UBAPAUCRuntimeClass@@XZ";
    m[5] = "?GetErrorMessage@CFileException@@UAAHPAGIPAI@Z";
    m
};
const CSIMPLE_EXCEPTION_METHODS: [&str; 7] = {
    let mut methods = [""; 7];
    let mut i = 0;
    while i < 7 {
        methods[i] = CEXCEPTION_METHODS[i];
        i += 1;
    }
    methods[5] = "?GetErrorMessage@CSimpleException@@UAAHPAGIPAI@Z";
    methods
};
const CCONTROLBAR_METHODS: [&str; 55] = {
    let mut methods = [""; 55];
    let mut index = 0;
    while index < CWND_METHODS.len() {
        methods[index] = CWND_METHODS[index];
        index += 1;
    }
    methods[0] = "?GetRuntimeClass@CControlBar@@UBAPAUCRuntimeClass@@XZ";
    methods[3] = "?AssertValid@CObject@@UBAXXZ";
    methods[4] = "?Dump@CObject@@UBAXAAVCDumpContext@@@Z";
    methods[12] = "?GetMessageMap@CControlBar@@MBAPBUAFX_MSGMAP@@XZ";
    methods[23] = "?DestroyWindow@CControlBar@@UAAHXZ";
    methods[24] = "?PreCreateWindow@CControlBar@@UAAHAAUtagCREATESTRUCTW@@@Z";
    methods[41] = "?PostNcDestroy@CControlBar@@UAAXXZ";
    methods[46] = "?CalcFixedLayout@CControlBar@@UAA?AVCSize@@HH@Z";
    methods[47] = "?CalcDynamicLayout@CControlBar@@UAA?AVCSize@@HK@Z";
    methods[48] = "PureVirtual";
    methods[49] = "?DelayShow@CControlBar@@UAAXH@Z";
    methods[50] = "?IsVisible@CControlBar@@UBAHXZ";
    methods[51] = "?RecalcDelayShow@CControlBar@@UAAKPAUAFX_SIZEPARENTPARAMS@@@Z";
    methods[52] = "?IsDockBar@CControlBar@@UBAHXZ";
    methods[53] = "?OnBarStyleChange@CControlBar@@UAAXKK@Z";
    methods[54] = "?DoPaint@CControlBar@@UAAXPAVCDC@@@Z";
    methods
};
const CTOOLBAR_METHODS: [&str; 55] = {
    let mut methods = [""; 55];
    let mut index = 0;
    while index < CCONTROLBAR_METHODS.len() {
        methods[index] = CCONTROLBAR_METHODS[index];
        index += 1;
    }
    methods[0] = "?GetRuntimeClass@CToolBar@@UBAPAUCRuntimeClass@@XZ";
    methods[12] = "?GetMessageMap@CToolBar@@MBAPBUAFX_MSGMAP@@XZ";
    methods[46] = "?CalcFixedLayout@CToolBar@@UAA?AVCSize@@HH@Z";
    methods[47] = "?CalcDynamicLayout@CToolBar@@UAA?AVCSize@@HK@Z";
    methods[48] = "?OnUpdateCmdUI@CToolBar@@UAAXPAVCFrameWnd@@H@Z";
    methods[53] = "?OnBarStyleChange@CToolBar@@UAAXKK@Z";
    methods
};
const CCECOMMANDBAR_METHODS: [&str; 60] = {
    let mut methods = [""; 60];
    let mut index = 0;
    while index < CTOOLBAR_METHODS.len() {
        methods[index] = CTOOLBAR_METHODS[index];
        index += 1;
    }
    methods[0] = "?GetRuntimeClass@CCeCommandBar@@UBAPAUCRuntimeClass@@XZ";
    methods[12] = "?GetMessageMap@CCeCommandBar@@MBAPBUAFX_MSGMAP@@XZ";
    methods[46] = "?CalcFixedLayout@CCeCommandBar@@UAA?AVCSize@@HH@Z";
    methods[47] = "?CalcDynamicLayout@CCeCommandBar@@UAA?AVCSize@@HK@Z";
    methods[55] = "?Create@CCeCommandBar@@UAAHPAVCWnd@@KI@Z";
    methods[56] = "?CreateEx@CCeCommandBar@@UAAHPAVCWnd@@KKVCRect@@I@Z";
    methods[57] = "?LoadToolBar@CToolBar@@UAAHI@Z";
    methods[58] = "?GetItemRect@CToolBar@@UBAXHPAUtagRECT@@@Z";
    methods[59] = "?OnSharedNew@CCeCommandBar@@MAAXXZ";
    methods
};
const CCEDOCLISTCB_METHODS: [&str; 60] = {
    let mut methods = CCECOMMANDBAR_METHODS;
    methods[0] = "?GetRuntimeClass@CCeDocListCB@@UBAPAUCRuntimeClass@@XZ";
    methods[5] = "?OnCmdMsg@CCeDocListCB@@UAAHIHPAXPAUAFX_CMDHANDLERINFO@@@Z";
    methods[12] = "?GetMessageMap@CCeDocListCB@@MBAPBUAFX_MSGMAP@@XZ";
    methods[59] = "?OnSharedNew@CCeDocListCB@@MAAXXZ";
    methods
};
const CTABCTRL_METHODS: [&str; 47] = {
    let mut methods = [""; 47];
    let mut index = 0;
    while index < CWND_METHODS.len() {
        methods[index] = CWND_METHODS[index];
        index += 1;
    }
    methods[0] = "?GetRuntimeClass@CTabCtrl@@UBAPAUCRuntimeClass@@XZ";
    methods[12] = "?GetMessageMap@CTabCtrl@@MBAPBUAFX_MSGMAP@@XZ";
    methods[42] = "?OnChildNotify@CTabCtrl@@MAAHIIJPAJ@Z";
    methods[46] = "?DrawItem@CTabCtrl@@UAAXPAUtagDRAWITEMSTRUCT@@@Z";
    methods
};
const CBUTTON_METHODS: [&str; 47] = {
    let mut methods = [""; 47];
    let mut index = 0;
    while index < CWND_METHODS.len() {
        methods[index] = CWND_METHODS[index];
        index += 1;
    }
    methods[0] = "?GetRuntimeClass@CButton@@UBAPAUCRuntimeClass@@XZ";
    methods[12] = "?GetMessageMap@CWnd@@MBAPBUAFX_MSGMAP@@XZ";
    methods[42] = "?OnChildNotify@CButton@@MAAHIIJPAJ@Z";
    methods[46] = "?DrawItem@CButton@@UAAXPAUtagDRAWITEMSTRUCT@@@Z";
    methods
};

const CLISTBOX_METHODS: [&str; 52] = {
    let mut methods = [""; 52];
    let mut index = 0;
    while index < CWND_METHODS.len() {
        methods[index] = CWND_METHODS[index];
        index += 1;
    }
    methods[0] = "?GetRuntimeClass@CListBox@@UBAPAUCRuntimeClass@@XZ";
    methods[42] = "?OnChildNotify@CListBox@@MAAHIIJPAJ@Z";
    methods[46] = "?DrawItem@CListBox@@UAAXPAUtagDRAWITEMSTRUCT@@@Z";
    methods[47] = "?MeasureItem@CListBox@@UAAXPAUtagMEASUREITEMSTRUCT@@@Z";
    methods[48] = "?CompareItem@CListBox@@UAAHPAUtagCOMPAREITEMSTRUCT@@@Z";
    methods[49] = "?DeleteItem@CListBox@@UAAXPAUtagDELETITEMSTRUCT@@@Z";
    methods[50] = "?VKeyToItem@CListBox@@UAAHII@Z";
    methods[51] = "?CharToItem@CListBox@@UAAHII@Z";
    methods
};
const CMENU_METHODS: &[&str] = &[
    "?GetRuntimeClass@CMenu@@UBAPAUCRuntimeClass@@XZ",
    "DeletingDestructor",
    "?Serialize@CObject@@UAAXAAVCArchive@@@Z",
    "?AssertValid@CObject@@UBAXXZ",
    "?Dump@CObject@@UBAXAAVCDumpContext@@@Z",
];
const CLASSES: &[Class] = &[
    Class {
        name: "CCeDocListDocTemplate",
        base: Some("CSingleDocTemplate"),
        runtime_alias: None,
        size: 124,
        schema: 0xffff,
        factory: None,
        methods: CCEDOCLISTTEMPLATE_METHODS,
    },
    Class {
        name: "CCeDocListCB",
        base: Some("CCeCommandBar"),
        runtime_alias: None,
        size: 228,
        schema: 0xffff,
        factory: None,
        methods: &CCEDOCLISTCB_METHODS,
    },
    Class {
        name: "CCeDocList",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 628,
        schema: 0xffff,
        factory: None,
        methods: CCEDOCLIST_METHODS,
    },
    Class {
        name: "CCtrlView",
        base: Some("CView"),
        runtime_alias: None,
        size: 84,
        schema: 0xffff,
        factory: None,
        methods: CCTRLVIEW_METHODS,
    },
    Class {
        name: "CEditView",
        base: Some("CCtrlView"),
        runtime_alias: None,
        size: 96,
        schema: 0xffff,
        factory: Some("??0CEditView@@QAA@XZ"),
        methods: CEDITVIEW_METHODS,
    },
    Class {
        name: "CDocTemplate",
        base: Some("CCmdTarget"),
        runtime_alias: None,
        size: 104,
        schema: 0xffff,
        factory: None,
        methods: &CDOCTEMPLATE_METHODS,
    },
    Class {
        name: "CSingleDocTemplate",
        base: Some("CDocTemplate"),
        runtime_alias: None,
        size: 108,
        schema: 0xffff,
        factory: None,
        methods: &CSINGLEDOCTEMPLATE_METHODS,
    },
    Class {
        name: "CDocManager",
        base: Some("CObject"),
        runtime_alias: None,
        size: 32,
        schema: 0xffff,
        factory: None,
        methods: CDOCMANAGER_METHODS,
    },
    Class {
        name: "CArchiveException",
        base: Some("CException"),
        runtime_alias: None,
        size: 16,
        schema: 0xffff,
        factory: None,
        methods: &CARCHIVE_EXCEPTION_METHODS,
    },
    Class {
        name: "CFileException",
        base: Some("CException"),
        runtime_alias: None,
        size: 20,
        schema: 0xffff,
        factory: None,
        methods: &CFILE_EXCEPTION_METHODS,
    },
    Class {
        name: "CView",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 76,
        schema: 0xffff,
        factory: None,
        methods: &CVIEW_METHODS,
    },
    Class {
        name: "CDocument",
        base: Some("CCmdTarget"),
        runtime_alias: None,
        size: 88,
        schema: 0xffff,
        factory: None,
        methods: &CDOCUMENT_METHODS,
    },
    Class {
        name: "CButton",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: &CBUTTON_METHODS,
    },
    Class {
        name: "CTabCtrl",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: &CTABCTRL_METHODS,
    },
    Class {
        name: "CProgressCtrl",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: CWND_METHODS,
    },
    Class {
        name: "CSliderCtrl",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: CWND_METHODS,
    },
    Class {
        name: "CBrush",
        base: Some("CGdiObject"),
        runtime_alias: None,
        size: 8,
        schema: 0xffff,
        factory: None,
        methods: COBJECT_METHODS,
    },
    Class {
        name: "CGdiObject",
        base: Some("CObject"),
        runtime_alias: None,
        size: 8,
        schema: 0xffff,
        factory: Some("??0CGdiObject@@QAA@XZ"),
        methods: COBJECT_METHODS,
    },
    Class {
        name: "CMenu",
        base: Some("CObject"),
        runtime_alias: None,
        size: 8,
        schema: 0xffff,
        factory: Some("?CreateObject@CMenu@@SAPAVCObject@@XZ"),
        methods: CMENU_METHODS,
    },
    Class {
        name: "CCeCommandBar",
        base: Some("CToolBar"),
        runtime_alias: None,
        size: 224,
        schema: 0xffff,
        factory: None,
        methods: &CCECOMMANDBAR_METHODS,
    },
    Class {
        name: "CToolBar",
        base: Some("CControlBar"),
        runtime_alias: None,
        size: 164,
        schema: 0xffff,
        factory: None,
        methods: &CTOOLBAR_METHODS,
    },
    Class {
        name: "CControlBar",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 124,
        schema: 0xffff,
        factory: None,
        methods: &CCONTROLBAR_METHODS,
    },
    Class {
        name: "CResourceException",
        base: Some("CException"),
        runtime_alias: None,
        size: 280,
        schema: 0xffff,
        factory: None,
        methods: &CSIMPLE_EXCEPTION_METHODS,
    },
    Class {
        name: "CUserException",
        base: Some("CException"),
        runtime_alias: None,
        size: 280,
        schema: 0xffff,
        factory: None,
        methods: &CSIMPLE_EXCEPTION_METHODS,
    },
    Class {
        name: "CMemoryException",
        base: Some("CException"),
        runtime_alias: None,
        size: 280,
        schema: 0xffff,
        factory: None,
        methods: &CSIMPLE_EXCEPTION_METHODS,
    },
    Class {
        name: "CException",
        base: Some("CObject"),
        runtime_alias: None,
        size: 8,
        schema: 0xffff,
        factory: None,
        methods: CEXCEPTION_METHODS,
    },
    Class {
        name: "CStatic",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: CWND_METHODS,
    },
    Class {
        name: "CEdit",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: CWND_METHODS,
    },
    Class {
        name: "CListBox",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: &CLISTBOX_METHODS,
    },
    Class {
        name: "CListCtrl",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: None,
        methods: CLISTCTRL_METHODS,
    },
    Class {
        name: "CDialog",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 124,
        schema: 0xffff,
        factory: None,
        methods: CDIALOG_METHODS,
    },
    Class {
        name: "CEvent",
        base: Some("CSyncObject"),
        runtime_alias: None,
        size: 8,
        schema: 0xffff,
        factory: None,
        methods: CEVENT_METHODS,
    },
    Class {
        name: "CSyncObject",
        base: Some("CObject"),
        runtime_alias: None,
        size: 8,
        schema: 0xffff,
        factory: None,
        methods: CSYNCOBJECT_METHODS,
    },
    Class {
        name: "CSplitterWnd",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 208,
        schema: 0xffff,
        factory: None,
        methods: &CSPLITTER_METHODS,
    },
    Class {
        name: "CFrameWnd",
        base: Some("CWnd"),
        runtime_alias: None,
        size: 232,
        schema: 0xffff,
        factory: Some("??0CFrameWnd@@QAA@XZ"),
        methods: CFRAMEWND_METHODS,
    },
    Class {
        name: "CPtrList",
        base: Some("CObject"),
        runtime_alias: None,
        size: 28,
        schema: 0xffff,
        factory: None,
        methods: CPTRLIST_METHODS,
    },
    Class {
        name: "CStringList",
        base: Some("CObject"),
        runtime_alias: None,
        size: 28,
        schema: 0,
        factory: Some("??0CStringList@@QAA@H@Z"),
        methods: CSTRINGLIST_METHODS,
    },
    Class {
        name: "CObList",
        base: Some("CObject"),
        runtime_alias: None,
        size: 28,
        schema: 0,
        factory: Some("??0CObList@@QAA@H@Z"),
        methods: COBLIST_METHODS,
    },
    Class {
        name: "CWnd",
        base: Some("CCmdTarget"),
        runtime_alias: None,
        size: 72,
        schema: 0xffff,
        factory: Some("??0CWnd@@QAA@XZ"),
        methods: CWND_METHODS,
    },
    Class {
        name: "CDC",
        base: Some("CObject"),
        runtime_alias: None,
        size: 20,
        schema: 0xffff,
        factory: Some("??0CDC@@QAA@XZ"),
        methods: CDC_METHODS,
    },
    Class {
        name: "CClientDC",
        base: Some("CDC"),
        runtime_alias: None,
        size: 24,
        schema: 0xffff,
        factory: None,
        methods: CDC_METHODS,
    },
    Class {
        name: "CPaintDC",
        base: Some("CDC"),
        runtime_alias: None,
        size: 88,
        schema: 0xffff,
        factory: None,
        methods: CDC_METHODS,
    },
    Class {
        name: "CByteArray",
        base: Some("CObject"),
        runtime_alias: None,
        size: 20,
        schema: 0,
        factory: Some("??0CByteArray@@QAA@XZ"),
        methods: CBYTEARRAY_METHODS,
    },
    Class {
        name: "CObArray",
        base: Some("CObject"),
        runtime_alias: None,
        size: 20,
        schema: 0,
        factory: Some("??0CObArray@@QAA@XZ"),
        methods: COBARRAY_METHODS,
    },
    Class {
        name: "CWordArray",
        base: Some("CObject"),
        runtime_alias: None,
        size: 20,
        schema: 0,
        factory: Some("??0CWordArray@@QAA@XZ"),
        methods: CWORDARRAY_METHODS,
    },
    Class {
        name: "CDWordArray",
        base: Some("CObject"),
        runtime_alias: None,
        size: 20,
        schema: 0,
        factory: Some("??0CDWordArray@@QAA@XZ"),
        methods: CDWORDARRAY_METHODS,
    },
    Class {
        name: "CCommandLineInfo",
        base: Some("CObject"),
        runtime_alias: Some("CObject"),
        size: 24,
        schema: 0xffff,
        factory: None,
        methods: CCOMMANDLINEINFO_METHODS,
    },
    Class {
        name: "CMapStringToOb",
        base: Some("CObject"),
        runtime_alias: None,
        size: 28,
        schema: 0,
        factory: Some("??0CMapStringToOb@@QAA@H@Z"),
        methods: CMAPSTRINGTOOB_METHODS,
    },
    Class {
        name: "CMapWordToOb",
        base: Some("CObject"),
        runtime_alias: None,
        size: 28,
        schema: 0,
        factory: Some("??0CMapWordToOb@@QAA@H@Z"),
        methods: CMAPWORDTOOB_METHODS,
    },
    Class {
        name: "CMapStringToString",
        base: Some("CObject"),
        runtime_alias: None,
        size: 28,
        schema: 0,
        factory: Some("??0CMapStringToString@@QAA@H@Z"),
        methods: CMAPSTRINGTOSTRING_METHODS,
    },
    Class {
        name: "CStringArray",
        base: Some("CObject"),
        runtime_alias: None,
        size: 20,
        schema: 0,
        factory: Some("??0CStringArray@@QAA@XZ"),
        methods: CSTRINGARRAY_METHODS,
    },
    Class {
        name: "CObject",
        base: None,
        runtime_alias: None,
        size: 4,
        schema: 0xffff,
        factory: None,
        methods: COBJECT_METHODS,
    },
    Class {
        name: "CWinApp",
        base: Some("CWinThread"),
        runtime_alias: None,
        size: 180,
        schema: 0xffff,
        factory: None,
        methods: CWINAPP_METHODS,
    },
    Class {
        name: "CWinThread",
        base: Some("CCmdTarget"),
        runtime_alias: None,
        size: 104,
        schema: 0xffff,
        factory: None,
        methods: CWINTHREAD_METHODS,
    },
    Class {
        name: "CCmdTarget",
        base: Some("CObject"),
        runtime_alias: None,
        size: 32,
        schema: 0xffff,
        factory: None,
        methods: CCMDTARGET_METHODS,
    },
];
fn class(name: &str) -> Result<&'static Class, KernelError> {
    CLASSES
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| KernelError::Dispatch(format!("Unsupported MFC class {name}")))
}
pub(super) fn object_size(name: &str) -> Result<u32, KernelError> {
    Ok(class(name)?.size)
}
fn symbol(name: &str, member: &str) -> String {
    format!("{PREFIX}{name}:{member}")
}
pub(super) fn register(d: &mut WinCeDispatcher) {
    for c in CLASSES {
        if c.runtime_alias.is_none() {
            d.register_handler("coredll.dll", &symbol(c.name, "base"), base);
        }
        if c.factory.is_some() {
            d.register_handler("coredll.dll", &symbol(c.name, "create"), create);
        }
        for i in 0..c.methods.len() {
            d.register_handler(
                "coredll.dll",
                &symbol(c.name, &i.to_string()),
                virtual_method,
            );
        }
    }
}
fn identity<'a>(ctx: &CallCtx<'a>) -> Result<(&'a str, &'a str), KernelError> {
    let pocket_pe::ImportBinding::Name(n) = &ctx.thunk.binding else {
        return Err(KernelError::Loader("MFC class symbol missing".into()));
    };
    n.strip_prefix(PREFIX)
        .and_then(|s| s.split_once(':'))
        .ok_or_else(|| KernelError::Loader("Invalid MFC class symbol".into()))
}
fn owner(ctx: &CallCtx<'_>) -> Result<u32, KernelError> {
    super::application::owner(ctx)
}
fn address(ctx: &CallCtx<'_>, name: &str, member: &str) -> Result<u32, KernelError> {
    ctx.kernel.dynamic_exports[&owner(ctx)?]
        .get(&symbol(name, member))
        .copied()
        .ok_or_else(|| KernelError::Loader(format!("MFC class export {name}:{member} missing")))
}
fn data(ctx: &mut CallCtx<'_>, name: &str) -> Result<u32, KernelError> {
    let c = class(name)?;
    let key = symbol(name, "data");
    let h = owner(ctx)?;
    if let Some(&p) = ctx.kernel.dynamic_exports[&h].get(&key) {
        return Ok(p);
    }
    if let Some(parent) = c.base {
        let _ = data(ctx, parent)?;
    }
    let size = (c.methods.len() * 4
        + if c.runtime_alias.is_none() {
            24 + c.name.len() + 1
        } else {
            0
        }) as u32;
    let p = alloc(ctx, size)?;
    let result = (|| {
        ctx.cpu.write_mem(p, &vec![0; size as usize])?;
        for i in 0..c.methods.len() {
            let f = address(ctx, name, &i.to_string())?;
            put(ctx, p + i as u32 * 4, f)?;
        }
        if c.runtime_alias.is_some() {
            return Ok::<_, KernelError>(());
        }
        let descriptor = p + c.methods.len() as u32 * 4;
        let text = descriptor + 24;
        ctx.cpu
            .write_mem(text, format!("{}\0", c.name).as_bytes())?;
        let factory = if c.factory.is_some() {
            address(ctx, name, "create")?
        } else {
            0
        };
        let parent = address(ctx, name, "base")?;
        for (i, v) in [text, c.size, c.schema, factory, parent, 0]
            .into_iter()
            .enumerate()
        {
            put(ctx, descriptor + i as u32 * 4, v)?;
        }
        Ok::<_, KernelError>(())
    })();
    if let Err(e) = result {
        ctx.kernel.heap.free(p);
        return Err(e);
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&h)
        .unwrap()
        .insert(key, p);
    Ok(p)
}
pub(super) fn vtable(ctx: &mut CallCtx<'_>, name: &str) -> Result<u32, KernelError> {
    data(ctx, name)
}
pub(super) fn runtime(ctx: &mut CallCtx<'_>, name: &str) -> Result<u32, KernelError> {
    let c = class(name)?;
    if let Some(alias) = c.runtime_alias {
        return runtime(ctx, alias);
    }
    let n = c.methods.len() as u32;
    Ok(data(ctx, name)? + n * 4)
}
fn base(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let name = identity(ctx)?.0.to_string();
    let p = match class(&name)?.base {
        Some(n) => runtime(ctx, n)?,
        None => 0,
    };
    Ok(DispatchOutcome::ReturnedR0(p))
}
fn create(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let name = identity(ctx)?.0.to_string();
    create_named(ctx, &name)
}
fn create_named(ctx: &mut CallCtx<'_>, name: &str) -> Result<DispatchOutcome, KernelError> {
    let c = class(name)?;
    let constructor = c
        .factory
        .ok_or_else(|| KernelError::Dispatch(format!("Unsupported MFC factory {name}")))?;
    let handler = super::handler_named(constructor)
        .ok_or_else(|| KernelError::Loader("MFC constructor missing".into()))?;
    // Static CreateObject factories allocate/initialize their own instance.
    // Constructor-backed metadata uses the common allocate-then-construct path.
    if !constructor.starts_with("??0") {
        return handler(ctx);
    }
    let object = alloc_cpp(ctx, c.size)?;
    ctx.cpu.write_arg(0, object)?;
    ctx.cpu.write_arg(1, 10)?;
    match handler(ctx) {
        Ok(result) => Ok(result),
        Err(e) => {
            ctx.kernel.heap.free(object);
            Err(e)
        }
    }
}
fn virtual_method(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let (name, slot) = identity(ctx)?;
    let name = name.to_string();
    let slot: usize = slot
        .parse()
        .map_err(|_| KernelError::Loader("Invalid MFC method slot".into()))?;
    let c = class(&name)?;
    if slot == 0 {
        return Ok(DispatchOutcome::ReturnedR0(runtime(ctx, &name)?));
    }
    let method = c
        .methods
        .get(slot)
        .ok_or_else(|| KernelError::Loader("Invalid MFC vtable slot".into()))?;
    if slot == 1 && matches!(name.as_str(), "CArchiveException" | "CFileException") {
        return super::io_exceptions::deleting(ctx, name == "CFileException");
    }
    if slot == 1 && name == "CCeDocListDocTemplate" {
        return super::doc_list_template::delete(ctx, true);
    }
    if slot == 1 && matches!(name.as_str(), "CDocTemplate" | "CSingleDocTemplate") {
        return super::document_templates::delete(ctx, true);
    }
    if slot == 1 && name == "CDocManager" {
        return super::document_manager::delete(ctx, true);
    }
    if slot == 1 && name == "CCeDocList" {
        return super::doc_list::delete(ctx, true);
    }
    if slot == 1 && name == "CEditView" {
        return super::edit_view::delete(ctx, true);
    }
    if slot == 1 && name == "CView" {
        return super::views::delete(ctx, true);
    }
    if slot == 1 && name == "CDocument" {
        return super::documents::delete(ctx, true);
    }
    if slot == 1 && name == "CCeDocListCB" {
        return super::control_bars::deleting(ctx, "CCeCommandBar");
    }
    if slot == 1 && matches!(name.as_str(), "CControlBar" | "CToolBar" | "CCeCommandBar") {
        return super::control_bars::deleting(ctx, &name);
    }
    if slot == 1
        && matches!(
            name.as_str(),
            "CProgressCtrl"
                | "CSliderCtrl"
                | "CTabCtrl"
                | "CDialog"
                | "CButton"
                | "CStatic"
                | "CEdit"
                | "CListCtrl"
                | "CListBox"
        )
    {
        return super::common_control_lifetime::destruct(ctx, &name, true);
    }
    if slot == 1
        && matches!(
            name.as_str(),
            "CException" | "CUserException" | "CMemoryException" | "CResourceException"
        )
    {
        return super::exception_types::deleting(ctx);
    }
    if slot == 1 && name == "CFrameWnd" {
        return super::frames::delete(ctx, true);
    }
    if slot == 1 && name == "CWnd" {
        return super::window_lifetime::deleting(ctx);
    }
    if slot == 1 && name == "CSplitterWnd" {
        return super::splitter_windows::deleting(ctx);
    }
    if slot == 1 && name == "CMenu" {
        return super::menus::deleting(ctx);
    }
    if slot == 1 && name == "CDC" {
        return super::device_contexts::deleting(ctx);
    }
    if slot == 1 && name == "CWinThread" {
        return super::worker_threads::deleting(ctx);
    }
    if slot == 1 && matches!(name.as_str(), "CGdiObject" | "CBrush") {
        return super::gdi::deleting(ctx);
    }
    if slot == 1 && matches!(name.as_str(), "CClientDC" | "CPaintDC") {
        return super::window_dcs::deleting(ctx, name == "CPaintDC");
    }
    if slot == 1
        && matches!(
            name.as_str(),
            "CObject"
                | "CByteArray"
                | "CStringArray"
                | "CMapStringToOb"
                | "CMapStringToString"
                | "CMapWordToOb"
                | "CCommandLineInfo"
                | "CWordArray"
                | "CDWordArray"
                | "CPtrList"
                | "CObList"
                | "CStringList"
                | "CSyncObject"
                | "CEvent"
                | "CObArray"
        )
    {
        let object = ctx.arg_u32(0)?;
        let flags = ctx.arg_u32(1)?;
        if flags & 2 != 0 {
            return Err(KernelError::Dispatch(
                "MFC vector destruction is not implemented".into(),
            ));
        }
        let destructor = match name.as_str() {
            "CByteArray" => Some("??1CByteArray@@UAA@XZ"),
            "CObArray" => Some("??1CObArray@@UAA@XZ"),
            "CObList" => Some("??1CObList@@UAA@XZ"),
            "CStringList" => Some("??1CStringList@@UAA@XZ"),
            "CPtrList" => Some("??1CPtrList@@UAA@XZ"),
            "CSyncObject" | "CEvent" => Some("??1CSyncObject@@UAA@XZ"),
            "CStringArray" => Some("??1CStringArray@@UAA@XZ"),
            "CDWordArray" => Some("??1CDWordArray@@UAA@XZ"),
            "CWordArray" => Some("??1CWordArray@@UAA@XZ"),
            "CCommandLineInfo" => Some("??1CCommandLineInfo@@UAA@XZ"),
            "CMapStringToString" => Some("??1CMapStringToString@@UAA@XZ"),
            "CMapWordToOb" => Some("??1CMapWordToOb@@UAA@XZ"),
            "CMapStringToOb" => Some("??1CMapStringToOb@@UAA@XZ"),
            _ => None,
        };
        if let Some(n) = destructor {
            super::handler_named(n)
                .ok_or_else(|| KernelError::Loader("MFC destructor missing".into()))?(
                ctx
            )?;
        }
        if flags & 1 != 0 {
            ctx.kernel.heap.free(object);
        }
        return Ok(DispatchOutcome::ReturnedR0(object));
    }
    if let Some(handler) = super::handler_named(method) {
        return handler(ctx);
    }
    if (3..=4).contains(&slot) || (slot == 2 && method.starts_with("?Serialize@CObject")) {
        // Retail CObject's Serialize/AssertValid/Dump have no release-side
        // payload; validate the real object before their native void return.
        let object = ctx.arg_u32(0)?;
        ctx.cpu
            .check_guest_access(object, c.size, pocket_cpu::Prot::READ)?;
        return Ok(DispatchOutcome::ReturnedR0(0));
    }
    Err(KernelError::Dispatch(format!(
        "MFC {name}::{method} is not implemented"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn serializable_class_metadata_factories_and_deleting_destructors_agree() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        let run = |cpu: &mut pocket_cpu::stub::StubCpu,
                   k: &mut pocket_kernel::KernelState,
                   d: &mut WinCeDispatcher,
                   n: &str,
                   a: &[u32]| {
            super::super::tests::result(call(cpu, k, d, "coredll.dll", n, a))
        };
        for (name, size) in [
            ("CByteArray", 20),
            ("CStringArray", 20),
            ("CObArray", 20),
            ("CMapStringToOb", 28),
            ("CWordArray", 20),
        ] {
            let descriptor = run(&mut cpu, &mut k, &mut d, &symbol(name, "0"), &[]);
            assert_eq!(cpu.read_u32_le(descriptor + 4).unwrap(), size);
            assert_eq!(cpu.read_u32_le(descriptor + 8).unwrap(), 0);
            assert_ne!(cpu.read_u32_le(descriptor + 12).unwrap(), 0);
            assert_ne!(cpu.read_u32_le(descriptor + 16).unwrap(), 0);
            let baseline = k.heap.free_bytes();
            let object = run(&mut cpu, &mut k, &mut d, &symbol(name, "create"), &[]);
            assert_ne!(object, 0);
            assert_eq!(
                run(&mut cpu, &mut k, &mut d, &symbol(name, "0"), &[object]),
                descriptor
            );
            run(&mut cpu, &mut k, &mut d, &symbol(name, "1"), &[object, 1]);
            assert_eq!(k.heap.free_bytes(), baseline);
        }
        let root = run(&mut cpu, &mut k, &mut d, &symbol("CObject", "0"), &[]);
        assert_eq!(
            run(&mut cpu, &mut k, &mut d, &symbol("CObject", "base"), &[]),
            0
        );
        let imported = super::super::tests::result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1501",
            &[],
        ));
        assert_eq!(imported, root);
        assert_eq!(
            run(&mut cpu, &mut k, &mut d, "__PocketHLE_CFile_BaseClass", &[]),
            root
        );
    }
}

#[cfg(test)]
mod allocation_origin_tests {
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    #[test]
    fn runtime_factory_objects_keep_operator_new_ownership_until_their_deleting_destructor() {
        let (mut cpu, mut k, mut d) = super::super::tests::framework_setup();
        let descriptor = super::super::tests::result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_Class:CObArray:0",
            &[],
        ));
        let object = super::super::tests::result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_Class:CObArray:create",
            &[],
        ));
        assert_eq!(cpu.read_u32_le(descriptor + 4).unwrap(), 20);
        assert_eq!(k.heap.msize(object), Some(20));
        k.heap.free_crt(object);
        assert_eq!(k.heap.msize(object), Some(20));
        call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_Class:CObArray:1",
            &[object, 1],
        );
        assert_eq!(k.heap.msize(object), None);
    }
}

/// Resolve serialized class names against real guest registrations first, then
/// declarative HLE classes. No class is selected by the executable's identity.
pub(super) fn find_runtime(ctx: &mut CallCtx<'_>, name: &[u8]) -> Result<Option<u32>, KernelError> {
    let state = super::application::state(ctx)?;
    let mut at = word(ctx, state + 0x20)?;
    let mut seen = std::collections::HashSet::new();
    while at != 0 {
        if seen.len() >= 4096 || !seen.insert(at) {
            return Err(KernelError::Dispatch(
                "Invalid MFC runtime-class registry".into(),
            ));
        }
        ctx.cpu.check_guest_access(at, 24, pocket_cpu::Prot::READ)?;
        let text = word(ctx, at)?;
        let mut matches = true;
        for (i, b) in name.iter().copied().chain(Some(0)).enumerate() {
            if ctx.cpu.read_mem(text + i as u32, 1)?[0] != b {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(Some(at));
        }
        at = word(ctx, at + 20)?;
    }
    match CLASSES.iter().find(|c| c.name.as_bytes() == name) {
        Some(c) => Ok(Some(runtime(ctx, c.name)?)),
        None => Ok(None),
    }
}

fn exported_metadata(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let name = match &ctx.thunk.binding {
        pocket_pe::ImportBinding::Name(name) => name.as_str(),
        pocket_pe::ImportBinding::Ordinal(ordinal) => super::ordinal_name(*ordinal)
            .ok_or_else(|| KernelError::Dispatch("Unknown MFC metadata ordinal".into()))?,
    };
    let (operation, rest) = name
        .split_once('@')
        .ok_or_else(|| KernelError::Dispatch("Invalid MFC metadata symbol".into()))?;
    let class_name = rest.split("@@").next().unwrap().to_owned();
    match operation {
        "?CreateObject" => create_named(ctx, &class_name),
        "?GetRuntimeClass" => Ok(DispatchOutcome::ReturnedR0(runtime(ctx, &class_name)?)),
        "?_GetBaseClass" => Ok(DispatchOutcome::ReturnedR0(
            match class(&class_name)?.base {
                Some(base) => runtime(ctx, base)?,
                None => 0,
            },
        )),
        _ => Err(KernelError::Dispatch(
            "Invalid MFC metadata operation".into(),
        )),
    }
}

#[cfg(test)]
mod byte_array_ownership_tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::Cpu;
    use pocket_kernel::Dispatcher;
    use pocket_pe::{ImportBinding, ImportSymbol};

    fn invoke(
        cpu: &mut pocket_cpu::stub::StubCpu,
        k: &mut pocket_kernel::KernelState,
        d: &mut crate::WinCeDispatcher,
        name: &str,
        args: &[u32],
    ) -> u32 {
        let dll = if name.starts_with("ord:") {
            "mfcce300.dll"
        } else {
            "coredll.dll"
        };
        let name = name.strip_prefix("ord:").map_or(name, |ordinal| {
            super::super::ordinal_name(ordinal.parse().unwrap()).unwrap()
        });
        result(call(cpu, k, d, dll, name, args))
    }
    #[test]
    fn byte_metadata_and_native_data_imports_share_one_owner_across_object_lifetimes_and_restore() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let descriptor = invoke(&mut cpu, &mut k, &mut d, "ord:1444", &[]);
        let parent = invoke(&mut cpu, &mut k, &mut d, "ord:411", &[]);
        assert_eq!(cpu.read_u32_le(descriptor + 4).unwrap(), 20);
        assert_eq!(cpu.read_u32_le(descriptor + 8).unwrap(), 0);
        assert_ne!(cpu.read_u32_le(descriptor + 12).unwrap(), 0);
        assert_eq!(parent, invoke(&mut cpu, &mut k, &mut d, "ord:1501", &[]));
        for (address, binding) in [
            (0x1000, ImportBinding::Ordinal(26)),
            (
                0x1004,
                ImportBinding::Name("?classCByteArray@CByteArray@@2UCRuntimeClass@@A".into()),
            ),
        ] {
            k.image_imports.push(ImportSymbol {
                dll: "mfcce300.dll".into(),
                binding,
                iat_va: address,
            });
        }
        super::super::bind_data_imports(&mut cpu, &mut k).unwrap();
        assert_eq!(cpu.read_u32_le(0x1000).unwrap(), descriptor);
        assert_eq!(cpu.read_u32_le(0x1004).unwrap(), descriptor);
        let baseline = k.heap.free_bytes();
        let mut vtable = None;
        for _ in 0..128 {
            for object in [0x2000u32, 0x2200] {
                invoke(&mut cpu, &mut k, &mut d, "ord:47", &[object]);
                let table = cpu.read_u32_le(object).unwrap();
                assert_eq!(*vtable.get_or_insert(table), table);
                assert_eq!(table + 5 * 4, descriptor);
                invoke(&mut cpu, &mut k, &mut d, "ord:218", &[object]);
                assert!(k.heap.msize(table).is_some());
            }
            assert_eq!(k.heap.free_bytes(), baseline);
        }
        let saved = k.capture_snapshot().unwrap();
        let (_, mut restored, _) = framework_setup();
        saved.restore(&mut restored).unwrap();
        k = restored;
        invoke(&mut cpu, &mut k, &mut d, "ord:47", &[0x2000]);
        assert_eq!(cpu.read_u32_le(0x2000).unwrap(), vtable.unwrap());
        invoke(&mut cpu, &mut k, &mut d, "ord:218", &[0x2000]);
        assert_eq!(k.heap.free_bytes(), baseline);
    }
    #[test]
    fn base_destructor_never_releases_a_derived_vtable_and_scalar_delete_frees_only_the_instance() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let descriptor = invoke(&mut cpu, &mut k, &mut d, "ord:1444", &[]);
        let table = descriptor - 5 * 4;
        let guest_table = invoke(&mut cpu, &mut k, &mut d, "ord:315", &[5 * 4]);
        let bytes = cpu.read_mem(table, 5 * 4).unwrap();
        cpu.write_mem(guest_table, &bytes).unwrap();
        let baseline = k.heap.free_bytes();
        for _ in 0..128 {
            let object = invoke(&mut cpu, &mut k, &mut d, "ord:961", &[]);
            invoke(
                &mut cpu,
                &mut k,
                &mut d,
                "ord:2413",
                &[object, 512, u32::MAX],
            );
            let data = cpu.read_u32_le(object + 4).unwrap();
            cpu.write_mem(data, &[0x5a; 512]).unwrap();
            cpu.write_mem(object, &guest_table.to_le_bytes()).unwrap();
            invoke(&mut cpu, &mut k, &mut d, "ord:218", &[object]);
            assert_eq!(k.heap.msize(object), Some(20));
            assert!(k.heap.msize(data).is_none());
            assert_eq!(k.heap.msize(guest_table), Some(20));
            assert_eq!(cpu.read_mem(guest_table, 20).unwrap(), bytes);
            invoke(&mut cpu, &mut k, &mut d, "ord:317", &[object]);
            assert_eq!(k.heap.free_bytes(), baseline);
            let object = invoke(&mut cpu, &mut k, &mut d, "ord:961", &[]);
            invoke(
                &mut cpu,
                &mut k,
                &mut d,
                "__PocketHLE_MFC_Class:CByteArray:1",
                &[object, 1],
            );
            assert!(k.heap.msize(object).is_none());
            assert!(k.heap.msize(table).is_some());
            assert_eq!(k.heap.free_bytes(), baseline);
        }
        invoke(&mut cpu, &mut k, &mut d, "ord:317", &[guest_table]);
    }
    #[test]
    fn vector_delete_is_rejected_before_mutating_array_storage_or_shared_metadata() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let object = invoke(&mut cpu, &mut k, &mut d, "ord:961", &[]);
        invoke(
            &mut cpu,
            &mut k,
            &mut d,
            "ord:2413",
            &[object, 16, u32::MAX],
        );
        let before = cpu.read_mem(object, 20).unwrap();
        cpu.write_arg(0, object).unwrap();
        cpu.write_arg(1, 3).unwrap();
        let va = k.dynamic_exports[&1]["__PocketHLE_MFC_Class:CByteArray:1"];
        let thunk = pocket_kernel::Thunk {
            thunk_va: va,
            iat_va: 0,
            dll: "coredll.dll".into(),
            binding: ImportBinding::Name("__PocketHLE_MFC_Class:CByteArray:1".into()),
            friendly_name: None,
        };
        assert!(d
            .dispatch(&mut cpu, &thunk, &mut k)
            .unwrap_err()
            .to_string()
            .contains("vector destruction"));
        assert_eq!(cpu.read_mem(object, 20).unwrap(), before);
        invoke(
            &mut cpu,
            &mut k,
            &mut d,
            "__PocketHLE_MFC_Class:CByteArray:1",
            &[object, 1],
        );
    }
}
