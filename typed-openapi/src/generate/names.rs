//! What a schema becomes in Rust, answered in one place.
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
//! Two definitions that come back under one name are refused. typify emits a
//! definition per schema and does not uniquify a name two of them reduce to,
//! so the alternative is a generated file carrying the same `struct` twice —
//! and this is the rule the command line already follows, where two operations
//! reducing to one `<group> <command>` are refused by name rather than one of
//! them being renamed by a generator nobody asked.
//!
//! It is also how the answer stays right for a type the adopter owns.
//! [`Settings::replace`](super::Settings::replace) substitutes their type
//! wherever a schema states the format without naming it, and there is no
//! `crate::types::` name to find for such a schema — only the path they named,
//! which is what typify hands back.
//!
//! A schema the document *does* name has a type of its own, always: typify
//! writes a struct, an enum or a newtype for every definition it is given, and
//! the one thing that stops it is a definition whose name this crate reserved
//! for a replaced type. So a named schema with no type of its own is that
//! collision and nothing else, and it is refused by name — the alternative is
//! the vendor's schema quietly becoming the adopter's type everywhere it is
//! used.
//!
//! # A schema an operation states inline is a schema like any other
//!
//! A document is free to describe a request body or a response where it is
//! used rather than under a name in `components.schemas`, and such a schema
//! carries everything a named one carries: `$ref`s to named schemas on its
//! properties, `enum`s, `pattern`s, a `required` list. typify converts any
//! schema it is given, so the only thing standing between an inline schema and
//! a real Rust type is being offered one — [`sites`] is what finds them and
//! [`Site`] is the key both emitters reach them by.
//!
//! What such a type is called goes through typify as well: this module
//! *suggests* the operation's own name and the role the schema plays, and
//! typify settles the spelling. A `title` on the schema wins over the
//! suggestion, which is typify's rule and the right one — `title` is what
//! OpenAPI offers for naming a shape, so a vendor who wrote one has named the
//! type and a generator overriding them would be inventing a name nobody
//! asked for. Either way the name is a fact of the document, so a bless run
//! reproduces it, and either way it is put through the same refusal.

use std::collections::{BTreeMap, BTreeSet};

use openapiv3::{MediaType, OpenAPI, PathItem, ReferenceOr, Schema, StatusCode};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use schemars::schema::{Schema as JsonSchema, SchemaObject};
use syn::visit_mut::VisitMut;
use typify::{Type, TypeDetails, TypeId, TypeSpace};

use super::GenerateError;
use crate::model::Body;
use crate::{Document, Operation};

/// The module a generated type is reached through, from the file beside it.
const MODULE: [&str; 2] = ["crate", "types"];

/// Which of an operation's two schemas an inline one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Role {
    /// The JSON request body a call sends.
    Body,
    /// The body of the successful response it deserialises.
    Response,
}

impl Role {
    /// The word a suggested name ends in.
    fn word(self) -> &'static str {
        match self {
            Role::Body => "body",
            Role::Response => "response",
        }
    }
}

/// One place an operation states a schema instead of naming one.
///
/// The operation and the role are the whole key, which is what makes the same
/// site nameable from both emitters without either one carrying a schema.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Site {
    op: String,
    role: Role,
}

impl Site {
    /// The request body `op` states inline.
    pub(super) fn body(op: &str) -> Self {
        Self {
            op: op.to_owned(),
            role: Role::Body,
        }
    }

    /// The successful response `op` states inline.
    pub(super) fn response(op: &str) -> Self {
        Self {
            op: op.to_owned(),
            role: Role::Response,
        }
    }

    /// What this crate suggests the type be called.
    ///
    /// Words rather than an identifier: typify sanitises a name it is given
    /// into PascalCase, so handing it `addVoucher-body` and handing it
    /// `AddVoucherBody` are the same suggestion, and spelling it here would be
    /// the second copy of a rule this module exists to avoid keeping.
    fn suggested(&self) -> String {
        format!("{}-{}", self.op, self.role.word())
    }

    /// How a refusal names this site, as a phrase that reads inside a sentence
    /// beside a named schema's.
    pub(super) fn described(&self) -> String {
        match self.role {
            Role::Body => format!("the request body stated inline by `{}`", self.op),
            Role::Response => format!("the response stated inline by `{}`", self.op),
        }
    }
}

/// What asked for a type, for the refusal that names two of them.
enum Origin {
    /// A schema the document declares under `components.schemas`.
    Named(String),
    /// A schema an operation states where it uses it.
    Inline(Site),
}

impl Origin {
    fn described(&self) -> String {
        match self {
            Origin::Named(schema) => format!("the schema `{schema}`"),
            Origin::Inline(site) => site.described(),
        }
    }
}

