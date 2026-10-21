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
//! it. A blob of 0.0.2 opened with the length of the server's URL, and the whole
//! magic never matches it: a URL of 137 bytes opens with `0x89`, and the next
//! byte of the magic is a `T` that would sit inside the URL, followed by a
//! newline that no URL holds.

use super::{Document, DocumentError, Effect, Field, Gate, Operation, Shape, Unmet, spends, unmet};
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
    /// break are applied again here: the confirmation word, the flags of each
    /// operation and the gates that stand on it, and each `requires`. A failure
    /// names the operation, except for the document's own confirmation word,
    /// which belongs to none.
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
        self.check_gates()?;
        self.check_flags()?;
        self.check_requires()
    }

    /// A gate stands on a write: a read is sent on sight, and a load refuses
    /// the document that names a gate on one.
    fn check_gates(&self) -> Result<(), DocumentError> {
        match (self.effect, self.gates.first()) {
            (Effect::Read, Some(gate)) => Err(DocumentError::GatedRead {
                op: self.id.clone(),
                gate: gate.as_str().to_owned(),
            }),
            _ => Ok(()),
        }
    }

    /// Every flag the operation declares passes the spelling a load gives it,
    /// is none that every subcommand declares already, and is declared once.
    ///
    /// The declarations are the confirmation word of a write, the gates, the
    /// parameters that have a flag and the properties of a body. A load keeps
    /// them apart in one namespace, and a name given twice stops clap when the
    /// command tree is built. A read declares no confirmation, so a parameter of
    /// a read may be spelled like it.
    fn check_flags(&self) -> Result<(), DocumentError> {
        let confirmation = (self.effect == Effect::Write).then_some(self.commit.as_str());
        let mut declared: Vec<&str> = confirmation.into_iter().collect();
        let flags = self
            .gates
            .iter()
            .map(Gate::as_str)
            .chain(self.params.iter().filter_map(|param| match &param.shape {
                Shape::Flag { flag, .. } => Some(flag.as_str()),
                Shape::Unreachable(_) => None,
            }))
            .chain(self.body.fields().iter().map(Field::flag));
        for flag in flags {
            let (op, owned) = (self.id.clone(), flag.to_owned());
            if !is_spelled(flag) {
                return Err(DocumentError::Flag { op, flag: owned });
            }
            if spends(flag) {
                return Err(DocumentError::FlagSpent { op, flag: owned });
            }
            if declared.contains(&flag) {
                return Err(DocumentError::FlagTwice { op, flag: owned });
            }
            declared.push(flag);
        }
        Ok(())
    }

    fn check_requires(&self) -> Result<(), DocumentError> {
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
