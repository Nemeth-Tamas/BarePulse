use std::{
    ffi::c_void,
    io,
    ptr::{null, null_mut},
};

use windows_sys::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptHash, BCryptOpenAlgorithmProvider,
};

const SHA256_BYTES: usize = 32;

struct AlgorithmHandle(*mut c_void);

impl Drop for AlgorithmHandle {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }

        // SAFETY:
        // self.0 is an algorithm-provider handle returned by
        // BCryptOpenAlgorithmProvider and owned by this wrapper.
        let _ = unsafe { BCryptCloseAlgorithmProvider(self.0, 0) };
    }
}

pub(crate) fn sha256_hex(data: &[u8]) -> io::Result<String> {
    if data.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SHA-256 input exceeds BCrypt's single-call size limit",
        ));
    }

    let algorithm_name = "SHA256".encode_utf16().chain(Some(0)).collect::<Vec<_>>();

    let mut algorithm: *mut c_void = null_mut();

    // SAFETY:
    // algorithm points to writable handle storage and algorithm_name is a
    // valid null-terminated UTF-16 BCrypt algorithm identifier.
    let status =
        unsafe { BCryptOpenAlgorithmProvider(&mut algorithm, algorithm_name.as_ptr(), null(), 0) };

    require_nt_success(status, "BCryptOpenAlgorithmProvider")?;

    let algorithm = AlgorithmHandle(algorithm);

    let mut digest = [0u8; SHA256_BYTES];

    // SAFETY:
    // algorithm is a valid SHA-256 provider handle. data and digest point
    // to valid buffers of the supplied lengths. A null secret selects an
    // ordinary unkeyed hash.
    let status = unsafe {
        BCryptHash(
            algorithm.0,
            null(),
            0,
            data.as_ptr(),
            data.len() as u32,
            digest.as_mut_ptr(),
            digest.len() as u32,
        )
    };

    require_nt_success(status, "BCryptHash")?;

    Ok(hex_lower(&digest))
}

fn require_nt_success(status: i32, operation: &str) -> io::Result<()> {
    if status >= 0 {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "{operation} failed with NTSTATUS 0x{:08X}",
        status as u32
    )))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut result = String::with_capacity(bytes.len() * 2);

    for &byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0F) as usize] as char);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc").expect("Windows SHA-256 should succeed"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
