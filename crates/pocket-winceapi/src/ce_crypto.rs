//! WinCE CryptoAPI handlers.
use crate::{CallCtx, Handler, WinCeDispatcher};
use des::cipher::{Block, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use des::Des;
use pocket_kernel::{
    crypto::{CryptoHash, CryptoKey, CryptoPublicKey, Digest, Sha1},
    DispatchOutcome, KernelError,
};

const BAD_DATA: u32 = 0x8009_0005;
const BAD_FLAGS: u32 = 0x8009_0009;
const BAD_TYPE: u32 = 0x8009_000a;
const BAD_ALGID: u32 = 0x8009_0008;
const BAD_KEY: u32 = 0x8009_0003;
const BAD_HASH: u32 = 0x8009_0002;
const BAD_UID: u32 = 0x8009_0001;
const BAD_KEYSET: u32 = 0x8009_0016;
const MAX_BYTES: u32 = 8 * 1024 * 1024;
const OK: DispatchOutcome = DispatchOutcome::ReturnedR0(1);

pub fn register(d: &mut WinCeDispatcher) {
    for (name, handler) in [
        ("CryptAcquireContextW", acquire as Handler),
        ("CryptReleaseContext", release),
        ("CryptCreateHash", create_hash),
        ("CryptHashData", hash_data),
        ("CryptSetHashParam", set_hash_param),
        ("CryptImportKey", import_key),
        ("CryptVerifySignatureW", verify_signature),
        ("CryptDestroyHash", destroy_hash),
        ("CryptDeriveKey", derive_key),
        ("CryptSetKeyParam", set_key_param),
        ("CryptDestroyKey", destroy_key),
        ("CryptEncrypt", encrypt),
        ("CryptDecrypt", decrypt),
    ] {
        d.register_handler("coredll.dll", name, handler);
    }
}

fn fail(ctx: &mut CallCtx<'_>, error: u32) -> Result<DispatchOutcome, KernelError> {
    crate::coredll::set_thread_error(ctx, error);
    Ok(DispatchOutcome::ReturnedR0(0))
}

fn acquire(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let out = ctx.arg_u32(0)?;
    let container = ctx.arg_u32(1)?;
    let name = ctx.arg_u32(2)?;
    let kind = ctx.arg_u32(3)?;
    let flags = ctx.arg_u32(4)?;
    if out == 0 || container != 0 {
        return fail(ctx, 87);
    }
    if kind != 1 {
        return fail(ctx, 0x8009_0014);
    }
    if name != 0
        && !crate::ce_shortcut::text(ctx, name, 260).is_ok_and(|name| {
            name.eq_ignore_ascii_case("Microsoft Enhanced Cryptographic Provider v1.0")
        })
    {
        return fail(ctx, 0x8009_0013);
    }
    match flags {
        0 if !ctx.kernel.crypto.default_keyset => return fail(ctx, BAD_KEYSET),
        8 if ctx.kernel.crypto.default_keyset => return fail(ctx, 0x8009_000f),
        0 | 8 | 0xf000_0000 => {}
        _ => return fail(ctx, BAD_FLAGS),
    }
    let Some(handle) = ctx.kernel.crypto.allocate() else {
        return fail(ctx, 8);
    };
    if ctx.cpu.write_mem(out, &handle.to_le_bytes()).is_err() {
        return fail(ctx, 998);
    }
    ctx.kernel.crypto.providers.insert(handle);
    if flags == 8 {
        ctx.kernel.crypto.default_keyset = true;
    }
    Ok(OK)
}

fn release(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let provider = ctx.arg_u32(0)?;
    if ctx.arg_u32(1)? != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    let state = &mut ctx.kernel.crypto;
    if !state.providers.remove(&provider) {
        return fail(ctx, BAD_UID);
    }
    state.hashes.retain(|_, hash| hash.provider != provider);
    state.keys.retain(|_, key| key.provider != provider);
    state.public_keys.retain(|_, key| key.provider != provider);
    Ok(OK)
}

fn create_hash(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let provider = ctx.arg_u32(0)?;
    if !ctx.kernel.crypto.providers.contains(&provider) {
        return fail(ctx, BAD_UID);
    }
    if ctx.arg_u32(1)? != 0x8004 {
        return fail(ctx, BAD_ALGID);
    }
    if ctx.arg_u32(2)? != 0 || ctx.arg_u32(3)? != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    let out = ctx.arg_u32(4)?;
    let Some(handle) = ctx.kernel.crypto.allocate() else {
        return fail(ctx, 8);
    };
    if ctx.cpu.write_mem(out, &handle.to_le_bytes()).is_err() {
        return fail(ctx, 998);
    }
    ctx.kernel.crypto.hashes.insert(
        handle,
        CryptoHash {
            provider,
            digest: Sha1::new(),
            finished: false,
            value: None,
        },
    );
    Ok(OK)
}

fn hash_data(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let ptr = ctx.arg_u32(1)?;
    let len = ctx.arg_u32(2)?;
    if ctx.arg_u32(3)? != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    let Some(hash) = ctx.kernel.crypto.hashes.get_mut(&handle) else {
        return fail(ctx, BAD_HASH);
    };
    if hash.finished {
        return fail(ctx, 0x8009_000c);
    }
    if len > MAX_BYTES {
        return fail(ctx, 8);
    }
    if len != 0 {
        let Ok(bytes) = ctx.cpu.read_mem(ptr, len) else {
            return fail(ctx, 998);
        };
        hash.digest.update(&bytes);
    }
    Ok(OK)
}

fn set_hash_param(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let parameter = ctx.arg_u32(1)?;
    let data = ctx.arg_u32(2)?;
    if ctx.arg_u32(3)? != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    if parameter != 2 {
        return fail(ctx, BAD_TYPE);
    }
    let Some(hash) = ctx.kernel.crypto.hashes.get_mut(&handle) else {
        return fail(ctx, BAD_HASH);
    };
    if hash.finished {
        return fail(ctx, 0x8009_000c);
    }
    let mut digest = [0; 20];
    if ctx.cpu.read_mem_into(data, &mut digest).is_err() {
        return fail(ctx, 998);
    }
    hash.value = Some(digest);
    hash.finished = true;
    Ok(OK)
}

fn rsa_public_key(modulus_le: &[u8], exponent: u32) -> Option<rsa::RsaPublicKey> {
    rsa::RsaPublicKey::new(
        rsa::BigUint::from_bytes_le(modulus_le),
        rsa::BigUint::from(exponent),
    )
    .ok()
}

fn import_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let provider = ctx.arg_u32(0)?;
    let data = ctx.arg_u32(1)?;
    let len = ctx.arg_u32(2)?;
    if !ctx.kernel.crypto.providers.contains(&provider) {
        return fail(ctx, BAD_UID);
    }
    if ctx.arg_u32(3)? != 0 {
        return fail(ctx, BAD_KEY);
    }
    // Public keys may be imported with CRYPT_EXPORTABLE.
    if ctx.arg_u32(4)? & !1 != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    let out = ctx.arg_u32(5)?;
    if !(20..=532).contains(&len) {
        return fail(ctx, BAD_DATA);
    }
    let Ok(blob) = ctx.cpu.read_mem(data, len) else {
        return fail(ctx, 998);
    };
    let word = |offset| u32::from_le_bytes(blob[offset..offset + 4].try_into().unwrap());
    if blob[..4] != [6, 2, 0, 0]
        || !matches!(word(4), 0x2400 | 0xa400)
        || word(8) != u32::from_le_bytes(*b"RSA1")
    {
        return fail(ctx, BAD_DATA);
    }
    let bits = word(12);
    let exponent = word(16);
    let modulus = &blob[20..];
    if bits != (modulus.len() as u32) * 8
        || modulus.last().is_none_or(|last| last & 0x80 == 0)
        || rsa_public_key(modulus, exponent).is_none()
    {
        return fail(ctx, BAD_DATA);
    }
    let Some(handle) = ctx.kernel.crypto.allocate() else {
        return fail(ctx, 8);
    };
    if ctx.cpu.write_mem(out, &handle.to_le_bytes()).is_err() {
        return fail(ctx, 998);
    }
    ctx.kernel.crypto.public_keys.insert(
        handle,
        CryptoPublicKey {
            provider,
            modulus_le: modulus.to_vec(),
            exponent,
        },
    );
    Ok(OK)
}

