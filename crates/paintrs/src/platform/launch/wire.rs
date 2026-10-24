//! Bounded, versioned local launch protocol. Native OS path units are never decoded as UTF-8.
use std::{io, path::PathBuf};
pub const LIMIT: usize = 1024 * 1024;
pub const MAX_PATHS: usize = 256;
#[derive(Debug, PartialEq)]
pub struct Batch {
    pub id: [u8; 16],
    pub paths: Vec<PathBuf>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid/oversized native launch batch",
    )
}
pub fn encode(batch: &Batch) -> io::Result<Vec<u8>> {
    if batch.paths.len() > MAX_PATHS {
        return Err(invalid());
    }
    let mut bytes = b"PRS1".to_vec();
    bytes.extend(batch.id);
    bytes.extend((batch.paths.len() as u32).to_le_bytes());
    for path in &batch.paths {
        if !path.is_absolute() {
            return Err(invalid());
        }
        #[cfg(unix)]
        let units = {
            use std::os::unix::ffi::OsStrExt;
            path.as_os_str().as_bytes().to_vec()
        };
        #[cfg(windows)]
        let units = {
            use std::os::windows::ffi::OsStrExt;
            path.as_os_str()
                .encode_wide()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        if units.is_empty() || units.len() + bytes.len() + 4 > LIMIT {
            return Err(invalid());
        }
        bytes.extend((units.len() as u32).to_le_bytes());
        bytes.extend(units);
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> io::Result<Batch> {
    if bytes.len() < 24 || bytes.len() > LIMIT || &bytes[..4] != b"PRS1" {
        return Err(invalid());
    }
    let id = bytes[4..20].try_into().map_err(|_| invalid())?;
    let count = u32::from_le_bytes(bytes[20..24].try_into().map_err(|_| invalid())?) as usize;
    if count > MAX_PATHS {
        return Err(invalid());
    }
    let mut rest = &bytes[24..];
    let mut paths = Vec::with_capacity(count);
    for _ in 0..count {
        let len = rest.get(..4).ok_or_else(invalid)?;
        let len = u32::from_le_bytes(len.try_into().map_err(|_| invalid())?) as usize;
        rest = &rest[4..];
        let raw = rest.get(..len).ok_or_else(invalid)?;
        if raw.is_empty() {
            return Err(invalid());
        }
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStringExt;
            if raw.contains(&0) {
                return Err(invalid());
            }
            PathBuf::from(std::ffi::OsString::from_vec(raw.to_vec()))
        };
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStringExt;
            if raw.len() % 2 != 0 {
                return Err(invalid());
            }
            let units: Vec<_> = raw
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            if units.contains(&0) {
                return Err(invalid());
            }
            PathBuf::from(std::ffi::OsString::from_wide(&units))
        };
        if !path.is_absolute() {
            return Err(invalid());
        }
        paths.push(path);
        rest = &rest[len..];
    }
    if !rest.is_empty() {
        return Err(invalid());
    }
    Ok(Batch { id, paths })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_keep_native_bytes_and_order_and_never_truncate() {
        #[cfg(unix)]
        let special = {
            use std::os::unix::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/a \xff.png".to_vec()))
        };
        #[cfg(windows)]
        let special = {
            use std::os::windows::ffi::OsStringExt;
            PathBuf::from(std::ffi::OsString::from_wide(&[
                67, 58, 92, 0xd800, 46, 112, 110, 103,
            ]))
        };
        let b = Batch {
            id: [4; 16],
            paths: vec![special.clone(), special],
        };
        assert_eq!(decode(&encode(&b).unwrap()).unwrap(), b);
        let huge = Batch {
            id: [0; 16],
            paths: vec![b.paths[0].clone(); MAX_PATHS + 1],
        };
        assert!(encode(&huge).is_err());
        for n in 0..24 {
            assert!(decode(&vec![0; n]).is_err());
        }
        assert!(decode(&vec![0; LIMIT + 1]).is_err());
    }
    #[test]
    fn empty_launch_is_activation_and_trailing_bytes_are_rejected() {
        let mut bytes = b"PRS1".to_vec();
        bytes.extend([7u8; 16]);
        bytes.extend(0u32.to_le_bytes());
        assert_eq!(
            decode(&bytes).unwrap(),
            Batch {
                id: [7; 16],
                paths: vec![]
            }
        );
        bytes.push(1);
        assert!(decode(&bytes).is_err());
    }
}
