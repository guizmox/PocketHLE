//! Bounded received-data packets from Java. No host transport is replayed.
use pocket_core::kernel::{bluetooth::TransportSnapshot, internet::BufferedTransfer};

pub(crate) fn http(bytes: &[u8]) -> Result<BufferedTransfer, String> {
    let length = u32::from_le_bytes(
        bytes
            .get(..4)
            .ok_or("short HTTP checkpoint")?
            .try_into()
            .unwrap(),
    ) as usize;
    if length > 1024 * 1024 || bytes.len() < 4 + length || bytes.len() - 4 - length > 65536 {
        return Err("invalid Android HTTP checkpoint length".into());
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&bytes[4..4 + length]).map_err(|e| e.to_string())?;
    let code = |field: &str| -> Result<u32, String> {
        metadata[field]
            .as_u64()
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| format!("invalid Android HTTP {field}"))
    };
    let head_error = code("head_error")?;
    let head = if head_error == 0 {
        Ok(serde_json::from_value(metadata["head"].clone()).map_err(|e| e.to_string())?)
    } else {
        Err(head_error)
    };
    let end = code("end_error")?;
    BufferedTransfer::new(
        head,
        bytes[4 + length..].to_vec(),
        if end == 0 { Ok(()) } else { Err(end) },
    )
}

pub(crate) fn bluetooth(bytes: &[u8]) -> Result<TransportSnapshot, String> {
    if bytes.len() < 4 || bytes.len() > 4 + 65536 {
        return Err("invalid Android Bluetooth checkpoint length".into());
    }
    let code = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    TransportSnapshot::new(
        bytes[4..].to_vec(),
        if code == 0 { Ok(()) } else { Err(code) },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn truncated_and_oversized_provider_packets_are_rejected() {
        assert!(http(&[]).is_err());
        assert!(http(&[0xff; 4]).is_err());
        assert!(bluetooth(&[0; 3]).is_err());
        assert!(bluetooth(&vec![0; 65541]).is_err());
    }
    #[test]
    fn pending_http_can_checkpoint_without_fabricated_headers() {
        let metadata = br#"{"head":null,"head_error":12030,"end_error":12030}"#;
        let mut bytes = (metadata.len() as u32).to_le_bytes().to_vec();
        bytes.extend(metadata);
        assert!(http(&bytes).is_ok());
        assert!(bluetooth(&10054u32.to_le_bytes()).is_ok());
    }
}