fn verify_signature(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let hash = ctx.arg_u32(0)?;
    let signature = ctx.arg_u32(1)?;
    let len = ctx.arg_u32(2)?;
    let key = ctx.arg_u32(3)?;
    if ctx.arg_u32(4)? != 0 {
        return fail(ctx, 87);
    }
    if ctx.arg_u32(5)? != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    let Some(hash) = ctx.kernel.crypto.hashes.get_mut(&hash) else {
        return fail(ctx, BAD_HASH);
    };
    let Some(key) = ctx
        .kernel
        .crypto
        .public_keys
        .get(&key)
        .filter(|key| key.provider == hash.provider)
    else {
        return fail(ctx, BAD_KEY);
    };
    if len as usize != key.modulus_le.len() {
        return fail(ctx, 0x8009_0006);
    }
    let Ok(mut signature) = ctx.cpu.read_mem(signature, len) else {
        return fail(ctx, 998);
    };
    signature.reverse();
    let digest = hash
        .value
        .unwrap_or_else(|| hash.digest.clone().finalize().into());
    hash.finished = true;
    let key = rsa_public_key(&key.modulus_le, key.exponent)
        .ok_or_else(|| KernelError::Dispatch("invalid stored RSA public key".into()))?;
    // DER DigestInfo prefix for SHA-1, followed by the existing 20-byte digest.
    let scheme = rsa::Pkcs1v15Sign {
        hash_len: Some(20),
        prefix: Box::from(*b"\x30\x21\x30\x09\x06\x05\x2b\x0e\x03\x02\x1a\x05\x00\x04\x14"),
    };
    if key.verify(scheme, &digest, &signature).is_err() {
        return fail(ctx, 0x8009_0006);
    }
    Ok(OK)
}