/// Every schema the document names or states inline, paired with the type a
/// wrapper names it by.
#[derive(Debug)]
pub(super) struct Names {
    named: BTreeMap<String, TokenStream>,
    inline: BTreeMap<Site, TokenStream>,
}

impl Names {
    /// Ask `space` what it called each of `schemas` and each of `inline`.
    ///
    /// Two passes, because the question is put to a mutable `space` and
    /// answered by a borrow of it: every schema is resolved to an id first, and
    /// the ids are read afterwards.
    pub(super) fn read<'a>(
        space: &mut TypeSpace,
        schemas: impl IntoIterator<Item = &'a str>,
        inline: impl IntoIterator<Item = (Site, JsonSchema)>,
    ) -> Result<Self, GenerateError> {
        let mut resolved: Vec<(Origin, TypeId)> = Vec::new();
        for schema in schemas {
            let id = space
                .add_type(&reference(schema))
                .map_err(GenerateError::Typify)?;
            resolved.push((Origin::Named(schema.to_owned()), id));
        }
        for (site, schema) in inline {
            let id = space
                .add_type_with_name(&schema, Some(site.suggested()))
                .map_err(GenerateError::Typify)?;
            resolved.push((Origin::Inline(site), id));
        }

        let defined = defined_in(space);
        let mut claimed: BTreeMap<String, String> = BTreeMap::new();
        let mut names = Self {
            named: BTreeMap::new(),
            inline: BTreeMap::new(),
        };
        for (origin, id) in resolved {
            let ty = space.get_type(&id).map_err(GenerateError::Typify)?;
            if let Origin::Named(schema) = &origin
                && !defined_by_typify(&ty)
            {
                return Err(GenerateError::Reserved {
                    schema: schema.clone(),
                });
            }
            let path = path_of(&ty, &defined)?;
            let asked = origin.described();
            if defined_by_typify(&ty)
                && let Some(first) = claimed.insert(ty.name(), asked.clone())
            {
                return Err(GenerateError::OneType {
                    first,
                    second: asked,
                    // typify's own spelling of the name, rather than the token
                    // stream's, because this is a sentence an adopter reads.
                    rust: ty.name(),
                });
            }
            match origin {
                Origin::Named(schema) => names.named.insert(schema, path),
                Origin::Inline(site) => names.inline.insert(site, path),
            };
        }
        Ok(names)
    }

    /// The type `schema` became, as a wrapper spells it.
    pub(super) fn get(&self, schema: &str) -> Result<&TokenStream, GenerateError> {
        self.named.get(schema).ok_or_else(|| GenerateError::NoType {
            schema: schema.to_owned(),
        })
    }

    /// The type the schema stated at `site` became, as a wrapper spells it.
    ///
    /// `None` only where an emitter asks about a site [`sites`] did not find.
    /// The two cannot disagree about that: [`sites`] reads the document through
    /// [`json_body`] and [`success`], and so does the emitter.
    pub(super) fn at(&self, site: &Site) -> Option<&TokenStream> {
        self.inline.get(site)
    }
}

/// Every schema an operation states inline, in the model's order.
///
/// Driven by the reduced model rather than by the document's paths, so that the
/// set is exactly the operations the wrappers are emitted for — an operation
/// the reduction dropped states nothing this generator has to name.
pub(super) fn sites<'a>(api: &'a OpenAPI, model: &Document) -> Vec<(Site, &'a Schema)> {
    let mut found = Vec::new();
    for op in model {
        let Some((_, operation)) = find(api, op) else {
            continue;
        };
        if matches!(op.body(), Body::JsonFields(_) | Body::JsonWhole { .. })
            && let JsonBody::Stated(ReferenceOr::Item(schema)) = json_body(operation)
        {
            found.push((Site::body(op.id()), schema));
        }
        if let Some(ReferenceOr::Item(schema)) = success(operation) {
            found.push((Site::response(op.id()), schema));
        }
    }
    found
}

/// What an operation says its JSON request body is.
///
/// One reading, because two of them would let the schema a type is generated
/// from and the schema a wrapper names come apart. The variants an emitter
/// refuses are named here rather than collapsed into "no body", so the refusal
/// can say which of them it was.
pub(super) enum JsonBody<'a> {
    /// The operation declares no `requestBody`, or declares it as a `$ref`.
    Referenced,
    /// It declares one, and none of its `content` entries is JSON.
    NotJson,
    /// It declares a JSON body and states no schema for it: the document
    /// describes no shape.
    Shapeless,
    /// It declares a JSON body and states this schema for it.
    Stated(&'a ReferenceOr<Schema>),
}

