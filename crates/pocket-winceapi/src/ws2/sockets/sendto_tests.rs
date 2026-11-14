use super::*;
use crate::bluetooth::tests::{call, setup};
use pocket_cpu::Cpu;
use pocket_kernel::bluetooth::{BtResult, RfcommSocket};
use std::sync::{Arc, Mutex};

type Writes = Arc<Mutex<Vec<(Vec<u8>, i32)>>>;
struct Sender(Writes);
impl RfcommSocket for Sender {
    fn bind(&mut self, _: &SocketAddress) -> BtResult<()> {
        Err(10045)
    }
    fn listen(&mut self, _: i32) -> BtResult<()> {
        Err(10045)
    }
    fn connect(&mut self, _: &SocketAddress) -> BtResult<()> {
        Err(10045)
    }
    fn accept(&mut self) -> BtResult<(Box<dyn RfcommSocket>, SocketAddress)> {
        Err(10045)
    }
    fn read(&mut self, _: &mut [u8], _: i32) -> BtResult<usize> {
        Err(WOULD_BLOCK)
    }
    fn write(&mut self, bytes: &[u8], flags: i32) -> BtResult<usize> {
        self.0.lock().unwrap().push((bytes.to_vec(), flags));
        Ok(bytes.len().min(2))
    }
    fn readiness(&mut self) -> BtResult<(bool, bool, bool)> {
        Ok((false, true, false))
    }
    fn available(&mut self) -> BtResult<u32> {
        Ok(0)
    }
    fn set_option(&mut self, _: i32, _: i32, _: &[u8]) -> BtResult<()> {
        Err(10045)
    }
    fn get_option(&mut self, _: i32, _: i32, _: usize) -> BtResult<Vec<u8>> {
        Err(10045)
    }
    fn address(&mut self, _: bool) -> BtResult<SocketAddress> {
        Ok(SocketAddress::default())
    }
    fn shutdown(&mut self, _: i32) -> BtResult<()> {
        Ok(())
    }
}

#[test]
fn sendto_has_real_stream_byte_counts_and_winsock_failures_in_both_dlls() {
    for dll in ["winsock.dll", "ws2.dll"] {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        kernel.thread_last_errors.insert(0, 789);
        let args = [u32::MAX, 0x1400, 8, 0, 0, 0];
        assert_eq!(
            call(&mut cpu, &mut kernel, &mut dispatcher, dll, "sendto", &args),
            DispatchOutcome::ReturnedR0(SOCKET_ERROR)
        );
        assert_eq!(kernel.winsock_last_errors[&0], 10093);
        kernel.vfs.bluetooth.startups = 1;
        assert_eq!(
            call(&mut cpu, &mut kernel, &mut dispatcher, dll, "sendto", &args),
            DispatchOutcome::ReturnedR0(SOCKET_ERROR)
        );
        assert_eq!(kernel.winsock_last_errors[&0], 10038);
        cpu.write_mem(0x1400, b"message!").unwrap();
        cpu.map_region(0x5000, 4096, Prot::WRITE).unwrap();
        let writes = Writes::default();
        let handle = kernel
            .vfs
            .bluetooth
            .insert_socket(Box::new(Sender(writes.clone())), false)
            .unwrap();
        // The destination is ignored for this connection-oriented socket,
        // including a non-readable address, just as in Winsock sendto.
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                dll,
                "sendto",
                &[handle, 0x1400, 8, 2, 0x5000, 40]
            ),
            DispatchOutcome::ReturnedR0(2)
        );
        assert_eq!(*writes.lock().unwrap(), [(b"message!".to_vec(), 2)]);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                dll,
                "sendto",
                &[handle, 0x5000, 8, 0, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(SOCKET_ERROR)
        );
        assert_eq!(kernel.winsock_last_errors[&0], 10014);
        assert_eq!(
            call(
                &mut cpu,
                &mut kernel,
                &mut dispatcher,
                dll,
                "sendto",
                &[handle, 0x1400, u32::MAX, 0, 0, 0]
            ),
            DispatchOutcome::ReturnedR0(SOCKET_ERROR)
        );
        assert_eq!(kernel.winsock_last_errors[&0], 10022);
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert_eq!(kernel.thread_last_errors[&0], 789);
        assert_eq!(cpu.read_mem(0x1400, 8).unwrap(), b"message!");
    }
}