fn destroy_hash(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    if ctx.kernel.crypto.hashes.remove(&handle).is_none() {
        return fail(ctx, BAD_HASH);
    }
    Ok(OK)
}

fn derive_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let provider = ctx.arg_u32(0)?;
    let hash = ctx.arg_u32(2)?;
    if !ctx.kernel.crypto.providers.contains(&provider) {
        return fail(ctx, BAD_UID);
    }
    if ctx.arg_u32(1)? != 0x6601 {
        return fail(ctx, BAD_ALGID);
    }
    if ctx.arg_u32(3)? & !0x10 != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    let out = ctx.arg_u32(4)?;
    let Some(source) = ctx
        .kernel
        .crypto
        .hashes
        .get(&hash)
        .filter(|h| h.provider == provider)
    else {
        return fail(ctx, BAD_HASH);
    };
    if source.finished {
        return fail(ctx, 0x8009_000c);
    }
    let digest = source.digest.clone().finalize();
    let bytes = digest[..8].try_into().unwrap();
    let Some(handle) = ctx.kernel.crypto.allocate() else {
        return fail(ctx, 8);
    };
    if ctx.cpu.write_mem(out, &handle.to_le_bytes()).is_err() {
        return fail(ctx, 998);
    }
    ctx.kernel.crypto.hashes.get_mut(&hash).unwrap().finished = true;
    ctx.kernel.crypto.keys.insert(
        handle,
        CryptoKey {
            provider,
            bytes,
            ecb: false,
        },
    );
    Ok(OK)
}

fn set_key_param(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let param = ctx.arg_u32(1)?;
    let ptr = ctx.arg_u32(2)?;
    if ctx.arg_u32(3)? != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    if param != 4 {
        return fail(ctx, BAD_TYPE);
    }
    if ctx.cpu.read_u32_le(ptr).ok() != Some(2) {
        return fail(ctx, BAD_DATA);
    }
    let Some(key) = ctx.kernel.crypto.keys.get_mut(&handle) else {
        return fail(ctx, BAD_KEY);
    };
    key.ecb = true;
    Ok(OK)
}

fn destroy_key(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    if ctx.kernel.crypto.keys.remove(&handle).is_none()
        && ctx.kernel.crypto.public_keys.remove(&handle).is_none()
    {
        return fail(ctx, BAD_KEY);
    }
    Ok(OK)
}

fn encrypt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    transform(ctx, true)
}

fn decrypt(ctx: &mut CallCtx<'_>) -> Result<DispatchOutcome, KernelError> {
    transform(ctx, false)
}

