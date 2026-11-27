//! Shared CCmdTarget base teardown for windows and framework threads.
use super::callbacks::Frame;
use super::*;
pub(super) const DESTRUCT: &str = "__PocketHLE_MFC_DestructTarget";
pub(super) const EXPORTS: &[(u16, &str, crate::Handler)] =
    &[(226, "??1CCmdTarget@@UAA@XZ", destruct)];
pub(super) fn register(d: &mut WinCeDispatcher) {
    d.register_handler("coredll.dll", DESTRUCT, destruct);
}
fn destruct(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let f = Frame::enter(ctx)?;
    let o = f.arg(ctx, 0)?;
    if f.fresh {
        ctx.cpu
            .check_guest_access(o, 32, pocket_cpu::Prot::READ | pocket_cpu::Prot::WRITE)?;
        let table = super::classes::vtable(ctx, "CCmdTarget")?;
        put(ctx, o, table)?;
        let interface = word(ctx, o + 0x10)?;
        if interface != 0 {
            let target = word(ctx, interface + 0x1c)?;
            return f.call_args(ctx, 1, target, &[o + 0x10]);
        }
    }
    put(ctx, o + 0x1c, 0)?;
    let table = super::classes::vtable(ctx, "CObject")?;
    put(ctx, o, table)?;
    f.finish(ctx, o)
}
