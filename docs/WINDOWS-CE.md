# Windows CE desktop profiles

PocketHLE-Reborn runs guest applications with Rust implementations of CE APIs.
It can use an extracted CE filesystem for original resources, fonts, system
version metadata, ordinal names and registry hives. This is high-level emulation;
OEM kernels, drivers and every original Control Panel applet are not implemented.

## Windows

Extract the Windows release and start `pockethle-gui.exe`. The **Windows CE**
page accepts an application EXE, an extracted image directory, and a device data
directory. Use **Save profile** to keep those choices. For CE6, select the `fs`
directory containing `Windows/coredll.dll`, or its parent containing `fs/Windows`.

The profile ZIP starts the original `Windows/explorer.exe` through
`profiles/wince6-native.json`. Place your filesystem under `wince6/fs`, beside
the `profiles` directory. Put the Windows emulator
beside `run-wince6-gui.cmd`, or pass its path as the script's first argument.
Paths in a JSON profile file are resolved relative to that file's directory.

```cmd
pockethle-gui.exe --ce-profile profiles\wince6-native.json --windowed
```

`profiles/wince6-native-cerf.json` mounts additional ARM applications from
`cerf/arm` at `\CERF\`. Use the native template or the GUI's folder selectors
for a different location. The `mounts` list stores
`guest_prefix` and `host_dir`; paths in profile files are relative to that file,
while library settings resolve them against the library root. Windows and
Android share this contract, and children inherit the parent's live namespace.
Extra application folders use writable device overlays, preserving their sources.

Portable Explorer remains a separate profile and `run-wince6-portable.cmd`
launcher. Its optional files belong in `portable-explorer/bin`.

The private device directory receives writable filesystem overlays and
`registry.json`. The supplied image stays read-only. Original hives are imported
when that device registry is created; later launches preserve its changes.
Applications and independently running children inherit the same device registry
and mounted filesystem. Relative image/data paths in library backups are resolved
against the library root on both Windows and Android.

## Command Prompt

The original CE6 `Windows/cmd.exe` runs in a classic console window. From this
source workspace, start `tools/run-wince6-cmd-gui.cmd`; it selects
`profiles/wince6-cmd.json` and a separate `ce-data/wince6-cmd` device.
This template expects the image at `wince6/fs`,
beside the profiles directory. Profile packages include `run-wince6-cmd.cmd`.

From Explorer, use **Start → Run** and enter `\Windows\cmd.exe`. Commands use
guest CE paths, such as `dir \Windows` and `start \CERF\sampleapp.exe`. The
guest CMD implements its own commands and batch parser; the emulator provides
its console, file access and process APIs.

The console accepts committed Unicode text, Enter, Backspace, Delete, Home/End
and arrow keys. Up/Down recall entered commands; Page Up/Page Down and the mouse
wheel browse bounded output history. Typing returns to the input line. Ctrl+C
signals the registered console control event, and Ctrl+Z requests end of input.
The caption supports dragging, minimizing and closing.

Standard input/output paths and open file positions pass to child processes.
Redirection, append and pipelines use the mounted VFS. Redirected text follows
the CE ANSI code page; direct console output preserves Unicode. Console children
without their own window share the parent's input focus. A child that outlives
the console owner can create a replacement console window.

`DIR` receives creation/access/write times in its find records and prints
64-bit totals. `COPY` preserves the destination's timestamps through
`SetFileTime`, with real file access checks and the image's private overlay.
These behaviors are console compatibility, not a claim that every CE6
application or driver is implemented.

## Android

Install the signed universal APK (Android 7.0 or later; ARM64 and ARMv7). Its
application ID is `com.pockethle.app`.

Import the application and open its settings. Enable **Use Windows CE profile**,
then **Choose extracted CE image**. The Android file selector copies the selected
image into the app's library without changing the original. CE uses 640x480,
classic frames and an unlimited runtime by default. Command-line and classic-frame
options are saved with that application.

If a complete private CE backup ZIP has been prepared, use **Restore backup** and
**Merge without overwriting** instead. The backup includes the application,
relative profile paths and image files; the device registry is created from its
hives on first launch. The public profile-template ZIP alone has no CE binaries
or firmware and is not a complete Android library backup.

## Desktop behavior and Control Panel

Classic windows have caption buttons and can be moved by dragging their title
bar. The desktop background is `#3A6EA5`. Explorer remains behind child processes
such as BlankApp; closing a child removes its pixels and exposes the desktop.
In classic CE profiles, MessageBox captions also support dragging; their buttons
move with the dialog, and a drag release does not dismiss it. The box stays
within the logical screen and reveals the current windows beneath it.
Pointer releases and key releases return to the process that received the press.
The taskbar and active shell menus retain their own layers.
Native child windows hosted inside built-in controls participate in the same
composition. SampleApp's tab pages retain their controls, follow ancestor
visibility and clip to the tab and main window. Default painting and BeginPaint
erase a real window's own backing surface; they do not clear the shared screen
over already composed Control Panel icons or neighbouring windows.

