//! Proteção por usuário do Windows. O banco nunca recebe JSON documental em claro.
use anyhow::{bail, Context, Result};

const PREFIX: &[u8] = b"PONTE-DPAPI1\0";

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
                .context("Conteúdo grande demais para proteção")?,
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
                .context("Não foi possível proteger/abrir os dados deste perfil Windows");
        }
        if output.data.is_null() {
            bail!("A proteção do Windows retornou dados inválidos");
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
    #[cfg(windows)]
    {
        let mut protected = PREFIX.to_vec();
        protected.extend(platform::transform(bytes, true)?);
        Ok(protected)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        bail!("A proteção DPAPI desta versão requer Windows")
    }
}

pub fn unseal(bytes: &[u8]) -> Result<Vec<u8>> {
    if !bytes.starts_with(PREFIX) {
        bail!("Formato de proteção desconhecido; não interpretar dados como texto em claro");
    }
    #[cfg(windows)]
    {
        platform::transform(&bytes[PREFIX.len()..], false)
    }
    #[cfg(not(windows))]
    {
        bail!("A proteção DPAPI desta versão requer Windows")
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
