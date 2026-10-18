//! COM8 NMEA stream backed by the existing, permission-gated GPS service.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub struct Port {
    pub device: Arc<Mutex<crate::gps::Device>>,
    pub queue: VecDeque<u8>,
    pub next: Instant,
    pub dcb: [u8; 28],
    pub timeouts: [u8; 20],
    pub mask: u32,
    pub access: u32,
    pub share: u32,
}
impl Port {
    pub fn new(device: Arc<Mutex<crate::gps::Device>>, access: u32, share: u32) -> Self {
        let mut dcb = [0u8; 28];
        dcb[..4].copy_from_slice(&28u32.to_le_bytes());
        dcb[4..8].copy_from_slice(&4800u32.to_le_bytes());
        dcb[8] = 1;
        dcb[18] = 8;
        Self {
            device,
            queue: VecDeque::new(),
            next: Instant::now(),
            dcb,
            timeouts: [0; 20],
            mask: 0,
            access,
            share,
        }
    }
    pub fn poll(&mut self) -> Result<(), u32> {
        if !self.device.lock().unwrap().access_allowed() {
            self.queue.clear();
            return Err(5);
        }
        if self.queue.is_empty() && Instant::now() >= self.next {
            let packet = self.device.lock().unwrap().packet()?;
            self.queue.extend(fix_sentences(&packet).bytes());
            self.next = Instant::now() + Duration::from_secs(1);
        }
        Ok(())
    }
}
fn coord(degrees: f64, lat: bool) -> (String, char) {
    let a = degrees.abs();
    let d = a.floor() as u32;
    let m = (a - d as f64) * 60.;
    (
        if lat {
            format!("{d:02}{m:09.6}")
        } else {
            format!("{d:03}{m:09.6}")
        },
        if lat {
            if degrees < 0. {
                'S'
            } else {
                'N'
            }
        } else if degrees < 0. {
            'W'
        } else {
            'E'
        },
    )
}
fn date(unix: u64) -> (i32, u32, u32) {
    let z = (unix / 86400) as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    (y as i32, m as u32, d as u32)
}
fn sentence(body: &str) -> String {
    let checksum = body.bytes().fold(0u8, |a, b| a ^ b);
    format!("${body}*{checksum:02X}\r\n")
}
/// Keep the serial compatibility port and GPS1 on the same fix. RMC carries
/// position/date; GGA and GSA carry the fix quality/mode used by CE receivers.
/// Unknown satellite identities, counts and DOP are not synthesized.
fn fix_sentences(p: &[u8; 180]) -> String {
    let word = |i| u32::from_le_bytes(p[i..i + 4].try_into().unwrap());
    let valid = word(15) != 0;
    let (lat, ns) = coord(word(21) as i32 as f64 / 1e7, true);
    let (lon, ew) = coord(word(25) as i32 as f64 / 1e7, false);
    let sec = (u64::from(word(11)) + 63072000) % 86400;
    let mode = u16::from_le_bytes(p[19..21].try_into().unwrap());
    let altitude = if valid && mode == 6 {
        format!("{:.2}", word(29) as i32 as f64 / 100.)
    } else {
        String::new()
    };
    let quality = u8::from(valid);
    let gga = sentence(&format!(
        "GPGGA,{:02}{:02}{:02}.00,{lat},{ns},{lon},{ew},{quality},00,,{altitude},M,,M,,",
        sec / 3600,
        sec / 60 % 60,
        sec % 60
    ));
    let mut gsa = vec![
        "GPGSA".to_owned(),
        "A".to_owned(),
        (if valid {
            if mode == 6 {
                3
            } else {
                2
            }
        } else {
            1
        })
        .to_string(),
    ];
    // Twelve satellite slots, then PDOP/HDOP/VDOP, all unavailable.
    gsa.extend(std::iter::repeat_n(String::new(), 15));
    gga + &sentence(&gsa.join(",")) + &rmc(p)
}
fn rmc(p: &[u8; 180]) -> String {
    let word = |i| u32::from_le_bytes(p[i..i + 4].try_into().unwrap());
    let lat = word(21) as i32 as f64 / 1e7;
    let lon = word(25) as i32 as f64 / 1e7;
    let (lat, ns) = coord(lat, true);
    let (lon, ew) = coord(lon, false);
    let unix = u64::from(word(11)) + 63072000;
    let sec = unix % 86400;
    let (y, m, d) = date(unix);
    let speed = u16::from_le_bytes(p[33..35].try_into().unwrap()) as f64 / 100. / 0.514444;
    let course = u16::from_le_bytes(p[35..37].try_into().unwrap()) as f64 / 100.;
    let body=format!("GPRMC,{:02}{:02}{:02}.00,{},{lat},{ns},{lon},{ew},{speed:.2},{course:.2},{d:02}{m:02}{:02},,",sec/3600,sec/60%60,sec%60,if word(15)!=0{'A'}else{'V'},y.rem_euclid(100));
    sentence(&body)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serial_reports_fix_type_without_inventing_satellites_or_dop() {
        let mut packet = [0u8; 180];
        packet[15..19].copy_from_slice(&1u32.to_le_bytes());
        packet[19..21].copy_from_slice(&5u16.to_le_bytes());
        let text = fix_sentences(&packet);
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            let (body, check) = line[1..].split_once('*').unwrap();
            assert_eq!(
                body.bytes().fold(0u8, |a, b| a ^ b),
                u8::from_str_radix(check, 16).unwrap()
            );
        }
        let gga: Vec<_> = lines[0].split('*').next().unwrap().split(',').collect();
        assert_eq!(gga[6], "1");
        assert_eq!(gga[7], "00");
        assert_eq!(gga[8], "");
        assert_eq!(gga[9], "");
        let gsa: Vec<_> = lines[1].split('*').next().unwrap().split(',').collect();
        assert_eq!(gsa.len(), 18);
        assert_eq!(gsa[2], "2");
        assert!(gsa[3..].iter().all(|s| s.is_empty()));
        packet[15..19].fill(0);
        let nofix = fix_sentences(&packet);
        assert!(nofix.lines().next().unwrap().split(',').nth(6) == Some("0"));
        assert!(nofix.lines().nth(1).unwrap().starts_with("$GPGSA,A,1,"));
        assert!(nofix.lines().nth(2).unwrap().contains(",V,"));
    }
    #[test]
    fn serial_fix_uses_existing_permission_and_nmea_checksum() {
        let service = crate::gps::Service::default();
        assert!(service.open().is_err());
        service
            .set_fixed_position(Some((44.052158, -123.095725)))
            .unwrap();
        service.set_allowed(true);
        let mut port = Port::new(service.open().unwrap(), 3, 0);
        port.poll().unwrap();
        let text = String::from_utf8(port.queue.iter().copied().collect()).unwrap();
        assert!(text.contains(",A,4403.129480,N,12305.743500,W,"), "{text}");
        let rmc = text
            .lines()
            .find(|line| line.starts_with("$GPRMC,"))
            .unwrap();
        let (body, sum) = rmc.trim().trim_start_matches('$').split_once('*').unwrap();
        assert_eq!(
            body.bytes().fold(0u8, |a, b| a ^ b),
            u8::from_str_radix(sum, 16).unwrap()
        );
        service.set_allowed(false);
        assert_eq!(port.poll(), Err(5));
        assert!(port.queue.is_empty());
    }
}