Portable Explorer's Start menus, Run/Browse dialog and native list controls use
the mounted CE filesystem. Image lists retain copied icon pixels and overlay
state, and caption icons use the registered class or `WM_SETICON` resource.

Editable drop-down fields, including Explorer's Run field, accept committed
Unicode text. Clicking the text area focuses it; the arrow opens the history.
Selecting a history entry copies its text into the field, and typing edits that
text independently of the saved entries. Enter with the history closed activates
the dialog's default button.

The supplied CE6 `control.exe` can discover HLE applets through `CPlApplet`:
Owner, Display brightness, Keyboard repeat, Mouse double-click timing, System
identity, Volume & Sounds, and Input Panel. Forms validate and save to the private
shared registry. Owner has Identification/Notes pages, System has
General/Memory/Device Name pages, and Volume & Sounds separates Volume and Sounds.
The remaining supported applets expose their corresponding single page. Mouse,
arrow keys and Ctrl+Tab switch pages while preserving unsaved fields. Apply and
OK validate all pages; Cancel and the caption X discard edits since the last Apply.
Owner notes support multiline text, and Memory reads the current emulated-device
accounting when selected. The original ROM `cplmain.cpl` is used only for read-only
resources; its instructions and `DllMain` are not executed. Other OEM CPLs and
hardware settings remain outside this implementation.

System sounds use the original WAV files in the mounted `Windows` directory.
`MessageBeep`, named and numeric `PlaySound` aliases, desktop startup, message
boxes, menu actions and classic window transitions follow the live CE registry.
The positional `HKLM\Snd\Event` schemes retain empty entries as intentional
silence. **Default sounds** includes startup and alerts; **All sounds** also
enables menus and window transitions; **No sounds** silences system events.

In **Control Panel → Volume & Sounds**, enable **system events**, choose a
scheme, and select **Apply**. **Test** previews the current form without saving
it. Application sounds and system events have separate enable switches and
share the selected volume. Changes apply to the desktop and running children.
Stopping `PlaySound` or closing a wave-output device preserves the other audio
streams. Synchronous sound playback yields to the scheduler while it completes.

In classic profiles, border-only applications such as the original CE6 Control
Panel receive a Close button at the right of their existing command bar. A
release outside the button cancels the press; a completed click sends the normal
`WM_CLOSE` to the guest, which retains responsibility for saving and shutdown.
The command bar and list keep their original work-area geometry, preserving
File/View and the first row of applet icons. Other windows that request caption
buttons keep their classic title bars.

Desktop shortcuts use their CE text commands, including quoted executable names
and arguments. Document launches respect the registered default verb and expand
association placeholders once, preserving literal percent sequences in paths.
Shortcut reads are bounded by both the declared length and the actual VFS file;
some extracted ROM links omit the two trailing bytes counted in their header.
`LVIF_DI_SETITEM` retains only the display fields/subitems cached by the native
shell, avoiding repeated icon metadata callbacks on selection repaints.