/// The JSON request body an operation declares.
pub(super) fn json_body(operation: &openapiv3::Operation) -> JsonBody<'_> {
    let Some(ReferenceOr::Item(body)) = &operation.request_body else {
        return JsonBody::Referenced;
    };
    let Some(media) = json(&body.content) else {
        return JsonBody::NotJson;
    };
    match &media.schema {
        None => JsonBody::Shapeless,
        Some(schema) => JsonBody::Stated(schema),
    }
}

/// The schema a successful response deserialises from, where the document
/// states one.
///
/// Every answer a caller has no type for — no success status, no JSON content,
/// no schema — is the same answer, so this is an `Option` rather than a
/// reading like [`JsonBody`]: a wrapper returns `NoContent` for all three.
pub(super) fn success(operation: &openapiv3::Operation) -> Option<&ReferenceOr<Schema>> {
    let (_, response) = operation.responses.responses.iter().find(
        |(status, _)| matches!(status, StatusCode::Code(code) if (200..300).contains(code)),
    )?;
    let ReferenceOr::Item(response) = response else {
        return None;
    };
    json(&response.content)?.schema.as_ref()
}

/// The document's own entry for an operation the model already accepted.
pub(super) fn find<'a>(
    api: &'a OpenAPI,
    op: &Operation,
) -> Option<(&'a PathItem, &'a openapiv3::Operation)> {
    let item = api.paths.paths.get(op.path())?.as_item()?;
    let operation = item.iter().find_map(|(_, candidate)| {
        (candidate.operation_id.as_deref() == Some(op.id())).then_some(candidate)
    })?;
    Some((item, operation))
}

/// The JSON entry of a `content` map, whichever spelling of the media type the
/// document used.
fn json<'a>(
    content: impl IntoIterator<Item = (&'a String, &'a MediaType)>,
) -> Option<&'a MediaType> {
    content
        .into_iter()
        .find_map(|(name, media)| crate::schema::is_json(name).then_some(media))
}

/// A named schema of the document, as typify spells a reference to one.
fn reference(schema: &str) -> JsonSchema {
    JsonSchema::Object(SchemaObject {
        reference: Some(format!("#/definitions/{schema}")),
        ..SchemaObject::default()
    })
}

/// The name of every type typify wrote into the file it emitted.
fn defined_in(space: &TypeSpace) -> BTreeSet<String> {
    space
        .iter_types()
        .filter(defined_by_typify)
        .map(|ty| ty.name())
        .collect()
}

/// Where a wrapper reaches the type a schema became.
///
/// typify spells a type without the module it lands in, and the generated
/// wrappers sit in a file beside that module rather than inside it — so every
/// name in the spelling that typify *defined* is qualified, and everything else
/// is left exactly as typify wrote it. Qualifying the names rather than
/// rebuilding the spelling is what keeps a compound answer right: a response
/// stated inline as an array of a named schema comes back as typify's own
/// `Vec<…>` with the item pointed at the module it is in, and a type
/// [`Settings::replace`] substituted is the adopter's own path from anywhere
/// the generated crate compiles, so nothing is added to it.
///
/// [`Settings::replace`]: super::Settings::replace
fn path_of(ty: &Type<'_>, defined: &BTreeSet<String>) -> Result<TokenStream, GenerateError> {
    let mut spelled: syn::Type =
        syn::parse2(ty.ident()).map_err(|source| GenerateError::NotRust {
            file: "ops.rs",
            source,
        })?;
    InModule(defined).visit_type_mut(&mut spelled);
    Ok(quote!(#spelled))
}

/// Every bare name of a generated type, reached through the module it is in.
///
/// A bare name is the only thing that can be one: typify writes every path it
/// does not own — `::std::vec::Vec`, `::serde_json::Value`, an adopter's
/// `money::Money` — with a root or a parent already on it.
struct InModule<'a>(&'a BTreeSet<String>);

impl VisitMut for InModule<'_> {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        syn::visit_mut::visit_path_mut(self, path);
        if path.leading_colon.is_some() || path.segments.len() != 1 {
            return;
        }
        let Some(segment) = path.segments.first() else {
            return;
        };
        if !self.0.contains(&segment.ident.to_string()) {
            return;
        }
        for (at, module) in MODULE.iter().enumerate() {
            path.segments
                .insert(at, syn::Ident::new(module, Span::call_site()).into());
        }
    }
}

/// Did typify write this type into the file it emitted?
fn defined_by_typify(ty: &Type<'_>) -> bool {
    matches!(
        ty.details(),
        TypeDetails::Enum(_) | TypeDetails::Struct(_) | TypeDetails::Newtype(_)
    )
}
