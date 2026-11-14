use super::*;
use crate::bluetooth::tests::{call, setup};
use pocket_cpu::{stub::StubCpu, Cpu};

fn fixture() -> (StubCpu, KernelState, crate::WinCeDispatcher) {
    let (mut cpu, mut kernel, dispatcher) = setup();
    cpu.map_region(0x5000_0000, 0x10000, Prot::READ | Prot::WRITE)
        .unwrap();
    kernel
        .window_classes
        .insert(0x201, "Clipboard owner".into());
    (cpu, kernel, dispatcher)
}

fn invoke(
    cpu: &mut StubCpu,
    kernel: &mut KernelState,
    d: &mut crate::WinCeDispatcher,
    name: &str,
    args: &[u32],
) -> u32 {
    let DispatchOutcome::ReturnedR0(value) = call(cpu, kernel, d, "coredll.dll", name, args) else {
        panic!("clipboard must not enter guest code")
    };
    value
}

fn allocate(
    cpu: &mut StubCpu,
    kernel: &mut KernelState,
    d: &mut crate::WinCeDispatcher,
    data: &[u8],
) -> u32 {
    let pointer = invoke(cpu, kernel, d, "LocalAlloc", &[0x40, data.len() as u32]);
    assert_ne!(pointer, 0);
    cpu.write_mem(pointer, data).unwrap();
    pointer
}

#[test]
fn text_and_shell_packets_cross_processes_as_local_handles_and_alloc_returns_owned_copies() {
    let (mut cpu, mut kernel, mut d) = fixture();
    let (mut child_cpu, mut child, mut child_d) = fixture();
    child.object_handles = kernel.object_handles.new_child().unwrap().1;
    let text: Vec<_> = "CE Проводник"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let text_pointer = allocate(&mut cpu, &mut kernel, &mut d, &text);
    let packet = [0x14, 0, 0, 0, 1, 0, 0x42, 0, 0, 0];
    let packet_pointer = allocate(&mut cpu, &mut kernel, &mut d, &packet);
    let format_name: Vec<_> = "Shell IDList Array"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    cpu.write_mem(0x1200, &format_name).unwrap();
    child_cpu.write_mem(0x1200, &format_name).unwrap();
    let format = invoke(
        &mut cpu,
        &mut kernel,
        &mut d,
        "RegisterClipboardFormatW",
        &[0x1200],
    );
    assert_eq!(
        format,
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "RegisterClipboardFormatW",
            &[0x1200]
        )
    );
    assert!(format >= 0xc000);
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "OpenClipboard", &[0x201]),
        1
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "OpenClipboard",
            &[0x201]
        ),
        0
    );
    assert_eq!(child.thread_last_errors[&0], 5);
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "EmptyClipboard", &[]),
        1
    );
    assert_eq!(
        invoke(
            &mut cpu,
            &mut kernel,
            &mut d,
            "SetClipboardData",
            &[13, text_pointer]
        ),
        text_pointer
    );
    assert_eq!(
        invoke(
            &mut cpu,
            &mut kernel,
            &mut d,
            "SetClipboardData",
            &[format, packet_pointer]
        ),
        packet_pointer
    );
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "CloseClipboard", &[]),
        1
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "CountClipboardFormats",
            &[]
        ),
        2
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "IsClipboardFormatAvailable",
            &[13]
        ),
        1
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "OpenClipboard",
            &[0x201]
        ),
        1
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "EnumClipboardFormats",
            &[0]
        ),
        13
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "EnumClipboardFormats",
            &[13]
        ),
        format
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "EnumClipboardFormats",
            &[format]
        ),
        0
    );
    assert_eq!(child.thread_last_errors[&0], 0);
    let borrowed = invoke(
        &mut child_cpu,
        &mut child,
        &mut child_d,
        "GetClipboardData",
        &[13],
    );
    assert_ne!(borrowed, 0);
    assert_eq!(
        child_cpu.read_mem(borrowed, text.len() as u32).unwrap(),
        text
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "GetClipboardData",
            &[13]
        ),
        borrowed
    );
    let owned = invoke(
        &mut child_cpu,
        &mut child,
        &mut child_d,
        "GetClipboardDataAlloc",
        &[13],
    );
    assert_ne!(owned, borrowed);
    assert_eq!(child.heap.msize(owned), Some(text.len() as u32));
    assert_eq!(child_cpu.read_mem(owned, text.len() as u32).unwrap(), text);
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "LocalFree",
            &[owned]
        ),
        0
    );
    assert!(child.heap.msize(borrowed).is_some());
    let child_packet = invoke(
        &mut child_cpu,
        &mut child,
        &mut child_d,
        "GetClipboardData",
        &[format],
    );
    assert_eq!(
        child_cpu
            .read_mem(child_packet, packet.len() as u32)
            .unwrap(),
        packet
    );
    // Destroying the producer must not destroy the consumer's payloads.
    kernel.object_handles.mark_inactive();
    cpu.write_mem(text_pointer, &vec![0xee; text.len()])
        .unwrap();
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "GetClipboardData",
            &[13]
        ),
        borrowed
    );
    assert_eq!(
        child_cpu.read_mem(borrowed, text.len() as u32).unwrap(),
        text
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "EmptyClipboard",
            &[]
        ),
        1
    );
    assert!(child.heap.msize(borrowed).is_none());
    assert!(child.heap.msize(child_packet).is_none());
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "CountClipboardFormats",
            &[]
        ),
        0
    );
}

