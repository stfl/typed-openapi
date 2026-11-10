//! Every file a bless writes, held in memory until each step that can refuse has
//! passed.
//!
//! An adoption commits what a bless writes, and the artefacts are one
//! description of one API: `types.rs` names the types `ops.rs` calls, and the
//! blob is the reduction both were emitted from. A bless that stops half-way and
//! leaves the new `types.rs` beside the old `ops.rs` is a tree that compiles and
//! describes two APIs, and the only trace of it is a diff that nobody reads as
//! half a bless. So nothing is written until the document, every emitter, the
//! blob's round trip and `rustfmt` have all said yes, and the bless that is
//! refused leaves the tree as it found it.
//!
//! The writing is two passes. Each file is written beside its destination
//! first and flushed to the disk, so a full disk or a path that cannot be made
//! stops the bless while every file in the tree is still the one that was
//! committed, and what was written beside is removed. Only when every file is
//! written is each renamed over its destination, which a file system does as
//! one step: a reader sees the old file or the new one and never a part of
//! either. A destination that is a directory is refused in the first pass,
//! where nothing is placed yet. A rename that fails all the same, after another
//! has succeeded, leaves the files that were renamed in place: the pass before
//! it has shown that a file can be written there, so what refuses it is the
//! file system.
//!
//! The renames are one step each and not one step together, and the directories
//! that hold them are not flushed. A power loss between two of them, or before
//! the disk has the directories, leaves some files new and the rest old, and the
//! bless run again writes the rest. Making the directories the first pass needs
//! is not undone: a bless that stops there leaves them, empty. The mode of a
//! destination is not copied to the file that replaces it, and a symlink there
//! becomes a file.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::{fs, io};

use super::GenerateError;

/// The suffix of the file written beside a destination, so that a file left by
/// a bless that was killed names itself.
const BESIDE: &str = ".bless-tmp";

/// What a bless writes: each path, with the bytes that go in it, in the order
/// the bless reports them.
pub(super) struct Artefacts(Vec<(PathBuf, Vec<u8>)>);

impl Artefacts {
    pub(super) fn new() -> Self {
        Self(Vec::new())
    }

    pub(super) fn add(&mut self, path: PathBuf, contents: impl Into<Vec<u8>>) {
        self.0.push((path, contents.into()));
    }

    /// Put every artefact in place and answer with their paths, or put none.
    pub(super) fn write(self) -> Result<Vec<PathBuf>, GenerateError> {
        let mut beside = Beside::default();
        for (path, contents) in &self.0 {
            beside.stage(path, contents)?;
        }
        beside.place()?;
        Ok(self.0.into_iter().map(|(path, _)| path).collect())
    }
}

/// The files written beside their destinations, each with the destination it is
/// bound for.
///
/// Dropping it removes every file that is still beside, so a bless that stops
/// leaves none of its own behind. A removal that fails leaves its file and is not
/// reported, because a drop has nobody to report to. A file that was renamed is
/// not there to remove.
#[derive(Default)]
struct Beside(Vec<(PathBuf, PathBuf)>);

impl Beside {
    /// Write `contents` beside `path`, making the directory it goes in.
    fn stage(&mut self, path: &Path, contents: &[u8]) -> Result<(), GenerateError> {
        let failed = |at: &Path| {
            let path = at.to_path_buf();
            move |source: io::Error| GenerateError::Write { path, source }
        };
        if path.is_dir() {
            return Err(failed(path)(io::Error::new(
                io::ErrorKind::IsADirectory,
                "a directory stands where the file goes",
            )));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(failed(parent))?;
        }
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(BESIDE);
        let beside = path.with_file_name(name);
        // Listed before it is written, because a write that fails half-way
        // leaves a file to remove.
        self.0.push((beside.clone(), path.to_path_buf()));
        fs::File::create(&beside)
            .and_then(|mut file| {
                file.write_all(contents)?;
                file.sync_all()
            })
            .map_err(failed(path))
    }

    /// Rename each file over its destination.
    fn place(&self) -> Result<(), GenerateError> {
        self.0.iter().try_for_each(|(beside, path)| {
            fs::rename(beside, path).map_err(|source| GenerateError::Write {
                path: path.clone(),
                source,
            })
        })
    }
}

impl Drop for Beside {
    fn drop(&mut self) {
        for (beside, _) in &self.0 {
            drop(fs::remove_file(beside));
        }
    }
}
