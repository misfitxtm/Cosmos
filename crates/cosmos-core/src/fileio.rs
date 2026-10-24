// SPDX-License-Identifier: GPL-3.0-or-later
//! The one way Cosmos reads a file it did not write: regular files only, size capped.

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::path::Path;

fn not_a_file() -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, "not a regular file")
}

/// Reads at most `max` bytes. A FIFO, device or directory is refused before it is opened
/// (opening a FIFO blocks until someone writes to it), and a file that grows after the
/// check still cannot get past the cap.
pub fn read_limited(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    if !fs::metadata(path)?.is_file() {
        return Err(not_a_file());
    }
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(not_a_file());
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::new(ErrorKind::FileTooLarge, "file is too large"));
    }
    Ok(bytes)
}

pub fn read_limited_text(path: &Path, max: u64) -> io::Result<String> {
    String::from_utf8(read_limited(path, max)?)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "not valid UTF-8 text"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn reads_a_small_file_and_refuses_a_big_one() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        fs::write(&f, "hello").unwrap();
        assert_eq!(read_limited(&f, 5).unwrap(), b"hello");
        assert_eq!(
            read_limited(&f, 4).unwrap_err().kind(),
            ErrorKind::FileTooLarge
        );
        assert_eq!(read_limited_text(&f, 100).unwrap(), "hello");
    }

    #[test]
    fn non_text_and_missing_files_are_errors() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("bin");
        fs::write(&f, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(
            read_limited_text(&f, 10).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            read_limited(&d.path().join("nope"), 10).unwrap_err().kind(),
            ErrorKind::NotFound
        );
    }

    #[test]
    fn directories_devices_and_fifos_are_refused_without_blocking() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            read_limited(d.path(), 10).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            read_limited(Path::new("/dev/zero"), 10).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        // Opening a FIFO with nobody writing would hang forever; this must return at once
        let fifo = d.path().join("pipe");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            read_limited(&fifo, 10).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_zero_cap_and_a_huge_cap_do_not_overflow() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a");
        fs::write(&f, "x").unwrap();
        assert!(read_limited(&f, 0).is_err());
        assert_eq!(read_limited(&f, u64::MAX).unwrap(), b"x");
    }
}
