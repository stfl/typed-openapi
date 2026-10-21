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
//! magic never matches it. `0x89` opens a length that the next byte, `T`, ends
//! at 10 761, and a URL that long would open with `OAMDL` and a newline, which
//! no URL holds.

use super::{
    Document, DocumentError, Effect, Field, Gate, Operation, Shape, Unmet, first_collision,
    gate_on_read, spends, spends_command, unmet,
};
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
    /// Hold a decoded reduction to the rules of a load that it can break.
    ///
    /// A load writes the model and checks it while it does, and a blob arrives
    /// with those rules not applied. The rules applied again are the
    /// confirmation word, the names and the flags of each operation and the
    /// gates that stand on it, the effect a method allows, each `requires`, and
    /// the names two operations share. A failure names the operation, or both of
    /// two, except for the document's own confirmation word, which belongs to
    /// none.
    ///
    /// A rule joins them when a value in the blob can break it and the break
    /// does one of two things: the binary panics, in clap when it builds the
    /// command tree in a debug build (in a release build one declaration
    /// silently takes the place of another), or a request that the gate holds
    /// back is sent on sight. A rule whose break costs only the run that meets it
    /// stays out: a `pattern` that no engine runs refuses every value of its flag
    /// when the flag is parsed, and a start-up that compiled every pattern would
    /// pay for that on every run. The spelling of a flag and each `requires` are
    /// repeated too, though a blob that breaks either builds a command line: a
    /// flag nobody can type and a `requires` nothing meets are a model that no
    /// load wrote, and the one message names the operation.
    pub(super) fn check(&self) -> Result<(), DocumentError> {
        if !confirms(&self.commit) {
            return Err(DocumentError::Confirmation {
                word: self.commit.clone(),
            });
        }
        self.ops.iter().try_for_each(Operation::check)?;
        match first_collision(&self.ops) {
            None => Ok(()),
            Some((first, second)) => Err(DocumentError::DuplicateCommand {
                group: first.group.clone(),
                command: first.command.clone(),
                first: first.id.clone(),
                second: second.id.clone(),
            }),
        }
    }
}

impl Operation {
    fn check(&self) -> Result<(), DocumentError> {
        self.check_names()?;
        self.check_effect()?;
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

    /// Neither name is one clap declares itself. Each is spelled already,
    /// because a `CommandName` is read through the spelling rule.
    fn check_names(&self) -> Result<(), DocumentError> {
        [("group", &self.group), ("command", &self.command)]
            .into_iter()
            .find(|(_, name)| spends_command(name.as_str()))
            .map_or(Ok(()), |(what, name)| {
                Err(DocumentError::NameSpent {
                    op: self.id.clone(),
                    what,
                    name: name.as_str().to_owned(),
                })
            })
    }

    /// A method that writes is not held as a read, which is sent on sight and
    /// declares no confirmation. The converse is no fault: the document's
    /// `x-cli-writes` makes a `GET` a write, and the blob does not say whether
    /// it did.
    fn check_effect(&self) -> Result<(), DocumentError> {
        match (self.effect, Effect::of_method(&self.method)) {
            (Effect::Read, Effect::Write) => Err(DocumentError::WriteAsRead {
                op: self.id.clone(),
                method: self.method.as_str().to_owned(),
            }),
            _ => Ok(()),
        }
    }

    /// A gate stands on a write: a read is sent on sight, and a load refuses
    /// the document that names a gate on one.
    fn check_gates(&self) -> Result<(), DocumentError> {
        gate_on_read(self.effect, &self.gates).map_or(Ok(()), |gate| {
            Err(DocumentError::GatedRead {
                op: self.id.clone(),
                gate: gate.as_str().to_owned(),
            })
        })
    }

    /// Every flag the operation declares passes the spelling a load gives it,
    /// is none that every subcommand spends already, and is declared once.
    ///
    /// The declarations are the confirmation word of a write, the gates, the
    /// parameters that have a flag and the properties of a body. A load keeps
    /// them apart in one namespace. A debug build of the binary panics on a name
    /// given twice when it builds the command tree, and a release build lets one
    /// declaration take the place of the other. A read declares no confirmation,
    /// so a parameter of a read may be spelled like it.
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
            if !is_spelled(flag) {
                return Err(DocumentError::Flag {
                    op: self.id.clone(),
                    flag: flag.to_owned(),
                });
            }
            if spends(flag) {
                return Err(DocumentError::FlagSpent {
                    op: self.id.clone(),
                    flag: flag.to_owned(),
                });
            }
            if declared.contains(&flag) {
                return Err(DocumentError::FlagTwice {
                    op: self.id.clone(),
                    flag: flag.to_owned(),
                });
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
