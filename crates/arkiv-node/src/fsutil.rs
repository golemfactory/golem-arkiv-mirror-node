//! Durable-filesystem steps of the genesis tooling. Callers add their own
//! context to the I/O error.

use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
};

/// Writes and syncs a file that must not exist yet.
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Syncs a directory, making the names created or renamed in it durable.
pub(crate) fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

/// The directory holding `path`; the current directory for a bare name.
pub(crate) fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Renames without replacing an existing destination.
pub(crate) fn rename_noreplace(from: &Path, to: &Path) -> rustix::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
}
