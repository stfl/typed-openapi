//! The names that two items of the generated `ops.rs` would share.
//!
//! The wrappers are methods of the handle, and a method is defined once. So are
//! the variants of `OperationId`, and so are the setters of one builder. typify and
//! `syn` accept a file that defines one twice, and the adopter's compiler does not:
//! a bless that wrote it reported success over a crate that does not build. This is
//! the one place that asks, over what the emitter of the wrappers has decided, and
//! it refuses by the two items that share the name ([`DuplicateIdentifier`]).
//!
//! # The three namespaces
//!
//! - The methods of the handle: the wrapper of each operation, the `_builder`
//!   method beside it, and the two methods that the handle has of its own, `call`
//!   and `document`. A handle is the adopter's, so the generator knows the two that
//!   `docs/generating.md` says it has, and no others.
//! - The variants of `OperationId`.
//! - The methods of one operation's builder, which `bon` writes: a setter for each
//!   argument, a second setter named `maybe_…` for each argument that is optional,
//!   and `call`, which ends the builder. Each operation has a builder of its own, so
//!   two operations never share this namespace.
//!
//! A refusal comes in the order the namespaces are asked, and within a namespace in
//! the order of the document, so the first item to claim a name is the one that
//! stands first in the document, or the handle.
//!
//! [`DuplicateIdentifier`]: super::GenerateError::DuplicateIdentifier

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use super::GenerateError;

/// The methods of the handle that the generated code does not write: the one every
/// wrapper calls, and the one that gives the document back.
const HANDLE: [&str; 2] = ["call", "document"];

/// What one operation contributes to the three namespaces.
pub(super) struct Operation<'a> {
    /// The `operationId`, as the document spells it.
    pub(super) id: &'a str,
    /// The variant of `OperationId`.
    pub(super) variant: String,
    /// The wrapper, a method of the handle.
    pub(super) wrapper: String,
    /// The method beside it that starts the builder.
    pub(super) builder: String,
    /// The arguments of the wrapper, in order.
    pub(super) members: Vec<Member>,
}

/// One argument of a wrapper, which `bon` makes a setter of.
#[derive(Clone)]
pub(super) struct Member {
    /// The name of the argument without the `r#` of a raw identifier, because the
    /// setter is the same word.
    pub(super) name: String,
    /// Whether the argument is an `Option`, which `bon` gives a second setter.
    pub(super) optional: bool,
    /// A phrase naming what the argument carries: `the parameter `limit``.
    pub(super) carries: String,
}

/// The names of one namespace, and the phrase for the item that claimed each.
#[derive(Default)]
struct Claims(BTreeMap<String, String>);

impl Claims {
    /// Records that `by` has `name`, and refuses where an item has it already.
    fn claim(&mut self, name: &str, by: String) -> Result<(), GenerateError> {
        match self.0.entry(name.to_owned()) {
            Entry::Occupied(first) => Err(GenerateError::DuplicateIdentifier {
                name: name.to_owned(),
                first: first.get().clone(),
                second: by,
            }),
            Entry::Vacant(slot) => {
                slot.insert(by);
                Ok(())
            }
        }
    }
}

/// Refuses the first name that two items of `ops.rs` would define.
pub(super) fn check(operations: &[Operation<'_>]) -> Result<(), GenerateError> {
    let mut methods = Claims::default();
    for name in HANDLE {
        methods.claim(name, format!("the handle's own `{name}`"))?;
    }
    for operation in operations {
        let id = operation.id;
        methods.claim(&operation.wrapper, format!("the wrapper of `{id}`"))?;
        methods.claim(
            &operation.builder,
            format!("the `_builder` method of `{id}`"),
        )?;
    }
    let mut variants = Claims::default();
    for operation in operations {
        variants.claim(
            &operation.variant,
            format!("the variant of `{}`", operation.id),
        )?;
    }
    operations.iter().try_for_each(setters)
}

/// Refuses the first name that two methods of the builder of `operation` would
/// have.
fn setters(operation: &Operation<'_>) -> Result<(), GenerateError> {
    let mut builder = Claims::default();
    builder.claim(
        "call",
        format!("the method that finishes the builder of `{}`", operation.id),
    )?;
    for member in &operation.members {
        let name = &member.name;
        let carries = &member.carries;
        builder.claim(name, format!("the setter `{name}` of {carries}"))?;
        if member.optional {
            let maybe = format!("maybe_{name}");
            builder.claim(&maybe, format!("the setter `{maybe}` of {carries}"))?;
        }
    }
    Ok(())
}