fn transform(ctx: &mut CallCtx<'_>, encrypt: bool) -> Result<DispatchOutcome, KernelError> {
    let handle = ctx.arg_u32(0)?;
    let final_block = ctx.arg_u32(2)? != 0;
    let data = ctx.arg_u32(4)?;
    let len_ptr = ctx.arg_u32(5)?;
    if ctx.arg_u32(1)? != 0 || ctx.arg_u32(3)? != 0 {
        return fail(ctx, BAD_FLAGS);
    }
    let Some(key) = ctx.kernel.crypto.keys.get(&handle) else {
        return fail(ctx, BAD_KEY);
    };
    if !key.ecb {
        return fail(ctx, BAD_TYPE);
    }
    let cipher = Des::new(&key.bytes.into());
    let Ok(len) = ctx.cpu.read_u32_le(len_ptr) else {
        return fail(ctx, 998);
    };
    if len > MAX_BYTES || ((!encrypt || !final_block) && len % 8 != 0) {
        return fail(ctx, BAD_DATA);
    }
    let output_len = if encrypt && final_block {
        (len / 8 + 1) * 8
    } else {
        len
    };
    if encrypt && (data == 0 || output_len > ctx.arg_u32(6)?) {
        if ctx
            .cpu
            .write_mem(len_ptr, &output_len.to_le_bytes())
            .is_err()
        {
            return fail(ctx, 998);
        }
        return if data == 0 { Ok(OK) } else { fail(ctx, 234) };
    }
    let mut bytes = if len == 0 {
        Vec::new()
    } else {
        let Ok(bytes) = ctx.cpu.read_mem(data, len) else {
            return fail(ctx, 998);
        };
        bytes
    };
    if encrypt && final_block {
        bytes.resize(output_len as usize, (output_len - len) as u8);
    }
    for chunk in bytes.chunks_exact_mut(8) {
        let block: &mut Block<Des> = chunk.try_into().unwrap();
        if encrypt {
            cipher.encrypt_block(block)
        } else {
            cipher.decrypt_block(block)
        }
    }
    if !encrypt && final_block {
        let padding = bytes.last().copied().unwrap_or(0) as usize;
        if !(1..=8).contains(&padding)
            || !bytes
                .iter()
                .rev()
                .take(padding)
                .all(|&b| b as usize == padding)
        {
            return fail(ctx, BAD_DATA);
        }
        bytes.truncate(bytes.len() - padding);
    }
    if (!bytes.is_empty() && ctx.cpu.write_mem(data, &bytes).is_err())
        || ctx
            .cpu
            .write_mem(len_ptr, &(bytes.len() as u32).to_le_bytes())
            .is_err()
    {
        return fail(ctx, 998);
    }
    Ok(OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::tests::{call, setup};
    use pocket_cpu::Cpu;

    #[test]
    fn ce_crypto_gtshell_sequence_matches_des_ecb_vector() {
        let (mut cpu, mut kernel, mut dispatcher) = setup();
        macro_rules! invoke {
            ($name:literal, $($arg:expr),* $(,)?) => {
                call(&mut cpu, &mut kernel, &mut dispatcher, "coredll.dll", $name, &[$($arg),*])
            };
        }
        assert_eq!(
            invoke!("CryptAcquireContextW", 0x1000, 0, 0, 1, 0),
            DispatchOutcome::ReturnedR0(0)
        );
        assert_eq!(invoke!("CryptAcquireContextW", 0x1000, 0, 0, 1, 8), OK);
        let provider = cpu.read_u32_le(0x1000).unwrap();
        assert_eq!(
            invoke!("CryptCreateHash", provider, 0x8004, 0, 0, 0x1004),
            OK
        );
        let hash = cpu.read_u32_le(0x1004).unwrap();
        cpu.write_mem(0x1100, b"abc").unwrap();
        assert_eq!(invoke!("CryptHashData", hash, 0x1100, 1, 0), OK);
        assert_eq!(invoke!("CryptHashData", hash, 0x1101, 2, 0), OK);
        assert_eq!(
            invoke!("CryptDeriveKey", provider, 0x6601, hash, 0x10, 0x1008),
            OK
        );
        let key = cpu.read_u32_le(0x1008).unwrap();
        assert_eq!(invoke!("CryptDestroyHash", hash), OK);
        cpu.write_mem(0x100c, &2u32.to_le_bytes()).unwrap();
        assert_eq!(invoke!("CryptSetKeyParam", key, 4, 0x100c, 0), OK);
        cpu.write_mem(0x1100, b"PocketHLE").unwrap();
        cpu.write_mem(0x1010, &9u32.to_le_bytes()).unwrap();
        assert_eq!(
            invoke!("CryptEncrypt", key, 0, 1, 0, 0x1100, 0x1010, 16),
            OK
        );
        assert_eq!(cpu.read_u32_le(0x1010).unwrap(), 16);
        assert_eq!(
            cpu.read_mem(0x1100, 16).unwrap(),
            [
                0x7a, 0xc7, 0x96, 0x18, 0xed, 0x0a, 0xfa, 0xe3, 0x81, 0x4c, 0xb3, 0x2b, 0xa3, 0xf4,
                0x97, 0x63
            ]
        );
        assert_eq!(invoke!("CryptDecrypt", key, 0, 1, 0, 0x1100, 0x1010), OK);
        assert_eq!(cpu.read_u32_le(0x1010).unwrap(), 9);
        assert_eq!(cpu.read_mem(0x1100, 9).unwrap(), b"PocketHLE");
        assert_eq!(invoke!("CryptDestroyKey", key), OK);
        assert_eq!(invoke!("CryptReleaseContext", provider, 0), OK);
        assert!(kernel.crypto.keys.is_empty() && kernel.crypto.providers.is_empty());
    }
}