Native in-process COM classes selected by the device registry can attach their
application DLL, obtain `IClassFactory` and create the requested interface. ARM
and Thumb callbacks preserve their thread, stack and continuation; the class
factory is released before activation returns. The existing HLE XML, media and
DirectShow providers retain their ownership rules. `CreateBindCtx` stores bind
options and named/bound objects with real IUnknown reference ownership; running
object-table and parameter enumeration are not implemented.

The original PegHelp and IESample can create their native `shdocvw.dll` browser
objects and application toolbars. WinINet option queries expose actual internet
handle types/parents and the local transport access state. That state does not
prove Internet reachability; complete HTML rendering and navigation remain
unverified. Pocket Word's private defaults supply the image's `\Windows`
template directory once, preserving existing registry values and subsequent
user deletions. These are application compatibility additions, not complete
Windows CE or browser conformance.

`GetTextFaceW` reports the requested DC's selected logical font face, with the
stock bitmap System face and the existing unnamed-font default. Its size query
and bounded UTF-16 copy count the terminator; invalid DCs/buffers fail without
drawing or publishing a frame. This supports Pocket Word's font-combobox setup;
complete font-substitution and rich-edit behavior remain separate work.

Original CE6 application DLLs such as `ceshell.dll` may have no relocation
table. The application loader reserves their preferred address and places import
thunks in a separate module slot. Existing mappings, including cold and
no-access pages, prevent the load with error 487. Unloading releases only that
module's exact mappings and allocator slot. Core OS DLLs and drivers continue
to use HLE providers; their ROM code is never executed.

The shell bootstrap exposes the existing keyboard through `SipEnumIM` and the
CE6 input-method CLSID ABI. `__GetUserKData` reads a DWORD at the SDK byte offset
from the live thread's KData window. API readiness queries report implemented
device providers and preserve cooperative waits for unavailable sets.
Original Explorer registers its own desktop and taskbar windows. These keep
their normal OS desktop parent, participate in desktop/taskbar composition and
suppress the fallback PocketPC strip while the registered taskbar exists.
Destroying a registered window removes its registration. `SignalStarted` records
the shell's startup identifier; it does not boot OEM services or drivers.
`SipSetDefaultRect` keeps the keyboard's rendering, hit testing and SIPINFO
geometry above the native taskbar, including rotated layouts. Hidden geometry
changes do not publish a new frame.
Native `CreateAPISet`/`RegisterAPISet` retain owned method tables and full 64-bit
CE6 signatures, expose registered IDs to readiness/name queries and release them
with their last handle or owning process. Handle-based API servers and general
cross-process signature marshalling remain outside this subset.
`CeRunAppAtEvent` subscriptions live in the shared device session, with cancellation
by normalized executable name. Setting the emulated UTC clock with `SetSystemTime`
or `SetLocalTime` delivers time-change notifications through the child runner;
saving a different device name delivers the machine-name event. Interval timers
and the host clock are unaffected. Other event sources require their actual
device providers, and subscriptions/clock edits are not persisted between sessions.
Bounded `StringCchCopyNExW`/`CatNExW` and their byte-counted aliases use the
CE seven-argument ABI, return end/remaining pointers and do not read beyond
the requested source component. The original shell uses these for paths.
Unsigned CE long-to-text conversions preserve all 32 bits for shell file sizes.
Both FILETIME/local-FILETIME directions preserve 100-ns precision in the virtual
UTC timezone.

Original Explorer also resolves shell display names through
`SHLoadIndirectString` while opening Start. Literal UTF-16 strings and
`@module,-resourceID` strings (including quoted paths and `;vN` modifiers) support
overlapping source/output buffers and bounded, terminated output. Resource reads
use only the named PE file, without attaching its DLL or searching unrelated
modules for the same ID. Missing or malformed resources return HRESULT failures;
resource-index locators and later Windows PRI formats remain unsupported.

The session clipboard stores Unicode/ANSI text, DIBs and memory-backed custom
formats such as Explorer's file-copy packets. Children share owned snapshots,
while GetClipboardData returns a handle in the reader's own heap and
GetClipboardDataAlloc returns a separately owned LocalFree-compatible copy.
Open/close ownership is per thread, enumeration distinguishes completion from
errors, and stale local copies are released when contents change. Delayed
rendering and clipboard GDI-object formats remain unsupported.

