//! The overlaid document, read into the OpenAPI object model.
//!
//! The model refuses a document with a message and no place: `openapiv3` reads
//! `paths` through a deserialiser that drops the path inside it, so serde says
//! "invalid type: string \"yes\", expected a boolean" for a document of 35 000
//! lines and stops at `paths`. [`model_of`] reads the document once, and when the
//! model refuses it inside `paths`, reads it again a path item and an operation
//! at a time and names the first node that fails.

use openapiv3::{OpenAPI, Operation, PathItem, ReferenceOr};
use serde::Deserialize;
use serde_json::Value;
use serde_path_to_error::{Path, Segment};

/// The methods a path item holds an operation under, in the order the model
/// reads them.
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// The first node of a document that the OpenAPI model refuses.
#[derive(Debug)]
pub(crate) struct Misfit {
    /// Where the model stopped, from the root of the document, spelled as
    /// `serde_path_to_error` spells a place: `info.title`,
    /// `components.schemas.Voucher`. Inside `paths` a path item is
    /// `paths[/vouchers]`, and an operation under it is `paths[/vouchers].get`.
    /// `.` is the document itself.
    pub(crate) at: String,
    /// `serde_json`'s reason, as the model gave it.
    pub(crate) source: serde_json::Error,
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

/// The first path item of `paths` that the model refuses, and inside it the
/// first operation, or `None` where every item reads alone and the fault is in
/// how `paths` itself is built.
fn inside_paths(document: &Value) -> Option<Misfit> {
    let paths = document.get("paths")?.as_object()?;
    paths.iter().find_map(|(path, item)| {
        let refused = read::<ReferenceOr<PathItem>>(item).err()?;
        let at = format!("paths[{path}]");
        let operation = METHODS.into_iter().find_map(|method| {
            let refused = read::<Operation>(item.get(method)?).err()?;
            Some((format!("{at}.{method}"), refused))
        });
        let (at, refused) = operation.unwrap_or((at, refused));
        Some(Misfit {
            at: place(&at, refused.path()),
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
