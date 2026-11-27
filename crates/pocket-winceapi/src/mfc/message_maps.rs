//! Native CE message-map data, distinct from executable import thunks.
use super::*;
struct Entry {
    fields: [u32; 5],
    method: &'static str,
}
struct Map {
    class: &'static str,
    base: Option<&'static str>,
    entries: &'static [Entry],
}
const CCMDTARGET: &[Entry] = &[];
const CWINAPP: &[Entry] = &[
    Entry {
        fields: [273, 0, 57665, 57665, 12],
        method: "?OnAppExit@CWinApp@@IAAXXZ",
    },
    Entry {
        fields: [273, 4294967295, 57616, 57616, 44],
        method: "?OnUpdateRecentFileMenu@CWinApp@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 0, 57616, 57631, 2],
        method: "?OnOpenRecentFile@CWinApp@@IAAHI@Z",
    },
];
const CWND: &[Entry] = &[
    Entry {
        fields: [312, 0, 0, 0, 10],
        method: "?OnNTCtlColor@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [307, 0, 0, 0, 10],
        method: "?OnNTCtlColor@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [309, 0, 0, 0, 10],
        method: "?OnNTCtlColor@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [308, 0, 0, 0, 10],
        method: "?OnNTCtlColor@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [310, 0, 0, 0, 10],
        method: "?OnNTCtlColor@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [306, 0, 0, 0, 10],
        method: "?OnNTCtlColor@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [311, 0, 0, 0, 10],
        method: "?OnNTCtlColor@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [43, 0, 0, 0, 32],
        method: "?OnDrawItem@CWnd@@IAAXHPAUtagDRAWITEMSTRUCT@@@Z",
    },
    Entry {
        fields: [44, 0, 0, 0, 32],
        method: "?OnMeasureItem@CWnd@@IAAXHPAUtagMEASUREITEMSTRUCT@@@Z",
    },
    Entry {
        fields: [25, 0, 0, 0, 4],
        method: "?OnCtlColor@CWnd@@IAAPAUHBRUSH__@@PAVCDC@@PAV1@I@Z",
    },
    Entry {
        fields: [57, 0, 0, 0, 33],
        method: "?OnCompareItem@CWnd@@IAAHHPAUtagCOMPAREITEMSTRUCT@@@Z",
    },
    Entry {
        fields: [276, 0, 0, 0, 29],
        method: "?OnHScroll@CWnd@@IAAXIIPAVCScrollBar@@@Z",
    },
    Entry {
        fields: [277, 0, 0, 0, 29],
        method: "?OnVScroll@CWnd@@IAAXIIPAVCScrollBar@@@Z",
    },
    Entry {
        fields: [45, 0, 0, 0, 32],
        method: "?OnDeleteItem@CWnd@@IAAXHPAUtagDELETITEMSTRUCT@@@Z",
    },
    Entry {
        fields: [47, 0, 0, 0, 6],
        method: "?OnCharToItem@CWnd@@IAAHIPAVCListBox@@I@Z",
    },
    Entry {
        fields: [46, 0, 0, 0, 6],
        method: "?OnVKeyToItem@CWnd@@IAAHIPAVCListBox@@I@Z",
    },
    Entry {
        fields: [32767, 0, 0, 0, 12],
        method: "?OnNcDestroy@CWnd@@IAAXXZ",
    },
    Entry {
        fields: [21, 0, 0, 0, 12],
        method: "?OnSysColorChange@CWnd@@IAAXXZ",
    },
    Entry {
        fields: [83, 0, 0, 0, 42],
        method: "?OnHelpInfo@CWnd@@IAAHPAUtagHELPINFO@@@Z",
    },
    Entry {
        fields: [26, 0, 0, 0, 47],
        method: "?OnSettingChange@CWnd@@IAAXIPBG@Z",
    },
    Entry {
        fields: [2, 0, 0, 0, 12],
        method: "?OnDestroy@CWnd@@IAAXXZ",
    },
    Entry {
        fields: [878, 0, 0, 0, 10],
        method: "?OnActivateTopLevel@CWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [513, 0, 0, 0, 49],
        method: "?OnLButtonDown@CWnd@@IAAXIVCPoint@@@Z",
    },
];
const CFRAMEWND: &[Entry] = &[
    Entry {
        fields: [279, 0, 0, 0, 22],
        method: "?OnInitMenuPopup@CFrameWnd@@IAAXPAVCMenu@@IH@Z",
    },
    Entry {
        fields: [885, 0, 0, 0, 10],
        method: "?OnPopMessageString@CFrameWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [866, 0, 0, 0, 10],
        method: "?OnSetMessageString@CFrameWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [886, 0, 0, 0, 10],
        method: "?OnHelpPromptAddr@CFrameWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [867, 0, 0, 0, 12],
        method: "?OnIdleUpdateCmdUI@CFrameWnd@@IAAXXZ",
    },
    Entry {
        fields: [276, 0, 0, 0, 29],
        method: "?OnHScroll@CFrameWnd@@IAAXIIPAVCScrollBar@@@Z",
    },
    Entry {
        fields: [277, 0, 0, 0, 29],
        method: "?OnVScroll@CFrameWnd@@IAAXIIPAVCScrollBar@@@Z",
    },
    Entry {
        fields: [7, 0, 0, 0, 23],
        method: "?OnSetFocus@CFrameWnd@@IAAXPAVCWnd@@@Z",
    },
    Entry {
        fields: [1, 0, 0, 0, 9],
        method: "?OnCreate@CFrameWnd@@IAAHPAUtagCREATESTRUCTW@@@Z",
    },
    Entry {
        fields: [2, 0, 0, 0, 12],
        method: "?OnDestroy@CFrameWnd@@IAAXXZ",
    },
    Entry {
        fields: [16, 0, 0, 0, 12],
        method: "?OnClose@CFrameWnd@@IAAXXZ",
    },
    Entry {
        fields: [5, 0, 0, 0, 17],
        method: "?OnSize@CFrameWnd@@IAAXIHH@Z",
    },
    Entry {
        fields: [26, 0, 0, 0, 47],
        method: "?OnSettingChange@CFrameWnd@@IAAXIPBG@Z",
    },
    Entry {
        fields: [20, 0, 0, 0, 1],
        method: "?OnEraseBkgnd@CFrameWnd@@IAAHPAVCDC@@@Z",
    },
    Entry {
        fields: [6, 0, 0, 0, 28],
        method: "?OnActivate@CFrameWnd@@IAAXIPAVCWnd@@H@Z",
    },
    Entry {
        fields: [274, 0, 0, 0, 18],
        method: "?OnSysCommand@CFrameWnd@@IAAXIJ@Z",
    },
    Entry {
        fields: [32, 0, 0, 0, 3],
        method: "?OnSetCursor@CFrameWnd@@IAAHPAVCWnd@@II@Z",
    },
    Entry {
        fields: [10, 0, 0, 0, 13],
        method: "?OnEnable@CFrameWnd@@IAAXH@Z",
    },
    Entry {
        fields: [783, 0, 0, 0, 35],
        method: "?OnQueryNewPalette@CFrameWnd@@IAAHXZ",
    },
    Entry {
        fields: [785, 0, 0, 0, 23],
        method: "?OnPaletteChanged@CFrameWnd@@IAAXPAVCWnd@@@Z",
    },
    Entry {
        fields: [869, 0, 0, 0, 10],
        method: "?OnCommandHelp@CFrameWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [870, 0, 0, 0, 10],
        method: "?OnHelpHitTest@CFrameWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [878, 0, 0, 0, 10],
        method: "?OnActivateTopLevel@CFrameWnd@@IAAJIJ@Z",
    },
    Entry {
        fields: [273, 4294967295, 59393, 59393, 44],
        method: "?OnUpdateControlBarMenu@CFrameWnd@@QAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 0, 59393, 59393, 2],
        method: "?OnBarCheck@CFrameWnd@@QAAHI@Z",
    },
    Entry {
        fields: [273, 4294967295, 59392, 59392, 44],
        method: "?OnUpdateControlBarMenu@CFrameWnd@@QAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 0, 59392, 59392, 2],
        method: "?OnBarCheck@CFrameWnd@@QAAHI@Z",
    },
    Entry {
        fields: [273, 4294967295, 59396, 59396, 44],
        method: "?OnUpdateControlBarMenu@CFrameWnd@@QAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 0, 59396, 59396, 2],
        method: "?OnBarCheck@CFrameWnd@@QAAHI@Z",
    },
    Entry {
        fields: [273, 4294967295, 59137, 59137, 44],
        method: "?OnUpdateKeyIndicator@CFrameWnd@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 59138, 59138, 44],
        method: "?OnUpdateKeyIndicator@CFrameWnd@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 59139, 59139, 44],
        method: "?OnUpdateKeyIndicator@CFrameWnd@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 59142, 59142, 44],
        method: "?OnUpdateKeyIndicator@CFrameWnd@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 0, 1, 1, 12],
        method: "?OnOK@CFrameWnd@@IAAXXZ",
    },
    Entry {
        fields: [78, 11012, 58378, 58378, 38],
        method: "?OnFolderChanged@CFrameWnd@@IAAXPAUtagDLNHDR@@PAJ@Z",
    },
    Entry {
        fields: [78, 11032, 58378, 58378, 38],
        method: "?OnCreateDocList@CFrameWnd@@IAAXPAUtagDLNHDR@@PAJ@Z",
    },
    Entry {
        fields: [78, 11033, 58378, 58378, 38],
        method: "?OnDestroyDocList@CFrameWnd@@IAAXPAUtagDLNHDR@@PAJ@Z",
    },
];
const CDIALOG: &[Entry] = &[
    Entry {
        fields: [25, 0, 0, 0, 4],
        method: "?OnCtlColor@CDialog@@IAAPAUHBRUSH__@@PAVCDC@@PAVCWnd@@I@Z",
    },
    Entry {
        fields: [273, 0, 1, 1, 12],
        method: "??_9@$BME@AA",
    },
    Entry {
        fields: [273, 0, 2, 2, 12],
        method: "??_9@$BMI@AA",
    },
    Entry {
        fields: [869, 0, 0, 0, 10],
        method: "?OnCommandHelp@CDialog@@IAAJIJ@Z",
    },
    Entry {
        fields: [870, 0, 0, 0, 10],
        method: "?OnHelpHitTest@CDialog@@IAAJIJ@Z",
    },
    Entry {
        fields: [272, 0, 0, 0, 10],
        method: "?HandleInitDialog@CDialog@@IAAJIJ@Z",
    },
    Entry {
        fields: [48, 0, 0, 0, 10],
        method: "?HandleSetFont@CDialog@@IAAJIJ@Z",
    },
    Entry {
        fields: [26, 0, 0, 0, 47],
        method: "?OnSettingChange@CDialog@@IAAXIPBG@Z",
    },
    Entry {
        fields: [6, 0, 0, 0, 28],
        method: "?OnActivate@CDialog@@IAAXIPAVCWnd@@H@Z",
    },
];
const CLISTCTRL: &[Entry] = &[Entry {
    fields: [32767, 0, 0, 0, 12],
    method: "?OnNcDestroy@CListCtrl@@IAAXXZ",
}];
const CCONTROLBAR: &[Entry] = &[
    Entry {
        fields: [15, 0, 0, 0, 12],
        method: "?OnPaint@CControlBar@@QAAXXZ",
    },
    Entry {
        fields: [25, 0, 0, 0, 4],
        method: "?OnCtlColor@CControlBar@@QAAPAUHBRUSH__@@PAVCDC@@PAVCWnd@@I@Z",
    },
    Entry {
        fields: [867, 0, 0, 0, 10],
        method: "?OnIdleUpdateCmdUI@CControlBar@@QAAJIJ@Z",
    },
    Entry {
        fields: [865, 0, 0, 0, 10],
        method: "?OnSizeParent@CControlBar@@QAAJIJ@Z",
    },
    Entry {
        fields: [24, 0, 0, 0, 14],
        method: "?OnShowWindow@CWnd@@IAAXHI@Z",
    },
    Entry {
        fields: [31, 0, 0, 0, 12],
        method: "?OnCancelMode@CWnd@@IAAXXZ",
    },
    Entry {
        fields: [1, 0, 0, 0, 9],
        method: "?OnCreate@CControlBar@@QAAHPAUtagCREATESTRUCTW@@@Z",
    },
    Entry {
        fields: [2, 0, 0, 0, 12],
        method: "?OnDestroy@CControlBar@@QAAXXZ",
    },
    Entry {
        fields: [868, 0, 0, 0, 12],
        method: "?OnInitialUpdate@CControlBar@@QAAXXZ",
    },
    Entry {
        fields: [20, 0, 0, 0, 1],
        method: "?OnEraseBkgnd@CWnd@@IAAHPAVCDC@@@Z",
    },
];
const CTOOLBAR: &[Entry] = &[
    Entry {
        fields: [15, 0, 0, 0, 12],
        method: "?OnPaint@CToolBar@@IAAXXZ",
    },
    Entry {
        fields: [20, 0, 0, 0, 1],
        method: "?OnEraseBkgnd@CToolBar@@IAAHPAVCDC@@@Z",
    },
];
const CCECOMMANDBAR: &[Entry] = &[Entry {
    fields: [273, 0, 10, 10, 12],
    method: "??_9@$BOM@AA",
}];
const CSPLITTERWND: &[Entry] = &[
    Entry {
        fields: [32, 0, 0, 0, 3],
        method: "?OnSetCursor@CSplitterWnd@@IAAHPAVCWnd@@II@Z",
    },
    Entry {
        fields: [512, 0, 0, 0, 49],
        method: "?OnMouseMove@CSplitterWnd@@IAAXIVCPoint@@@Z",
    },
    Entry {
        fields: [15, 0, 0, 0, 12],
        method: "?OnPaint@CSplitterWnd@@IAAXXZ",
    },
    Entry {
        fields: [513, 0, 0, 0, 49],
        method: "?OnLButtonDown@CSplitterWnd@@IAAXIVCPoint@@@Z",
    },
    Entry {
        fields: [515, 0, 0, 0, 49],
        method: "?OnLButtonDblClk@CSplitterWnd@@IAAXIVCPoint@@@Z",
    },
    Entry {
        fields: [514, 0, 0, 0, 49],
        method: "?OnLButtonUp@CSplitterWnd@@IAAXIVCPoint@@@Z",
    },
    Entry {
        fields: [256, 0, 0, 0, 16],
        method: "?OnKeyDown@CSplitterWnd@@IAAXIII@Z",
    },
    Entry {
        fields: [5, 0, 0, 0, 17],
        method: "?OnSize@CSplitterWnd@@IAAXIHH@Z",
    },
    Entry {
        fields: [276, 0, 0, 0, 29],
        method: "?OnHScroll@CSplitterWnd@@IAAXIIPAVCScrollBar@@@Z",
    },
    Entry {
        fields: [277, 0, 0, 0, 29],
        method: "?OnVScroll@CSplitterWnd@@IAAXIIPAVCScrollBar@@@Z",
    },
    Entry {
        fields: [274, 0, 0, 0, 18],
        method: "?OnSysCommand@CSplitterWnd@@IAAXIJ@Z",
    },
    Entry {
        fields: [31, 0, 0, 0, 12],
        method: "?OnCancelMode@CSplitterWnd@@IAAXXZ",
    },
    Entry {
        fields: [26, 0, 0, 0, 12],
        method: "?OnDisplayChange@CSplitterWnd@@IAAXXZ",
    },
];
const CTABCTRL: &[Entry] = &[Entry {
    fields: [2, 0, 0, 0, 12],
    method: "?OnDestroy@CTabCtrl@@IAAXXZ",
}];
const CVIEW: &[Entry] = &[
    Entry {
        fields: [15, 0, 0, 0, 12],
        method: "?OnPaint@CView@@IAAXXZ",
    },
    Entry {
        fields: [1, 0, 0, 0, 9],
        method: "?OnCreate@CView@@IAAHPAUtagCREATESTRUCTW@@@Z",
    },
    Entry {
        fields: [2, 0, 0, 0, 12],
        method: "?OnDestroy@CView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57653, 57653, 2],
        method: "?OnSplitCmd@CView@@IAAHI@Z",
    },
    Entry {
        fields: [273, 4294967295, 57653, 57653, 44],
        method: "?OnUpdateSplitCmd@CView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57680, 57680, 44],
        method: "?OnUpdateNextPaneMenu@CView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 0, 57680, 57680, 2],
        method: "?OnNextPaneCmd@CView@@IAAHI@Z",
    },
    Entry {
        fields: [273, 4294967295, 57681, 57681, 44],
        method: "?OnUpdateNextPaneMenu@CView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 0, 57681, 57681, 2],
        method: "?OnNextPaneCmd@CView@@IAAHI@Z",
    },
    Entry {
        fields: [868, 0, 0, 0, 12],
        method: "??_9@$BMI@AA",
    },
];
const CDOCUMENT: &[Entry] = &[
    Entry {
        fields: [273, 0, 57602, 57602, 12],
        method: "?OnFileClose@CDocument@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57603, 57603, 12],
        method: "?OnFileSave@CDocument@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57604, 57604, 12],
        method: "?OnFileSaveAs@CDocument@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 1, 1, 12],
        method: "?OnOK@CDocument@@IAAXXZ",
    },
];
const CCTRLVIEW: &[Entry] = &[Entry {
    fields: [15, 0, 0, 0, 12],
    method: "?OnPaint@CCtrlView@@IAAXXZ",
}];
const CEDITVIEW: &[Entry] = &[
    Entry {
        fields: [273, 4294967295, 57635, 57635, 44],
        method: "?OnUpdateNeedSel@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57637, 57637, 44],
        method: "?OnUpdateNeedClip@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57642, 57642, 44],
        method: "?OnUpdateNeedText@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57643, 57643, 44],
        method: "?OnUpdateEditUndo@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57636, 57636, 44],
        method: "?OnUpdateNeedText@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57641, 57641, 44],
        method: "?OnUpdateNeedText@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57640, 57640, 44],
        method: "?OnUpdateNeedFind@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57634, 57634, 44],
        method: "?OnUpdateNeedSel@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [273, 4294967295, 57632, 57632, 44],
        method: "?OnUpdateNeedSel@CEditView@@IAAXPAVCCmdUI@@@Z",
    },
    Entry {
        fields: [48401, 768, 0, 0, 35],
        method: "?OnEditChange@CEditView@@IAAHXZ",
    },
    Entry {
        fields: [1, 0, 0, 0, 9],
        method: "?OnCreate@CEditView@@IAAHPAUtagCREATESTRUCTW@@@Z",
    },
    Entry {
        fields: [48, 0, 0, 0, 10],
        method: "?OnSetFont@CEditView@@IAAJIJ@Z",
    },
    Entry {
        fields: [273, 0, 57635, 57635, 12],
        method: "?OnEditCut@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57634, 57634, 12],
        method: "?OnEditCopy@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57637, 57637, 12],
        method: "?OnEditPaste@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57632, 57632, 12],
        method: "?OnEditClear@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57643, 57643, 12],
        method: "?OnEditUndo@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57642, 57642, 12],
        method: "?OnEditSelectAll@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57636, 57636, 12],
        method: "?OnEditFind@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57641, 57641, 12],
        method: "?OnEditReplace@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [273, 0, 57640, 57640, 12],
        method: "?OnEditRepeat@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [2, 0, 0, 0, 12],
        method: "?OnDestroy@CEditView@@IAAXXZ",
    },
    Entry {
        fields: [514, 0, 0, 0, 49],
        method: "?OnLButtonUp@CEditView@@IAAXIVCPoint@@@Z",
    },
    Entry {
        fields: [257, 0, 0, 0, 16],
        method: "?OnKeyUp@CEditView@@IAAXIII@Z",
    },
    Entry {
        fields: [258, 0, 0, 0, 16],
        method: "?OnChar@CEditView@@IAAXIII@Z",
    },
    Entry {
        fields: [49152, 0, 0, 0, 0],
        method: "?OnFindReplaceCmd@CEditView@@IAAJIJ@Z",
    },
    Entry {
        fields: [135, 0, 0, 0, 35],
        method: "?OnGetDlgCode@CEditView@@IAAJXZ",
    },
];