#[test]
fn stale_transferred_blocks_are_released_on_the_next_open_and_lock_is_thread_owned() {
    let (mut cpu, mut kernel, mut d) = fixture();
    let pointer = allocate(&mut cpu, &mut kernel, &mut d, b"first\0");
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "OpenClipboard", &[0x201]),
        1
    );
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "EmptyClipboard", &[]),
        1
    );
    assert_eq!(
        invoke(
            &mut cpu,
            &mut kernel,
            &mut d,
            "SetClipboardData",
            &[1, pointer]
        ),
        pointer
    );
    kernel.current_thread = 1;
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "CloseClipboard", &[]),
        0
    );
    assert_eq!(
        kernel.thread_last_errors[&1],
        pocket_kernel::clipboard::NOT_OPEN
    );
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "OpenClipboard", &[0x201]),
        0
    );
    // A dead thread must release its clipboard lock too.
    kernel.object_handles.set_exit(Some(0), 0);
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "OpenClipboard", &[0x201]),
        1
    );
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "CloseClipboard", &[]),
        1
    );
    let (mut child_cpu, mut child, mut child_d) = fixture();
    child.object_handles = kernel.object_handles.new_child().unwrap().1;
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "OpenClipboard",
            &[0x201]
        ),
        1
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "EmptyClipboard",
            &[]
        ),
        1
    );
    assert_eq!(
        invoke(
            &mut child_cpu,
            &mut child,
            &mut child_d,
            "CloseClipboard",
            &[]
        ),
        1
    );
    assert!(kernel.heap.msize(pointer).is_some());
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "OpenClipboard", &[0x201]),
        1
    );
    assert!(kernel.heap.msize(pointer).is_none());
}

#[test]
fn failed_publication_preserves_data_and_enumeration_distinguishes_errors_from_end() {
    let (mut cpu, mut kernel, mut d) = fixture();
    let pointer = allocate(&mut cpu, &mut kernel, &mut d, b"data\0");
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "EmptyClipboard", &[]),
        0
    );
    assert_eq!(
        kernel.thread_last_errors[&0],
        pocket_kernel::clipboard::NOT_OPEN
    );
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "OpenClipboard", &[0xbad]),
        0
    );
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "OpenClipboard", &[0x201]),
        1
    );
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "EmptyClipboard", &[]),
        1
    );
    assert_eq!(
        invoke(
            &mut cpu,
            &mut kernel,
            &mut d,
            "SetClipboardData",
            &[1, pointer]
        ),
        pointer
    );
    for (format, handle, error) in [
        (1, 0, 50),
        (1, 0x1337, 6),
        (0, pointer, 87),
        (2, pointer, 50),
    ] {
        assert_eq!(
            invoke(
                &mut cpu,
                &mut kernel,
                &mut d,
                "SetClipboardData",
                &[format, handle]
            ),
            0
        );
        assert_eq!(kernel.thread_last_errors[&0], error);
        assert_eq!(
            invoke(&mut cpu, &mut kernel, &mut d, "GetClipboardData", &[1]),
            pointer
        );
        assert!(kernel.heap.msize(pointer).is_some());
    }
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "EnumClipboardFormats", &[8]),
        0
    );
    assert_eq!(kernel.thread_last_errors[&0], 87);
    assert_eq!(
        invoke(&mut cpu, &mut kernel, &mut d, "EnumClipboardFormats", &[1]),
        0
    );
    assert_eq!(kernel.thread_last_errors[&0], 0);
}
