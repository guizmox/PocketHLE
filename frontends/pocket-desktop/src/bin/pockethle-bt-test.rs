//! Optional native-radio smoke test. Never invoked by the emulator itself.
use pocket_core::kernel::bluetooth::{Service, PortParams, Port, WOULD_BLOCK};
use std::{sync::Arc, time::{Duration, Instant}};

fn main() {
    if let Err(e) = run() { eprintln!("Bluetooth test failed: {e}"); std::process::exit(1); }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || !matches!(args[0].as_str(), "scan" | "server" | "client") {
        println!("Usage: pockethle-bt-test scan | server | client AA:BB:CC:DD:EE:FF");
        return Err("choose scan, server or client".into());
    }
    let service = Service::default(); service.set_allowed(true); service.enable(true);
    if args[0] == "scan" {
        println!("Searching for Bluetooth Classic devices…");
        for device in service.active_backend().map_err(code)?.scan().map_err(code)? {
            let hex = format!("{:012X}",device.address);
            let mac = (0..6).map(|i| &hex[i*2..i*2+2]).collect::<Vec<_>>().join(":");
            println!("{mac}  {}",device.name);
        }
        return Ok(());
    }
    let server = args[0] == "server";
    let address = if server { 0 } else {
        let mac = args.get(1).ok_or("client needs a Bluetooth address")?.replace([':', '-'], "");
        if mac.len() != 12 { return Err("address must contain 12 hexadecimal digits".into()); }
        u64::from_str_radix(&mac,16).map_err(|_| "invalid Bluetooth address")?
    };
    let params = PortParams { server,address,channel:2,uuid:[0;16],flags:4 };
    let handle = service.register(4,&params).map_err(code)?;
    let port = service.port(4).ok_or("registration produced no port")?;
    let deadline = Instant::now()+Duration::from_secs(60);
    println!("{}; waiting up to 60 seconds. Pair the devices in OS settings first.", if server { "Server listening" } else { "Client connecting" });
    let result = if server {
        receive(&port,b"POCKETHLE-BT-PING\n",deadline).and_then(|_| send(&port,b"POCKETHLE-BT-PONG\n",deadline))
    } else {
        send(&port,b"POCKETHLE-BT-PING\n",deadline).and_then(|_| receive(&port,b"POCKETHLE-BT-PONG\n",deadline))
    };
    service.deregister(handle); result?;
    println!("PASS: real RFCOMM connection and data in both directions."); Ok(())
}
fn code(e: u32) -> String { format!("host error {e}") }
fn send(port: &Arc<Port>, bytes: &[u8], deadline: Instant) -> Result<(), String> {
    let mut offset=0;
    while offset<bytes.len() {
        if Instant::now()>=deadline { return Err("send timed out".into()); }
        match port.write(&bytes[offset..]) {
            Ok(0) => return Err("zero-byte write".into()), Ok(n) => offset+=n,
            Err(WOULD_BLOCK) => std::thread::sleep(Duration::from_millis(10)), Err(e) => return Err(code(e)),
        }
    }
    Ok(())
}
fn receive(port: &Arc<Port>, expected: &[u8], deadline: Instant) -> Result<(), String> {
    let mut received=Vec::new(); let mut bytes=[0;256];
    while received.len()<expected.len() {
        if Instant::now()>=deadline { return Err("receive timed out".into()); }
        match port.read(&mut bytes) {
            Ok(0) => return Err("peer closed before the complete message".into()), Ok(n) => received.extend_from_slice(&bytes[..n]),
            Err(WOULD_BLOCK) => std::thread::sleep(Duration::from_millis(10)), Err(e) => return Err(code(e)),
        }
    }
    if received==expected { Ok(()) } else { Err("received message does not match".into()) }
}
