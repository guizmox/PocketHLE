fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or("expected managed PE path")?;
    let bytes = std::fs::read(path)?;
    let option = args.next();
    let method = match option.as_deref() {
        Some(token) if token != "--frame" => {
            let token = token.to_str().ok_or("invalid token")?;
            let token = u32::from_str_radix(token, 16)?;
            if let Some((namespace, kind, member)) = pocket_netcf::referenced_member(&bytes, token)
            {
                println!("{namespace}.{kind}.{member}");
                return Ok(());
            }
            pocket_netcf::method(&bytes, token)
        }
        _ => pocket_netcf::entry_point(&bytes),
    }
    .ok_or("no supported IL method")?;
    println!(
        "{} {} ({:#010x}): {} IL bytes",
        method.runtime_version,
        method.method_name,
        method.token,
        method.il.len()
    );
    let mut offset = 0;
    while offset < method.il.len() {
        match pocket_netcf::il::decode(method.il, offset) {
            Ok((instruction, next)) => {
                println!("{offset:04x} {instruction:?}");
                offset = next;
            }
            Err(error) => {
                println!("{error:?}");
                println!(
                    "{:02x?}",
                    &method.il[offset..][..method.il.len().saturating_sub(offset).min(16)]
                );
                break;
            }
        }
    }
    let mut session = pocket_netcf::Session::start(&bytes, (240, 320), 500_000)
        .map_err(|error| format!("{error:?}"))?;
    println!("Application.Run entered: {}", session.running());
    for frame in 0..10 {
        session
            .tick(frame * 40, 500_000)
            .map_err(|error| format!("timer {frame}: {error:?}"))?;
    }
    if let Some((generation, (w, h), pixels)) = session.frame() {
        println!("Frame {generation}: {w}x{h}, {} bytes", pixels.len());
        if let Some(output) = args.next() {
            std::fs::write(output, pixels)?;
        }
    }
    Ok(())
}
