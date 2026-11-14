//! Filtered PeekMessage must inspect queues without consuming unrelated
//! messages. In particular PM_NOYIELD render-interruption probes must not
//! mistake a synthetic WM_PAINT for a click or keyboard message.
use super::*;
type Message = (u32, u32, u32, u32);

fn matches(kernel: &KernelState, m: Message, window: u32, low: u32, high: u32) -> bool {
    if m.1 == WM_QUIT {
        return true;
    }
    let range = (low == 0 && high == 0) || (low <= m.1 && m.1 <= high);
    range
        && match window {
            0 => true,
            u32::MAX => m.0 == 0,
            _ => m.0 != 0 && focus_belongs_to_window(kernel, m.0, window),
        }
}

pub(super) fn peek(
    ctx: &mut CallCtx<'_>,
    out: u32,
    window: u32,
    low: u32,
    high: u32,
    mode: u32,
) -> Result<DispatchOutcome, KernelError> {
    let remove = mode & 1 != 0;
    let found = find(ctx, window, low, high, remove);
    if let Some(m) = found {
        write_retrieved_message(ctx, out, m.0, m.1, m.2, m.3)?;
        if remove {
            ctx.kernel.synthetic_message_count += 1;
        }
        return Ok(DispatchOutcome::ReturnedR0(1));
    }
    if mode & 2 == 0 {
        if let Some(result) = park_worker(ctx, 0)? {
            return Ok(result);
        }
        if let Some(result) = resume_worker(ctx, 0)? {
            return Ok(result);
        }
    }
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn find(ctx: &mut CallCtx<'_>, window: u32, low: u32, high: u32, remove: bool) -> Option<Message> {
    let accepts = |kernel: &KernelState, m| matches(kernel, m, window, low, high);
    if let Some(index) = ctx.kernel.current_thread.checked_sub(1) {
        let queue = &ctx.kernel.threads.get(index)?.messages;
        if let Some(i) = queue
            .iter()
            .position(|&(msg, wp, lp)| accepts(ctx.kernel, (0, msg, wp, lp)))
        {
            let (msg, wp, lp) = if remove {
                ctx.kernel.threads[index].messages.remove(i)?
            } else {
                queue[i]
            };
            return Some((0, msg, wp, lp));
        }
        if let Some(i) = ctx.kernel.posted_messages.iter().position(|m| {
            posted_message_belongs_to(ctx.kernel, m, ctx.kernel.current_thread)
                && accepts(ctx.kernel, *m)
        }) {
            return if remove {
                take_posted_message_at(ctx.kernel, i)
            } else {
                Some(ctx.kernel.posted_messages[i])
            };
        }
        if let Some(input) = find_host_input(ctx, window, low, high, remove) {
            return Some(input);
        }
    } else {
        if let Some((hwnd, lp)) = ctx.kernel.pending_create {
            let m = (hwnd, WM_CREATE, 0, lp);
            if accepts(ctx.kernel, m) {
                if remove {
                    ctx.kernel.pending_create = None;
                }
                return Some(m);
            }
        }
        if let Some(m) = ctx.kernel.pending_message {
            if accepts(ctx.kernel, m) {
                if remove {
                    ctx.kernel.pending_message = None;
                }
                return Some(m);
            }
        }
        if let Some(i) = ctx
            .kernel
            .pending_startup
            .iter()
            .position(|&(msg, wp, lp)| accepts(ctx.kernel, (FAKE_HWND, msg, wp, lp)))
        {
            let (msg, wp, lp) = if remove {
                ctx.kernel.pending_startup.remove(i)?
            } else {
                ctx.kernel.pending_startup[i]
            };
            return Some((FAKE_HWND, msg, wp, lp));
        }
        if let Some(input) = find_host_input(ctx, window, low, high, remove) {
            return Some(input);
        }
        let mixer = ctx.kernel.threads.iter().any(|t| t.started && !t.finished);
        if let Some(i) = ctx.kernel.posted_messages.iter().position(|&m| {
            !(m.1 == MM_WOM_DONE && mixer)
                && posted_message_belongs_to(ctx.kernel, &m, ctx.kernel.current_thread)
                && accepts(ctx.kernel, m)
        }) {
            return if remove {
                take_posted_message_at(ctx.kernel, i)
            } else {
                Some(ctx.kernel.posted_messages[i])
            };
        }
        if input_window_thread(ctx.kernel) == ctx.kernel.current_thread
            && ctx.kernel.controls.is_empty()
            && !ctx.kernel.held_keys.is_empty()
            && ctx
                .kernel
                .key_repeat_next_ms
                .is_some_and(|t| monotonic_ms() >= t)
        {
            let cursor = ctx.kernel.key_repeat_cursor % ctx.kernel.held_keys.len();
            let m = (
                input_window(ctx.kernel),
                WM_KEYDOWN,
                u32::from(ctx.kernel.held_keys[cursor]),
                0x40000001,
            );
            if accepts(ctx.kernel, m) {
                if remove {
                    key_repeat_if_due(ctx)?;
                }
                return Some(m);
            }
        }
    }
    // A filtered probe observes raw input without activating an HLE control.
    // Native shell drag loops also run on UI workers: preserve the actual
    // recipient, its client coordinates and thread ownership when inspecting
    // their mouse range, leaving unrelated events queued.
    let input = pending_input_is_available(ctx)
        .then(|| {
            ctx.kernel
                .pending_input
                .iter()
                .enumerate()
                .find_map(|(i, &event)| {
                    let event = remap_input(
                        event,
                        ctx.kernel.gapi_keys_queried,
                        ctx.kernel.gapi_action_key,
                    );
                    let message = addressed_input(ctx.kernel, event)?;
                    (window_thread(ctx.kernel, message.0) == ctx.kernel.current_thread
                        && accepts(ctx.kernel, message))
                    .then_some((i, event, message))
                })
        })
        .flatten();
    if let Some((i, event, m)) = input {
        if remove {
            ctx.kernel.remove_pending_input(i);
            if matches!(event, InputEvent::KeyDown { .. }) {
                restore_active_dialog_focus(ctx.kernel);
            }
            update_key_state(ctx, event);
        }
        return Some(m);
    }
    let now = monotonic_ms();
    let timer = ctx
        .kernel
        .timers
        .iter()
        .filter(|(key, value)| {
            key.0 == ctx.kernel.current_thread
                && value.1 <= now
                && accepts(ctx.kernel, (key.1, WM_TIMER, key.2, value.2))
        })
        .min_by_key(|(key, value)| (value.1, **key))
        .map(|(key, value)| (*key, *value));
    if let Some((key, value)) = timer {
        if remove {
            ctx.kernel.timers.get_mut(&key)?.1 =
                advance_deadline(value.1, u64::from(value.0).max(1), now);
        }
        return Some((key.1, WM_TIMER, key.2, value.2));
    }
    let target = if remove {
        idle_paint_target(ctx.kernel)
    } else {
        synthetic_paint_target(ctx.kernel)
    };
    let paint = (target, WM_PAINT, 0, 0);
    if ctx.kernel.current_thread == 0 && target != 0 && accepts(ctx.kernel, paint) {
        // The ordinary PeekMessage path guarantees an empty poll after a
        // fabricated paint. Filtered WM_PAINT probes need the same guarantee;
        // a preceding WM_CLOSE probe must not consume that opportunity.
        if ctx.kernel.synthetic_peek_paint_yield.remove(&0) {
            return None;
        }
        if now < ctx.kernel.synthetic_paint_next_ms {
            return None;
        }
        if remove {
            ctx.kernel.synthetic_paint_next_ms = advance_deadline(
                ctx.kernel.synthetic_paint_next_ms,
                SYNTHETIC_PAINT_INTERVAL_MS,
                now,
            );
            ctx.kernel.synthetic_peek_paint_yield.insert(0);
        }
        return Some(paint);
    }
    None
}

fn find_host_input(
    ctx: &mut CallCtx<'_>,
    window: u32,
    low: u32,
    high: u32,
    remove: bool,
) -> Option<Message> {
    if ctx.kernel.ce.windows.enabled || input_window_thread(ctx.kernel) != ctx.kernel.current_thread
    {
        return None;
    }
    let accepts = |kernel: &KernelState, message| matches(kernel, message, window, low, high);
    // A render probe observes raw input. It must neither activate an HLE
    // control nor discard a queued key while looking only for mouse input.
    let target = input_window(ctx.kernel);
    let input = ctx
        .kernel
        .pending_input
        .iter()
        .enumerate()
        .find_map(|(i, &event)| {
            let event = remap_input(
                event,
                ctx.kernel.gapi_keys_queried,
                ctx.kernel.gapi_action_key,
            );
            let (msg, wp, lp) = input_to_message(event)?;
            accepts(ctx.kernel, (target, msg, wp, lp)).then_some((i, event, (target, msg, wp, lp)))
        });
    if let Some((i, event, m)) = input {
        if remove {
            ctx.kernel.remove_pending_input(i);
            if matches!(event, InputEvent::KeyDown { .. }) {
                restore_active_dialog_focus(ctx.kernel);
            }
            update_key_state(ctx, event);
        }
        return Some(m);
    }
    None
}
