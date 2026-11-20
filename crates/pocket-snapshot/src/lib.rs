//! Shared bounded serialization/compression for native and managed checkpoints.
//! The owner defines section IDs and versions; no backend resources live here.
pub mod fields;
use bincode::Options;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
pub const PAYLOAD_LIMIT: u64 = 512 * 1024 * 1024;
pub const IO_BUFFER_SIZE: usize = 64 * 1024;
pub fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(PAYLOAD_LIMIT)
}
pub fn write_stream<W: Write>(
    mut output: W,
    magic: &[u8; 8],
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> io::Result<()> {
    output.write_all(magic)?;
    let compressed = flate2::write::ZlibEncoder::new(output, flate2::Compression::fast());
    let mut buffered = BufWriter::with_capacity(IO_BUFFER_SIZE, compressed);
    write(&mut buffered)?;
    buffered.into_inner()?.finish()?;
    Ok(())
}
pub fn read_stream<R: Read, T>(
    mut input: R,
    magic: &[u8; 8],
    read: impl FnOnce(&mut dyn Read) -> io::Result<T>,
) -> io::Result<T> {
    let mut actual = [0; 8];
    input.read_exact(&mut actual)?;
    if &actual != magic {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported snapshot format",
        ));
    }
    let compressed = CompleteZlib::new(input);
    let mut buffered = BufReader::with_capacity(IO_BUFFER_SIZE, compressed);
    let value = read(&mut buffered)?;
    if buffered.read(&mut [0; 1])? != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "trailing snapshot data",
        ));
    }
    Ok(value)
}
/// flate2's Read adapter treats an incomplete stream at EOF like a normal EOF.
/// Checkpoints require StreamEnd, including the zlib checksum, and no suffix.
/// The codec/DEFLATE implementation remains flate2; this adapter checks completion.
struct CompleteZlib<R> {
    input: BufReader<R>,
    decoder: flate2::Decompress,
    ended: bool,
}
impl<R: Read> CompleteZlib<R> {
    fn new(input: R) -> Self {
        Self {
            input: BufReader::with_capacity(IO_BUFFER_SIZE, input),
            decoder: flate2::Decompress::new(true),
            ended: false,
        }
    }
}
impl<R: Read> Read for CompleteZlib<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.ended {
            return Ok(0);
        }
        loop {
            let input = self.input.fill_buf()?;
            let eof = input.is_empty();
            let before_in = self.decoder.total_in();
            let before_out = self.decoder.total_out();
            let status = self
                .decoder
                .decompress(
                    input,
                    output,
                    if eof {
                        flate2::FlushDecompress::Finish
                    } else {
                        flate2::FlushDecompress::None
                    },
                )
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let consumed = (self.decoder.total_in() - before_in) as usize;
            let produced = (self.decoder.total_out() - before_out) as usize;
            self.input.consume(consumed);
            if status == flate2::Status::StreamEnd {
                if !self.input.fill_buf()?.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "trailing compressed snapshot data",
                    ));
                }
                self.ended = true;
                return Ok(produced);
            }
            if produced != 0 {
                return Ok(produced);
            }
            if eof {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete compressed snapshot",
                ));
            }
            if consumed == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "snapshot decoder made no progress",
                ));
            }
        }
    }
}
pub fn write_document(
    output: impl Write,
    magic: &[u8; 8],
    value: &impl serde::Serialize,
) -> io::Result<()> {
    write_stream(output, magic, |out| {
        let size = codec().serialized_size(value).map_err(io::Error::other)?;
        if size > PAYLOAD_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "snapshot exceeds size limit",
            ));
        }
        out.write_all(&size.to_le_bytes())?;
        codec().serialize_into(out, value).map_err(io::Error::other)
    })
}
pub fn read_document<T: serde::de::DeserializeOwned>(
    input: impl Read,
    magic: &[u8; 8],
) -> io::Result<T> {
    read_stream(input, magic, |input| {
        let mut size = [0; 8];
        input.read_exact(&mut size)?;
        let size = u64::from_le_bytes(size);
        if size > PAYLOAD_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "snapshot exceeds size limit",
            ));
        }
        let mut bounded = input.take(size);
        let value = codec()
            .with_limit(size)
            .deserialize_from(&mut bounded)
            .map_err(io::Error::other)?;
        if bounded.limit() != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "snapshot length mismatch",
            ));
        }
        Ok(value)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const MAGIC: &[u8; 8] = b"PHTEST01";
    #[test]
    fn document_requires_complete_payload_and_valid_checksum() {
        let value = vec![7u8; 4096];
        let mut bytes = Vec::new();
        write_document(&mut bytes, MAGIC, &value).unwrap();
        assert_eq!(
            read_document::<Vec<u8>>(bytes.as_slice(), MAGIC).unwrap(),
            value
        );
        for cut in 0..bytes.len() {
            assert!(
                read_document::<Vec<u8>>(&bytes[..cut], MAGIC).is_err(),
                "truncation at {cut}"
            );
        }
        struct Short<'a>(&'a [u8]);
        impl Read for Short<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                let n = buf.len().min(1).min(self.0.len());
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        assert_eq!(
            read_document::<Vec<u8>>(Short(&bytes), MAGIC).unwrap(),
            value
        );
        let mut suffix = bytes.clone();
        suffix.extend_from_slice(&[0]);
        assert!(read_document::<Vec<u8>>(suffix.as_slice(), MAGIC).is_err());
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(read_document::<Vec<u8>>(bytes.as_slice(), MAGIC).is_err());
    }
    #[test]
    fn oversized_declarations_fail_before_decoding_an_object() {
        let mut bytes = Vec::new();
        write_stream(&mut bytes, MAGIC, |out| {
            out.write_all(&u64::MAX.to_le_bytes())
        })
        .unwrap();
        assert!(read_document::<Vec<u8>>(bytes.as_slice(), MAGIC).is_err());
    }
}
