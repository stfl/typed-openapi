//! What a schema the document names is called in Rust, answered in one place.
//!
//! Two emitters need that answer. [`types`](super::types) writes the types, and
//! [`ops`](super::ops) writes wrappers whose arguments and return values name
//! them. A rule spelled in both places can be spelled two ways, and what an
//! adopter gets when it is spelled two ways is a generated crate that does not
//! compile — one emitter asking for a type the other never defined.
//!
//! So neither emitter spells it. typify chose the name, and [`Names`] asks
//! typify what it chose: a reference to a definition it has already converted
//! resolves to that type without adding anything, so this is a reading of the
//! mapping rather than a second copy of the rule behind it. A copy would be a
//! real risk and not a theoretical one — typify's own `sanitize` prefixes a
//! name that starts with a digit with `x`, and progenitor's, written by the
//! same authors against the same rule, prefixes it with `_`. Two copies of one
//! rule already disagree in the wild.
//!
//! Two schemas that come back under one name are refused. typify emits a
//! definition per schema and does not uniquify a name two of them reduce to,
//! so the alternative is a generated file carrying the same `struct` twice —
//! and this is the rule the command line already follows, where two operations
//! reducing to one `<group> <command>` are refused by name rather than one of
//! them being renamed by a generator nobody asked.
//!
//! It is also how the answer stays right for a type the adopter owns.
//! [`Settings::replace`](super::Settings::replace) substitutes their type, and
//! where typify writes no definition for the schema at all there is no
//! `crate::types::` name to find — only the path they named, which is what
//! typify hands back.

use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::quote;
use schemars::schema::{Schema, SchemaObject};
use typify::{Type, TypeDetails, TypeId, TypeSpace};

use super::GenerateError;

/// Every schema the document names, paired with the type a wrapper names it by.
#[derive(Debug)]
pub(super) struct Names(BTreeMap<String, TokenStream>);

impl Names {
    /// Ask `space` what it called each of `schemas`.
    ///
    /// Two passes, because the question is put to a mutable `space` and
    /// answered by a borrow of it: every name is resolved to an id first, and
    /// the ids are read afterwards.
    pub(super) fn read<'a>(
        space: &mut TypeSpace,
        schemas: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, GenerateError> {
        let resolved: Vec<(String, TypeId)> = schemas
            .into_iter()
            .map(|schema| {
                let id = space
                    .add_type(&reference(schema))
                    .map_err(GenerateError::Typify)?;
                Ok((schema.to_owned(), id))
            })
            .collect::<Result<_, GenerateError>>()?;
        let mut claimed: BTreeMap<String, String> = BTreeMap::new();
        let mut names = BTreeMap::new();
        for (schema, id) in resolved {
            let ty = space.get_type(&id).map_err(GenerateError::Typify)?;
            let path = path_of(&ty);
            if let Some(first) = claimed.insert(path.to_string(), schema.clone()) {
                return Err(GenerateError::OneType {
                    first,
                    second: schema,
                    // typify's own spelling of the name, rather than the token
                    // stream's, because this is a sentence an adopter reads.
                    rust: ty.name(),
                });
            }
            names.insert(schema, path);
        }
        Ok(Self(names))
    }

    /// The type `schema` became, as a wrapper spells it.
    pub(super) fn get(&self, schema: &str) -> Result<&TokenStream, GenerateError> {
        self.0.get(schema).ok_or_else(|| GenerateError::NoType {
            schema: schema.to_owned(),
        })
    }
}

/// A named schema of the document, as typify spells a reference to one.
fn reference(schema: &str) -> Schema {
    Schema::Object(SchemaObject {
        reference: Some(format!("#/definitions/{schema}")),
        ..SchemaObject::default()
    })
}

/// Where a wrapper reaches the type a schema became.
///
/// A type typify defined is in the generated `types` module, so a wrapper names
/// it through `crate::types`. A type [`Settings::replace`] substituted is the
/// adopter's own and names itself from anywhere the generated crate compiles —
/// putting that one under `crate::types` would name a module it was never in.
///
/// [`Settings::replace`]: super::Settings::replace
fn path_of(ty: &Type<'_>) -> TokenStream {
    let ident = ty.ident();
    if defined_by_typify(ty) {
        quote!(crate::types::#ident)
    } else {
        ident
    }
}

/// Did typify write this type into the file it emitted?
fn defined_by_typify(ty: &Type<'_>) -> bool {
    matches!(
        ty.details(),
        TypeDetails::Enum(_) | TypeDetails::Struct(_) | TypeDetails::Newtype(_)
    )
}