The original Explorer's recycle database uses typed CEDB properties, including
Unicode names, integers, timestamps and binary data. Databases and mounted
volumes persist through the private guest filesystem, with independent open
cursors, bounded buffers, sort keys and unique-key checks. A failed read-size
negotiation preserves the cursor; closing a database releases its handle before
volume unmount. Gizmondo contacts retain their existing separate store. These
volumes use PocketHLE's storage format; pre-existing foreign database files are
rejected without being overwritten.

CERF SampleApp's TreeView, Header and UpDown classes have independent control
state and rendering. Trees retain Unicode labels, item data, hierarchy,
expansion and selection; headers retain columns and display order; numeric
controls retain signed ranges and edit buddies. Tree selection/expansion and
numeric changes send synchronous parent notifications, including vetoes and
continuation restoration. In CE desktop mode, an unregistered class fails with
error 1407 instead of reentering the application's main WM_CREATE handler.
This is a supported common-control subset, not complete commctrl conformance.

## CE device and service boundaries

`GetPasswordActive` reads session-wide device metadata, inactive by default.
It preserves the calling thread's last error and never reads the Windows host
account. Password provisioning and authentication remain unsupported.

`RegisterAFSName` reserves a bounded, case-insensitive filesystem name and returns
an owned positive index. Invalid names/pointers return -1, duplicates fail, and
another process cannot deregister an owned index. Parent/child applications share
the namespace; process teardown releases only that owner's names. Reserving a name
does not mount a filesystem. `RegisterAFSEx` validates the index, API-set handle,
version and flags, then reports the absent handle-based filesystem marshalling.
`DeregisterAFS` distinguishes a reserved name from a mounted provider.

`ActivateDeviceEx` validates the HKLM path and complete ARM32 REGINI inputs before
examining the driver configuration. Missing keys/settings, malformed data and
unsupported providers have distinct failure results. Failed activation neither
creates an active driver handle nor publishes REGINI values. Supplying ROM service
executables is insufficient to implement their kernel/driver interfaces.

Winsock `sendto` shares the actual stream transport and its buffer, socket and
blocking error rules. For a connected RFCOMM stream the destination is ignored,
and the backend's partial byte count is returned. Unsupported IPv4/UDP socket
creation remains a failure; udp2tcp's invalid socket is not a successful send.

`PowerOffSystem` suspends the virtual CE device at CPU boundaries, retaining its
call until resume. Frontend Stop and input-file polling continue without taking
the shared execution gate. A new key, stylus-down or text event resumes the device
and emits the existing CE wakeup event; the input remains queued for the guest.
The Windows host is never suspended. Driver power callbacks, physical wake sources
and a full CE power-manager implementation remain separate work.

MSVC `__CxxThrowException` is a noreturn boundary: it records 0xe06d7363 and halts
with a bounded type diagnostic. Native C++ stack unwinding/catch dispatch remain
unsupported. EmulatorStub still requires the absent Device Emulator DMA7: channel;
handling its throw does not supply that driver or make the utility operational.

## Build and distribution

```cmd
tools\build-windows.cmd build --release -j 1 -p pocket-cli -p pocket-desktop --features pocket-cli/unicorn --locked
```

See [Build and release](BUILD.md#android) for the pinned Linux SDK/NDK workflow.
`tools/package-wince-profile.py --output profile.zip` produces public templates.
Adding `--image PATH` produces a private backup with the original Explorer.
`--cerf-apps PATH` includes the additional ARM applications and `--explorer-bin
PATH` optionally includes Portable Explorer as a separate library entry.
Do not publish that private backup as an emulator release.

Source and static FFmpeg reconstruction materials accompany the public binaries.
The xBRZ-containing binary distribution uses GPL-3.0 terms; individual components
retain their own licenses. Keep the permanent Android signing key outside Git
and reuse it for subsequent releases.
