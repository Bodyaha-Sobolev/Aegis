//! AEGIS02 authenticated source container. Legacy AEG0 is deliberately separate.
use crate::MAX_SOURCE;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, OsRng, Payload, rand_core::RngCore},
};
use zeroize::Zeroizing;

pub const MAGIC: &[u8; 8] = b"AEGIS02\0";
pub const HEADER: usize = 56;
pub const OVERHEAD: usize = HEADER + 16;
pub const MAX_KEY: usize = 65536;

pub fn is_protected(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

fn derive(key_file: &[u8], dialect: u16, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, String> {
    if !(32..=MAX_KEY).contains(&key_file.len()) {
        return Err("key file must contain 32–65536 bytes".into());
    }
    if dialect > 4095 {
        return Err("dialect must be in 0..4095".into());
    }
    let mut material = Zeroizing::new(key_file.to_vec());
    material.extend_from_slice(&dialect.to_le_bytes());
    let params = Params::new(19456, 2, 1, Some(32)).map_err(|e| e.to_string())?;
    let kdf = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    kdf.hash_password_into(&material, salt, &mut *key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

pub fn generate_key() -> Result<Zeroizing<[u8; 32]>, String> {
    let mut key = Zeroizing::new([0; 32]);
    OsRng
        .try_fill_bytes(&mut *key)
        .map_err(|e| format!("system random generator failed: {e}"))?;
    Ok(key)
}

pub fn seal(source: &[u8], key_file: &[u8], dialect: u16) -> Result<Vec<u8>, String> {
    if source.len() > MAX_SOURCE {
        return Err("source exceeds 1 MiB".into());
    }
    let mut header = [0u8; HEADER];
    header[..8].copy_from_slice(MAGIC);
    OsRng
        .try_fill_bytes(&mut header[8..48])
        .map_err(|e| format!("system random generator failed: {e}"))?;
    header[48..56].copy_from_slice(&(source.len() as u64).to_le_bytes());
    let key = derive(key_file, dialect, &header[8..24])?;
    let cipher = XChaCha20Poly1305::new((&*key).into());
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&header[24..48]),
            Payload {
                msg: source,
                aad: &header,
            },
        )
        .map_err(|_| "source encryption failed".to_owned())?;
    let mut result = Vec::with_capacity(HEADER + ciphertext.len());
    result.extend_from_slice(&header);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

pub fn open(container: &[u8], key_file: &[u8], dialect: u16) -> Result<Zeroizing<Vec<u8>>, String> {
    if container.len() < OVERHEAD
        || container.len() > MAX_SOURCE + OVERHEAD
        || !is_protected(container)
    {
        return Err("invalid or unsupported protected source format (expected AEGIS02)".into());
    }
    let mut size = [0; 8];
    size.copy_from_slice(&container[48..56]);
    let size = u64::from_le_bytes(size);
    if size > MAX_SOURCE as u64 || container.len() != size as usize + OVERHEAD {
        return Err("invalid protected source size".into());
    }
    let key = derive(key_file, dialect, &container[8..24])?;
    let cipher = XChaCha20Poly1305::new((&*key).into());
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&container[24..48]),
            Payload {
                msg: &container[HEADER..],
                aad: &container[..HEADER],
            },
        )
        .map_err(|_| "wrong key/dialect or corrupted source".to_owned())?;
    Ok(Zeroizing::new(plaintext))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn randomized_authenticated_roundtrip() {
        let key = generate_key().expect("random key");
        let source = b"fn main() { return 0; }";
        let a = seal(source, &*key, 123).expect("seal");
        let b = seal(source, &*key, 123).expect("seal");
        assert_ne!(a, b);
        assert_eq!(&**open(&a, &*key, 123).expect("open"), source);
        assert!(open(&a, &*key, 124).is_err());
        for at in [8, 24, HEADER, a.len() - 1] {
            let mut bad = a.clone();
            bad[at] ^= 1;
            assert!(open(&bad, &*key, 123).is_err());
        }
    }
    #[test]
    fn reject_invalid_lengths_before_kdf() {
        assert!(open(b"AEG0", &[0; 32], 0).is_err());
        assert!(seal(b"", &[0; 4], 0).is_err());
        assert!(seal(b"", &[0; 32], 4096).is_err());
    }
}
