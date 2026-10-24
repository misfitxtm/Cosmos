// SPDX-License-Identifier: GPL-3.0-or-later
//! Writes that never leave a half-written file behind.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Writes `data` to a temp file beside `dest`, then renames it over `dest`.
/// If `dest` is a symlink the link is replaced, never written through.
/// A regular file that is replaced keeps its permissions, so a private file does not become world-readable.
pub fn write_atomic(dest: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = dest
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;
    let dir = dest.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    // symlink_metadata: a link is replaced, so it must not lend its target's permissions
    let keep = fs::symlink_metadata(dest)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.permissions());
    let result = (|| {
        // create_new refuses an existing file or symlink, so a planted link is never followed
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&tmp)?;
        f.write_all(data)?;
        if let Some(perms) = keep {
            f.set_permissions(perms)?;
        }
        f.sync_all()?;
        fs::rename(&tmp, dest)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"two");
        let leftovers: Vec<_> = fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "temp file left behind");
    }

    #[test]
    fn replaces_a_symlink_instead_of_writing_through_it() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("target.txt");
        fs::write(&target, b"precious").unwrap();
        let link = d.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"precious");
        assert_eq!(fs::read(&link).unwrap(), b"new");
        assert!(
            !fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn a_replaced_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        for want in [0o600, 0o640, 0o755] {
            let f = d.path().join(format!("m{want:o}"));
            fs::write(&f, b"old").unwrap();
            fs::set_permissions(&f, fs::Permissions::from_mode(want)).unwrap();
            write_atomic(&f, b"new").unwrap();
            assert_eq!(fs::read(&f).unwrap(), b"new");
            assert_eq!(mode(&f), want, "{want:o}");
        }
    }

    #[test]
    fn a_replaced_symlink_does_not_inherit_its_targets_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("target");
        fs::write(&target, b"x").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let link = d.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let fresh = d.path().join("fresh");
        write_atomic(&fresh, b"y").unwrap();
        write_atomic(&link, b"z").unwrap();
        assert_eq!(mode(&link), mode(&fresh)); // same as any new file, not the target's 0600
        assert_eq!(mode(&target), 0o600);
    }

    #[test]
    fn failure_leaves_no_temp_file() {
        let d = tempfile::tempdir().unwrap();
        // The destination is a non-empty directory, so the final rename fails
        let dest = d.path().join("dir");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("x"), b"x").unwrap();
        assert!(write_atomic(&dest, b"data").is_err());
        let names: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }
}
