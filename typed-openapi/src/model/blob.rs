//! What a model blob opens with, and what a decoded reduction is held to.
//!
//! A blob is a stamp and then the `postcard` encoding of a [`Document`]. The
//! stamp is a magic of eight bytes, the number of the layout the model was
//! written in as four little-endian bytes, and the version of typed-openapi that
//! wrote it as four little-endian bytes of length and then that many bytes of
//! UTF-8. Its format never changes, because a build that cannot decode a blob
//! reads the stamp to say why: a format that moved with the layout would be
//! unreadable by exactly the builds that need it.
//!
//! The magic opens with a byte that is not ASCII, so no text file starts with
//! it, and no blob of 0.0.2, which opened with the length of a URL, does either.

use super::{Document, DocumentError, Field, Operation, Shape, Unmet, spends, unmet};
use crate::names::is_spelled;

/// The first eight bytes of every stamped blob: a byte above ASCII, then
/// `TOAMDL` for the typed-openapi model, then a newline.
const MAGIC: [u8; 8] = *b"\x89TOAMDL\n";

/// What a stamp says.
#[derive(Debug)]
pub(super) struct Stamp {
    /// The number of the layout the blob was written in.
    pub(super) layout: u32,
    /// The version of typed-openapi that wrote it. Text for a message and
    /// nothing else, so bytes that are not UTF-8 are shown as replacement
    /// characters and do not hide the layout.
    pub(super) version: String,
}

/// The stamp of a blob written in `layout` by typed-openapi `version`, as the
/// start of the bytes the encoding is appended to.
pub(super) fn stamp(layout: u32, version: &str) -> Vec<u8> {
    // A version is a handful of bytes; the length saturates only for a string
    // no crate version is.
    let length = u32::try_from(version.len()).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(MAGIC.len() + 8 + version.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&layout.to_le_bytes());
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(version.as_bytes());
    bytes
}

/// The stamp a blob opens with and the bytes after it, or `None` where the
/// bytes are too short for a stamp or do not open with the magic.
pub(super) fn read(blob: &[u8]) -> Option<(Stamp, &[u8])> {
    let (magic, rest) = blob.split_first_chunk::<8>()?;
    if *magic != MAGIC {
        return None;
    }
    let (layout, rest) = rest.split_first_chunk::<4>()?;
    let (length, rest) = rest.split_first_chunk::<4>()?;
    let length = usize::try_from(u32::from_le_bytes(*length)).ok()?;
    let (version, encoding) = rest.split_at_checked(length)?;
    let stamp = Stamp {
        layout: u32::from_le_bytes(*layout),
        version: String::from_utf8_lossy(version).into_owned(),
    };
    Some((stamp, encoding))
}

/// Whether `word` can be the confirmation: the rule `Loading::commit` applies,
/// which is that a person can type it as a flag and no subcommand declares it
/// already.
fn confirms(word: &str) -> bool {
    is_spelled(word) && !spends(word)
}

impl Document {
    /// Hold a decoded reduction to what a load keeps.
    ///
    /// A load writes the confirmation word, every flag and every `requires` of
    /// the model, and checks each while it does. A blob arrives with the rules
    /// not applied, and a flag that clap cannot declare stops a binary when its
    /// command tree is built. So the rules a load applies and a blob could
    /// break are applied again here, and a failure names the operation.
    pub(super) fn check(&self) -> Result<(), DocumentError> {
        if !confirms(&self.commit) {
            return Err(DocumentError::Confirmation {
                word: self.commit.clone(),
            });
        }
        self.ops.iter().try_for_each(Operation::check)
    }
}

impl Operation {
    fn check(&self) -> Result<(), DocumentError> {
        if !confirms(&self.commit) {
            return Err(DocumentError::OperationConfirmation {
                op: self.id.clone(),
                word: self.commit.clone(),
            });
        }
        let flags = self
            .params
            .iter()
            .filter_map(|param| match &param.shape {
                Shape::Flag { flag, .. } => Some(flag.as_str()),
                Shape::Unreachable(_) => None,
            })
            .chain(self.body.fields().iter().map(Field::flag));
        for flag in flags {
            if !is_spelled(flag) {
                return Err(DocumentError::Flag {
                    op: self.id.clone(),
                    flag: flag.to_owned(),
                });
            }
        }
        match unmet(&self.params) {
            None => Ok(()),
            Some(Unmet::Unknown { name, requires }) => Err(DocumentError::RequiresUnknown {
                op: self.id.clone(),
                name: name.to_owned(),
                requires: requires.to_owned(),
            }),
            Some(Unmet::Unreachable {
                name,
                requires,
                why,
            }) => Err(DocumentError::RequiresUnreachable {
                op: self.id.clone(),
                name: name.to_owned(),
                requires: requires.to_owned(),
                why: why.clone(),
            }),
        }
    }
}