const CCEDOCLIST: &[Entry] = &[Entry {
    fields: [48206, 11000, 0, 0, 38],
    method: "?OnItemActivated@CCeDocList@@QAAXPAUtagDLNHDR@@@Z",
}];
const MAPS: &[Map] = &[
    Map {
        class: "CCeDocListCB",
        base: Some("CCeCommandBar"),
        entries: CCECOMMANDBAR,
    },
    Map {
        class: "CCeDocList",
        base: Some("CWnd"),
        entries: CCEDOCLIST,
    },
    Map {
        class: "CCtrlView",
        base: Some("CView"),
        entries: CCTRLVIEW,
    },
    Map {
        class: "CEditView",
        base: Some("CCtrlView"),
        entries: CEDITVIEW,
    },
    Map {
        class: "CView",
        base: Some("CWnd"),
        entries: CVIEW,
    },
    Map {
        class: "CDocument",
        base: Some("CCmdTarget"),
        entries: CDOCUMENT,
    },
    Map {
        class: "CTabCtrl",
        base: Some("CWnd"),
        entries: CTABCTRL,
    },
    Map {
        class: "CSplitterWnd",
        base: Some("CWnd"),
        entries: CSPLITTERWND,
    },
    Map {
        class: "CCeCommandBar",
        base: Some("CToolBar"),
        entries: CCECOMMANDBAR,
    },
    Map {
        class: "CToolBar",
        base: Some("CControlBar"),
        entries: CTOOLBAR,
    },
    Map {
        class: "CControlBar",
        base: Some("CWnd"),
        entries: CCONTROLBAR,
    },
    Map {
        class: "CCmdTarget",
        base: None,
        entries: CCMDTARGET,
    },
    Map {
        class: "CWinApp",
        base: Some("CCmdTarget"),
        entries: CWINAPP,
    },
    Map {
        class: "CWnd",
        base: Some("CCmdTarget"),
        entries: CWND,
    },
    Map {
        class: "CFrameWnd",
        base: Some("CWnd"),
        entries: CFRAMEWND,
    },
    Map {
        class: "CDialog",
        base: Some("CWnd"),
        entries: CDIALOG,
    },
    Map {
        class: "CListCtrl",
        base: Some("CWnd"),
        entries: CLISTCTRL,
    },
];
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] = &[
    (
        2705,
        "?_GetBaseMessageMap@CCeDocList@@KAPBUAFX_MSGMAP@@XZ",
        exported_base,
    ),
    (
        2706,
        "?_GetBaseMessageMap@CCeDocListCB@@KAPBUAFX_MSGMAP@@XZ",
        exported_base,
    ),
    (
        519,
        "?_GetBaseMessageMap@CCtrlView@@KAPBUAFX_MSGMAP@@XZ",
        exported_base,
    ),
    (
        523,
        "?_GetBaseMessageMap@CEditView@@KAPBUAFX_MSGMAP@@XZ",
        exported_base,
    ),
    (
        2729,
        "?GetMessageMap@CCeDocListCB@@MBAPBUAFX_MSGMAP@@XZ",
        get_doclist_bar,
    ),
    (
        2728,
        "?GetMessageMap@CCeDocList@@MBAPBUAFX_MSGMAP@@XZ",
        get_doclist,
    ),
    (
        1354,
        "?GetMessageMap@CCtrlView@@MBAPBUAFX_MSGMAP@@XZ",
        get_cctrlview,
    ),
    (
        1358,
        "?GetMessageMap@CEditView@@MBAPBUAFX_MSGMAP@@XZ",
        get_ceditview,
    ),
    (
        1357,
        "?GetMessageMap@CDocument@@MBAPBUAFX_MSGMAP@@XZ",
        get_cdocument,
    ),
    (
        1378,
        "?GetMessageMap@CView@@MBAPBUAFX_MSGMAP@@XZ",
        get_cview,
    ),
    (
        1373,
        "?GetMessageMap@CTabCtrl@@MBAPBUAFX_MSGMAP@@XZ",
        get_ctabctrl,
    ),
    (
        1371,
        "?GetMessageMap@CSplitterWnd@@MBAPBUAFX_MSGMAP@@XZ",
        get_csplitterwnd,
    ),
    (
        2727,
        "?GetMessageMap@CCeCommandBar@@MBAPBUAFX_MSGMAP@@XZ",
        get_ccecommandbar,
    ),
    (
        1374,
        "?GetMessageMap@CToolBar@@MBAPBUAFX_MSGMAP@@XZ",
        get_ctoolbar,
    ),
    (
        1353,
        "?GetMessageMap@CControlBar@@MBAPBUAFX_MSGMAP@@XZ",
        get_ccontrolbar,
    ),
    (
        1350,
        "?GetMessageMap@CCmdTarget@@MBAPBUAFX_MSGMAP@@XZ",
        get_ccmdtarget,
    ),
    (
        1379,
        "?GetMessageMap@CWinApp@@MBAPBUAFX_MSGMAP@@XZ",
        get_cwinapp,
    ),
    (1380, "?GetMessageMap@CWnd@@MBAPBUAFX_MSGMAP@@XZ", get_cwnd),
    (
        1361,
        "?GetMessageMap@CFrameWnd@@MBAPBUAFX_MSGMAP@@XZ",
        get_cframewnd,
    ),
    (
        1355,
        "?GetMessageMap@CDialog@@MBAPBUAFX_MSGMAP@@XZ",
        get_cdialog,
    ),
    (
        1362,
        "?GetMessageMap@CListCtrl@@MBAPBUAFX_MSGMAP@@XZ",
        get_clistctrl,
    ),
];
fn get_ccmdtarget(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CCmdTarget")?))
}
fn get_cwinapp(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CWinApp")?))
}
fn get_cwnd(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CWnd")?))
}
fn get_cframewnd(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CFrameWnd")?))
}
fn get_cdialog(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CDialog")?))
}
fn get_clistctrl(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CListCtrl")?))
}
const BASE: &str = "__PocketHLE_MFC_MessageMapBase:";
const ENTRY: &str = "__PocketHLE_MFC_MessageEntry:";
fn definition(class: &str) -> Result<&'static Map, KernelError> {
    MAPS.iter()
        .find(|map| map.class == class)
        .ok_or_else(|| KernelError::Dispatch(format!("Unsupported MFC message map {class}")))
}
pub(super) fn register(d: &mut WinCeDispatcher) {
    for map in MAPS {
        d.register_handler("coredll.dll", &format!("{BASE}{}", map.class), base);
        for entry in map.entries {
            d.register_handler("coredll.dll", &format!("{ENTRY}{}", entry.method), invoke);
        }
    }
}
pub(super) fn address(ctx: &mut CallCtx<'_>, class: &str) -> Result<u32, KernelError> {
    let map = definition(class)?;
    let owner = super::application::owner(ctx)?;
    let key = format!("__PocketHLE_MFC_MessageMapData:{class}");
    if let Some(&pointer) = ctx.kernel.dynamic_exports[&owner].get(&key) {
        return Ok(pointer);
    }
    let exports = &ctx.kernel.dynamic_exports[&owner];
    let getter = *exports
        .get(&format!("{BASE}{class}"))
        .ok_or_else(|| KernelError::Dispatch("MFC message-map base export missing".into()))?;
    let mut entries = Vec::with_capacity((map.entries.len() + 1) * 6);
    for entry in map.entries {
        entries.extend(entry.fields);
        entries.push(
            *exports
                .get(&format!("{ENTRY}{}", entry.method))
                .ok_or_else(|| {
                    KernelError::Dispatch("MFC message-handler export missing".into())
                })?,
        );
    }
    entries.extend([0; 6]);
    let registered = class == "CEditView";
    let extent = 8 + entries.len() as u32 * 4;
    let pointer = alloc(ctx, extent + if registered { 4 } else { 0 })?;
    if registered {
        for entry in entries.chunks_exact_mut(6).filter(|e| e[0] == 0xc000) {
            entry[4] = pointer + extent;
        }
    }
    let mut words = vec![getter, pointer + 8];
    words.extend(entries);
    if registered {
        words.push(crate::coredll::registered_window_message_id(
            "commdlg_FindReplace",
        ));
    }
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    if let Err(e) = ctx.cpu.write_mem(pointer, &bytes) {
        ctx.kernel.heap.free(pointer);
        return Err(e.into());
    }
    ctx.kernel
        .dynamic_exports
        .get_mut(&owner)
        .unwrap()
        .insert(key, pointer);
    Ok(pointer)
}
fn symbol<'a>(ctx: &CallCtx<'a>, prefix: &str) -> Result<&'a str, KernelError> {
    let pocket_pe::ImportBinding::Name(name) = &ctx.thunk.binding else {
        return Err(KernelError::Dispatch(
            "MFC message-map symbol missing".into(),
        ));
    };
    name.strip_prefix(prefix)
        .ok_or_else(|| KernelError::Dispatch("Invalid MFC message-map symbol".into()))
}
fn base(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let class = symbol(ctx, BASE)?;
    let target = definition(class)?.base;
    let pointer = match target {
        Some(class) => address(ctx, class)?,
        None => 0,
    };
    Ok(DispatchOutcome::ReturnedR0(pointer))
}
fn invoke(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let method = symbol(ctx, ENTRY)?.to_owned();
    let slot = match method.as_str() {
        "??_9@$BME@AA" => Some(49),
        "??_9@$BMI@AA" => Some(50),
        "??_9@$BOM@AA" => Some(59),
        _ => None,
    };
    if let Some(slot) = slot {
        use super::callbacks::{virtual_function, Frame};
        let frame = Frame::enter(ctx)?;
        if !frame.fresh {
            let returned = ctx.cpu.read_reg(pocket_cpu::regs::ArmReg::R0)?;
            return frame.finish(ctx, returned);
        }
        let object = frame.arg(ctx, 0)?;
        let target = virtual_function(ctx, object, slot)?;
        return frame.call_args(ctx, 1, target, &[object]);
    }
    match super::handler_named(&method) {
        Some(handler) => handler(ctx),
        None => Err(KernelError::Dispatch(format!(
            "Unsupported MFC message handler: {method}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{framework_setup, result};
    use crate::bluetooth::tests::call;
    use pocket_cpu::{regs::ArmReg, Cpu};
    use pocket_kernel::DispatchOutcome;
    #[test]
    fn dialog_command_entries_call_the_live_virtual_override_and_release_their_continuation() {
        let (mut cpu, mut k, mut d) = framework_setup();
        let map = result(call(
            &mut cpu,
            &mut k,
            &mut d,
            "mfcce300.dll",
            "ord:1355",
            &[],
        ));
        let entries = cpu.read_u32_le(map + 4).unwrap();
        assert_eq!(cpu.read_u32_le(entries + 24).unwrap(), 273);
        assert_eq!(cpu.read_u32_le(entries + 24 + 8).unwrap(), 1);
        assert_eq!(cpu.read_u32_le(entries + 24 + 16).unwrap(), 12);
        cpu.write_mem(0x2000, &0x2400u32.to_le_bytes()).unwrap();
        cpu.write_mem(0x2400 + 49 * 4, &0x6000u32.to_le_bytes())
            .unwrap();
        let baseline = k.heap.free_bytes();
        let jump = call(
            &mut cpu,
            &mut k,
            &mut d,
            "coredll.dll",
            "__PocketHLE_MFC_MessageEntry:??_9@$BME@AA",
            &[0x2000],
        );
        assert!(matches!(jump, DispatchOutcome::JumpTo(0x6000)));
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 0x2000);
        cpu.write_reg(ArmReg::R0, 17).unwrap();
        super::super::callbacks::return_from_callback(&mut cpu, &mut k, &mut d);
        assert_eq!(
            result(call(
                &mut cpu,
                &mut k,
                &mut d,
                "coredll.dll",
                "__PocketHLE_MFC_MessageEntry:??_9@$BME@AA",
                &[]
            )),
            17
        );
        assert_eq!(k.heap.free_bytes(), baseline);
    }
}

/// Shared CE window/thread lookup: code and command ID are both zero.
/// Registered messages store the runtime message ID behind the signature word.
pub(super) fn find_window_entry(
    ctx: &mut CallCtx<'_>,
    entry: u32,
    message: u32,
) -> Result<Option<[u32; 6]>, KernelError> {
    find_entry(ctx, entry, |ctx, fields| {
        Ok(fields[1] == 0
            && fields[2] == 0
            && if message >= 0xc000 {
                fields[0] == 0xc000 && word(ctx, fields[4])? == message
            } else {
                fields[0] == message
            })
    })
}
pub(super) fn find_command_entry(
    ctx: &mut CallCtx<'_>,
    entry: u32,
    message: u32,
    code: u32,
    id: u32,
) -> Result<Option<[u32; 6]>, KernelError> {
    find_entry(ctx, entry, |_, fields| {
        Ok(fields[0] == message && fields[1] == code && fields[2] <= id && id <= fields[3])
    })
}
fn find_entry(
    ctx: &mut CallCtx<'_>,
    mut entry: u32,
    mut matches: impl FnMut(&mut CallCtx<'_>, [u32; 6]) -> Result<bool, KernelError>,
) -> Result<Option<[u32; 6]>, KernelError> {
    for _ in 0..4096 {
        let mut fields = [0; 6];
        for (i, field) in fields.iter_mut().enumerate() {
            let at = entry
                .checked_add(i as u32 * 4)
                .ok_or_else(|| KernelError::Dispatch("MFC message entry overflow".into()))?;
            *field = word(ctx, at)?;
        }
        if fields == [0; 6] {
            return Ok(None);
        }
        if matches(ctx, fields)? {
            return Ok(Some(fields));
        }
        entry = entry
            .checked_add(24)
            .ok_or_else(|| KernelError::Dispatch("MFC message-map overflow".into()))?;
    }
    Err(KernelError::Dispatch("Unterminated MFC message map".into()))
}

fn get_ctabctrl(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CTabCtrl")?))
}
fn get_ccontrolbar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CControlBar")?))
}

fn get_ctoolbar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CToolBar")?))
}

fn get_ccecommandbar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CCeCommandBar")?))
}

fn get_csplitterwnd(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CSplitterWnd")?))
}

fn get_cview(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CView")?))
}

fn get_cdocument(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CDocument")?))
}
fn get_cctrlview(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CCtrlView")?))
}
fn get_ceditview(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CEditView")?))
}

fn get_doclist(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CCeDocList")?))
}

fn get_doclist_bar(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    Ok(DispatchOutcome::ReturnedR0(address(ctx, "CCeDocListCB")?))
}

fn exported_base(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let name = match &ctx.thunk.binding {
        pocket_pe::ImportBinding::Name(name) => name.as_str(),
        pocket_pe::ImportBinding::Ordinal(ordinal) => EXPORTS
            .iter()
            .find(|entry| entry.0 == *ordinal)
            .map(|entry| entry.1)
            .ok_or_else(|| KernelError::Dispatch("Unknown MFC base message map".into()))?,
    };
    let class = name
        .split('@')
        .nth(1)
        .ok_or_else(|| KernelError::Dispatch("Invalid MFC base message map symbol".into()))?;
    let pointer = match definition(class)?.base {
        Some(parent) => address(ctx, parent)?,
        None => 0,
    };
    Ok(DispatchOutcome::ReturnedR0(pointer))
}
