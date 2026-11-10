//! The vendor's document asked for its version, and the overlaid document read
//! into the OpenAPI object model.
//!
//! The model refuses a document with a message and no place: `openapiv3` reads
//! `paths` through a deserialiser that drops the path inside it, so serde says
//! "invalid type: string \"yes\", expected a boolean" for a document of 35 000
//! lines and stops at `paths`. [`model_of`] reads the document once, and when the
//! model refuses it inside `paths`, reads it again one path item at a time and
//! names the first node that fails.

use openapiv3::{OpenAPI, PathItem, ReferenceOr};
use serde::Deserialize;
use serde_json::Value;
use serde_path_to_error::{Path, Segment};

use crate::model::LoadError;

/// The first node of a document that the OpenAPI model refuses.
#[derive(Debug)]
pub(crate) struct Misfit {
    /// Where the model stopped, from the root of the document, spelled as
    /// `serde_path_to_error` spells a place: `info.title`,
    /// `components.schemas.Voucher`. Inside `paths` a path item is
    /// `paths[/vouchers]`, and the node inside it follows:
    /// `paths[/vouchers].get.deprecated`. `.` is the document itself.
    pub(crate) at: String,
    /// `serde_json`'s reason, as the model gave it.
    pub(crate) source: serde_json::Error,
}

/// Refuse a document whose `openapi` is a version this crate does not read.
///
/// This crate reads OpenAPI 3.0. 3.1 spells some rules differently, which the
/// model refuses with no place, and means others that 3.0 does not have, which
/// the model drops in silence: a `$ref` keeps the `maximum` beside it in 3.1, and
/// `const` is a rule. So the version is asked before anything else reads the
/// document, and of the document as the vendor wrote it: an Overlay cannot
/// change `openapi` (`overlay::apply` refuses it), so no layer relabels a 3.1
/// document as 3.0 and has its rules dropped.
///
/// A document with no `openapi` key has no version to name, and the model
/// refuses it at `.`. So does a value that is not text, at `openapi`.
pub(crate) fn require_3_0(document: &Value) -> Result<(), LoadError> {
    match document.get("openapi").and_then(Value::as_str) {
        Some(version) if !is_openapi_3_0(version) => Err(LoadError::Version {
            version: version.to_owned(),
        }),
        _ => Ok(()),
    }
}

/// `3.0`, and `3.0.` with a patch number after it, which a pre-release label may
/// follow as in `3.0.0-rc1`: every version of 3.0 the specification published.
fn is_openapi_3_0(version: &str) -> bool {
    let Some(patch) = version.strip_prefix("3.0.") else {
        return version == "3.0";
    };
    let (number, label) = match patch.split_once('-') {
        Some((number, label)) => (number, Some(label)),
        None => (patch, None),
    };
    !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && label.is_none_or(|label| !label.is_empty())
}

/// Read a document into the OpenAPI model, and say where it stopped if the model
/// refuses it.
pub(crate) fn model_of(document: &Value) -> Result<OpenAPI, Misfit> {
    serde_path_to_error::deserialize(document).map_err(|refused| {
        if is_paths(refused.path())
            && let Some(inside) = inside_paths(document)
        {
            return inside;
        }
        Misfit {
            at: refused.path().to_string(),
            source: refused.into_inner(),
        }
    })
}

/// Whether the model stopped at `paths` itself rather than inside another key.
fn is_paths(path: &Path) -> bool {
    let mut segments = path.iter();
    matches!(
        (segments.next(), segments.next()),
        (Some(Segment::Map { key }), None) if key == "paths"
    )
}

/// The first path item of `paths` that the model refuses, with the node inside
/// it, or `None` where every item reads alone and the fault is in how `paths`
/// itself is built.
///
/// The model reads as path items the keys that start with `/`, and skips the
/// rest, so those are the only keys tried: an extension, or a key with no slash,
/// loads whatever it holds.
fn inside_paths(document: &Value) -> Option<Misfit> {
    let paths = document.get("paths")?.as_object()?;
    paths
        .iter()
        .filter(|(path, _)| path.starts_with('/'))
        .find_map(|(path, item)| {
            let refused = read::<ReferenceOr<PathItem>>(item).err()?;
            // `ReferenceOr` reads a node it refuses again from a buffer, which
            // drops the place inside it. The item read as what it is keeps the
            // place, and reads its keys in the order the document lists them,
            // as the model does.
            let refused = read::<PathItem>(item).err().unwrap_or(refused);
            Some(Misfit {
                at: place(&format!("paths[{path}]"), refused.path()),
                source: refused.into_inner(),
            })
        })
}

/// One node on its own, with the place inside it kept.
fn read<'v, T: Deserialize<'v>>(
    node: &'v Value,
) -> Result<T, serde_path_to_error::Error<serde_json::Error>> {
    serde_path_to_error::deserialize(node)
}

/// `inside` below `node`, where `inside` is not the node itself.
fn place(node: &str, inside: &Path) -> String {
    match inside.iter().next() {
        Some(_) => format!("{node}.{inside}"),
        None => node.to_owned(),
    }
}
