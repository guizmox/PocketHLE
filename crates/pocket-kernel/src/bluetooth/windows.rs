//! Windows Bluetooth Classic using the Microsoft Bluetooth/Winsock stack.
use super::*;
use windows_sys::Win32::{Devices::Bluetooth::*, Networking::WinSock::*, Foundation::GetLastError};
use windows_sys::core::GUID;
use std::mem::size_of;
use std::time::{Duration, Instant};

pub struct WindowsBackend;
fn error() -> u32 { unsafe { WSAGetLastError() as u32 } }
fn init() -> BtResult<()> {
    static INIT: OnceLock<BtResult<()>> = OnceLock::new();
    *INIT.get_or_init(|| unsafe {
        let mut data = WSADATA::default();
        let code = WSAStartup(0x0202, &mut data);
        if code == 0 { Ok(()) } else { Err(code as u32) }
    })
}
fn uuid(params: &PortParams) -> GUID {
    let b = params.service_uuid();
    GUID { data1: u32::from_le_bytes(b[..4].try_into().unwrap()), data2: u16::from_le_bytes(b[4..6].try_into().unwrap()),
        data3: u16::from_le_bytes(b[6..8].try_into().unwrap()), data4: b[8..].try_into().unwrap() }
}
fn nonblocking(s: SOCKET) -> BtResult<()> {
    let mut yes = 1u32;
    if unsafe { ioctlsocket(s, FIONBIO, &mut yes) } == 0 { Ok(()) } else { Err(error()) }
}
struct Socket(SOCKET);
impl Drop for Socket { fn drop(&mut self) { unsafe { closesocket(self.0); } } }
impl Backend for WindowsBackend {
    fn hostname(&self) -> BtResult<String> { init()?; Ok(std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PocketHLE".into())) }
    fn scan(&self) -> BtResult<Vec<Device>> {
        init()?;
        unsafe {
            let mut radio = std::ptr::null_mut();
            let p = BLUETOOTH_FIND_RADIO_PARAMS { dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32 };
            let find_radio = BluetoothFindFirstRadio(&p, &mut radio);
            if find_radio.is_null() { return Err(NOT_READY); }
            BluetoothFindRadioClose(find_radio);
            let search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
                dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
                fReturnAuthenticated: 1, fReturnRemembered: 1, fReturnUnknown: 1,
                fReturnConnected: 1, fIssueInquiry: 1, cTimeoutMultiplier: 4, hRadio: radio,
            };
            let mut info = BLUETOOTH_DEVICE_INFO { dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32, ..Default::default() };
            let find = BluetoothFindFirstDevice(&search, &mut info);
            if find.is_null() {
                let e = GetLastError(); windows_sys::Win32::Foundation::CloseHandle(radio);
                return if e == 259 { Ok(Vec::new()) } else { Err(e) };
            }
            let mut devices = Vec::new();
            loop {
                let n = info.szName.iter().position(|b| *b == 0).unwrap_or(info.szName.len());
                devices.push(Device { address: info.Address.Anonymous.ullLong, name: String::from_utf16_lossy(&info.szName[..n]) });
                info.dwSize = size_of::<BLUETOOTH_DEVICE_INFO>() as u32;
                if BluetoothFindNextDevice(find, &mut info) == 0 { break; }
            }
            BluetoothFindDeviceClose(find); windows_sys::Win32::Foundation::CloseHandle(radio);
            devices.sort_by_key(|d| d.address); devices.dedup_by_key(|d| d.address);
            Ok(devices)
        }
    }
    fn open(&self, params: &PortParams) -> BtResult<Box<dyn Stream>> {
        init()?;
        let socket = unsafe { socket(AF_BTH as i32, SOCK_STREAM, BTHPROTO_RFCOMM as i32) };
        if socket == INVALID_SOCKET { return Err(error()); }
        let owned = Socket(socket); nonblocking(socket)?;
        let yes = 1i32;
        if unsafe { setsockopt(socket, SOL_RFCOMM as i32, SO_BTH_AUTHENTICATE as i32, (&yes as *const i32).cast(), 4) } != 0 { return Err(error()); }
        if params.flags & 8 != 0 && unsafe { setsockopt(socket, SOL_RFCOMM as i32, SO_BTH_ENCRYPT as i32, (&yes as *const i32).cast(), 4) } != 0 { return Err(error()); }
        let mut addr = SOCKADDR_BTH { addressFamily: AF_BTH, btAddr: if params.server { 0 } else { params.address }, serviceClassId: uuid(params), port: if params.server { u32::MAX } else { 0 } };
        let mut stream = WindowsStream { socket: owned, peer: None, server: params.server, connected: false,
            deadline: Instant::now() + Duration::from_secs(30), advertisement: None };
        if params.server {
            if unsafe { bind(socket, (&addr as *const SOCKADDR_BTH).cast(), size_of::<SOCKADDR_BTH>() as i32) } != 0 { return Err(error()); }
            if unsafe { listen(socket, 1) } != 0 { return Err(error()); }
            let mut len = size_of::<SOCKADDR_BTH>() as i32;
            if unsafe { getsockname(socket, (&mut addr as *mut SOCKADDR_BTH).cast(), &mut len) } != 0 { return Err(error()); }
            advertise(&mut addr, &mut uuid(params), RNRSERVICE_REGISTER)?;
            stream.advertisement = Some((addr, uuid(params)));
        } else {
            let status = unsafe { connect(socket, (&addr as *const SOCKADDR_BTH).cast(), size_of::<SOCKADDR_BTH>() as i32) };
            if status == 0 { stream.connected = true; } else {
                let e = error(); if e != WSAEWOULDBLOCK as u32 && e != WSAEINPROGRESS as u32 { return Err(e); }
            }
        }
        Ok(Box::new(stream))
    }
}
fn advertise(addr: &mut SOCKADDR_BTH, guid: &mut GUID, operation: WSAESETSERVICEOP) -> BtResult<()> {
    let mut name: Vec<u16> = "PocketHLE RFCOMM\0".encode_utf16().collect();
    let mut cs = CSADDR_INFO { LocalAddr: SOCKET_ADDRESS { lpSockaddr: (addr as *mut SOCKADDR_BTH).cast(), iSockaddrLength: size_of::<SOCKADDR_BTH>() as i32 },
        RemoteAddr: SOCKET_ADDRESS::default(), iSocketType: SOCK_STREAM, iProtocol: BTHPROTO_RFCOMM as i32 };
    let query = WSAQUERYSETW { dwSize: size_of::<WSAQUERYSETW>() as u32, lpszServiceInstanceName: name.as_mut_ptr(), lpServiceClassId: guid,
        dwNameSpace: NS_BTH, dwNumberOfCsAddrs: 1, lpcsaBuffer: &mut cs, ..Default::default() };
    if unsafe { WSASetServiceW(&query, operation, 0) } == 0 { Ok(()) } else { Err(error()) }
}
struct WindowsStream { socket: Socket, peer: Option<Socket>, server: bool, connected: bool, deadline: Instant, advertisement: Option<(SOCKADDR_BTH, GUID)> }
impl WindowsStream {
    fn ready(&mut self) -> BtResult<SOCKET> {
        if self.server {
            if self.peer.is_none() {
                let peer = unsafe { accept(self.socket.0, std::ptr::null_mut(), std::ptr::null_mut()) };
                if peer == INVALID_SOCKET { return Err(error()); }
                let owned = Socket(peer); nonblocking(peer)?; self.peer = Some(owned);
            }
            return Ok(self.peer.as_ref().unwrap().0);
        }
        if !self.connected {
            if Instant::now() >= self.deadline { return Err(10060); }
            let mut write = FD_SET::default(); write.fd_count = 1; write.fd_array[0] = self.socket.0;
            let mut fail = write;
            let timeout = TIMEVAL::default();
            let count = unsafe { select(0, std::ptr::null_mut(), &mut write, &mut fail, &timeout) };
            if count == SOCKET_ERROR { return Err(error()); } if count == 0 { return Err(WOULD_BLOCK); }
            let mut code = 0i32; let mut len = 4;
            if unsafe { getsockopt(self.socket.0, SOL_SOCKET, SO_ERROR, (&mut code as *mut i32).cast(), &mut len) } != 0 { return Err(error()); }
            if code != 0 { return Err(code as u32); }
            self.connected = true;
        }
        Ok(self.socket.0)
    }
}
impl Stream for WindowsStream {
    fn read(&mut self, bytes: &mut [u8]) -> BtResult<usize> {
        let socket = self.ready()?;
        let n = unsafe { recv(socket, bytes.as_mut_ptr(), bytes.len().min(i32::MAX as usize) as i32, 0) };
        if n == SOCKET_ERROR { Err(error()) } else { Ok(n as usize) }
    }
    fn write(&mut self, bytes: &[u8]) -> BtResult<usize> {
        let socket = self.ready()?;
        let n = unsafe { send(socket, bytes.as_ptr(), bytes.len().min(i32::MAX as usize) as i32, 0) };
        if n == SOCKET_ERROR { Err(error()) } else { Ok(n as usize) }
    }
}
impl Drop for WindowsStream { fn drop(&mut self) {
    if let Some((addr, guid)) = &mut self.advertisement { let _ = advertise(addr, guid, RNRSERVICE_DELETE); }
} }
