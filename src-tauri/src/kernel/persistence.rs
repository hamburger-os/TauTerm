//! Atomic replacement helpers for TauTerm-owned text/JSON state.
//!
//! Writes are staged in the destination directory and committed atomically so a process crash or
//! interrupted process write preserves a complete old/new file instead of leaving truncated JSON.

use atomic_write_file::AtomicWriteFile;
use std::io::{self, Write};
use std::path::Path;

pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(contents)?;
    file.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_complete_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"{\"version\":1}").unwrap();

        atomic_write(&path, b"{\"version\":2,\"ready\":true}").unwrap();

        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "{\"version\":2,\"ready\":true}"
        );
    }
}
