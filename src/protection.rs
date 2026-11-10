//! Proteção por usuário do Windows. O banco nunca recebe JSON documental em claro.
//!
//! Off Windows (the phone companion) there is no per-user sealing in the
//! operating system: the host application hands over a 256-bit key once
//! (`set_platform_key`; Android keeps it wrapped by the Keystore) and every
//! payload is sealed with AES-256-GCM under a different prefix, so data
//! sealed one way is never taken for the other.
use anyhow::{bail, Context, Result};

#[cfg(windows)]
const PREFIX: &[u8] = b"PONTE-DPAPI1\0";
#[cfg(not(windows))]
const PREFIX: &[u8] = b"PONTE-AESK1\0";

// Compiled everywhere so its test runs on the development machine too; only
// builds off Windows use it.
#[cfg_attr(windows, allow(dead_code))]
mod keyed {
    use super::*;
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
    use ring::rand::{SecureRandom, SystemRandom};
    use std::sync::OnceLock;

    static KEY: OnceLock<LessSafeKey> = OnceLock::new();
    /// Binds every sealed payload to this purpose, as the DPAPI entropy does.
    const PURPOSE: &[u8] = b"ponteproj-document-content-v1";

    pub fn set_key(key: &[u8]) -> Result<()> {
        anyhow::ensure!(key.len() == 32, "A chave do acervo precisa de 32 bytes.");
        let unbound = UnboundKey::new(&AES_256_GCM, key)
            .map_err(|_| anyhow::anyhow!("Chave do acervo inválida."))?;
        // A second call with another key would make the library unreadable
        // halfway through a session: it is refused.
        if KEY.set(LessSafeKey::new(unbound)).is_err() {
            bail!("A chave do acervo já foi definida nesta sessão.");
        }
        Ok(())
    }

    fn key() -> Result<&'static LessSafeKey> {
        KEY.get()
            .context("A chave do acervo ainda não foi entregue pelo aplicativo.")
    }

    pub fn transform(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>> {
        let key = key()?;
        if encrypt {
            let mut nonce = [0u8; NONCE_LEN];
            SystemRandom::new()
                .fill(&mut nonce)
                .map_err(|_| anyhow::anyhow!("O Windows não forneceu os dados aleatórios necessários para proteger o acervo."))?;
            let mut out = bytes.to_vec();
            key.seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(PURPOSE),
                &mut out,
            )
            .map_err(|_| anyhow::anyhow!("Não foi possível proteger os dados."))?;
            let mut sealed = nonce.to_vec();
            sealed.extend(out);
            Ok(sealed)
        } else {
            anyhow::ensure!(bytes.len() > NONCE_LEN, "Dados protegidos incompletos.");
            let (nonce, body) = bytes.split_at(NONCE_LEN);
            let nonce = Nonce::try_assume_unique_for_key(nonce)
                .map_err(|_| anyhow::anyhow!("Dados protegidos inválidos."))?;
            let mut body = body.to_vec();
            let plain = key
                .open_in_place(nonce, Aad::from(PURPOSE), &mut body)
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Não foi possível abrir os dados (chave diferente ou dados alterados)."
                    )
                })?;
            Ok(plain.to_vec())
        }
    }
}

/// Off Windows: the 256-bit key that seals this library, handed over once
/// by the application before the library is opened.
#[cfg(not(windows))]
use keyed as platform;

#[cfg(not(windows))]
pub fn set_platform_key(key: &[u8]) -> Result<()> {
    keyed::set_key(key)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub fn transform(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>> {
        let input = Blob {
            len: bytes
                .len()
                .try_into()
                .context("Conteúdo grande demais para ser protegido")?,
            data: bytes.as_ptr() as *mut u8,
        };
        let purpose = b"ponteproj-document-content-v1";
        let entropy = Blob {
            len: purpose.len() as u32,
            data: purpose.as_ptr() as *mut u8,
        };
        let mut output = Blob {
            len: 0,
            data: ptr::null_mut(),
        };
        let ok = unsafe {
            if encrypt {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    &entropy,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    1,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    ptr::null_mut(),
                    &entropy,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    1,
                    &mut output,
                )
            }
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error())
                .context("Não foi possível proteger ou abrir os dados desta conta do Windows");
        }
        if output.data.is_null() {
            bail!("O Windows devolveu dados protegidos inválidos");
        }
        let result =
            unsafe { std::slice::from_raw_parts(output.data, output.len as usize).to_vec() };
        unsafe {
            if !encrypt {
                std::ptr::write_bytes(output.data, 0, output.len as usize);
            }
            LocalFree(output.data as *mut c_void);
        }
        Ok(result)
    }
}

pub fn seal(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut protected = PREFIX.to_vec();
    protected.extend(platform::transform(bytes, true)?);
    Ok(protected)
}

pub fn unseal(bytes: &[u8]) -> Result<Vec<u8>> {
    if !bytes.starts_with(PREFIX) {
        bail!("Formato de proteção desconhecido; os dados não foram abertos");
    }
    platform::transform(&bytes[PREFIX.len()..], false)
}

#[cfg(test)]
mod keyed_tests {
    use super::keyed;
    // The sealing the phone uses, tested here: the key once, a fresh nonce
    // each time, tampering refused.
    #[test]
    fn seals_with_the_given_key_and_rejects_tampering() {
        assert!(keyed::set_key(&[7u8; 31]).is_err());
        keyed::set_key(&[7u8; 32]).unwrap();
        assert!(keyed::set_key(&[8u8; 32]).is_err());
        let secret = "Ofício de exemplo: conteúdo reservado".as_bytes();
        let encoded = keyed::transform(secret, true).unwrap();
        assert!(!encoded.windows(secret.len()).any(|w| w == secret));
        assert_eq!(keyed::transform(&encoded, false).unwrap(), secret);
        assert_ne!(
            keyed::transform(secret, true).unwrap(),
            encoded,
            "fresh nonce each time"
        );
        let mut corrupt = encoded;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(keyed::transform(&corrupt, false).is_err());
        assert!(keyed::transform(&[1, 2, 3], false).is_err());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn protects_document_and_rejects_plaintext() {
        let secret = "Ofício de exemplo: conteúdo reservado".as_bytes();
        let encoded = seal(secret).unwrap();
        assert!(!encoded.windows(secret.len()).any(|w| w == secret));
        assert_eq!(unseal(&encoded).unwrap(), secret);
        assert!(unseal(secret).is_err());
        let mut corrupt = encoded;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(unseal(&corrupt).is_err());
    }
}
