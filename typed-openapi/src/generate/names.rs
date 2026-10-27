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
//! them being renamed by a generator nobody asked. One schema stated in one
//! place and reached from two is not two definitions: a component that two
//! operations share, with a `title` on the schema it states inline, is a type
//! that both take. typify answers a name it already holds with the type it
//! holds, whatever shape the new schema has, so the refusal stands unless the
//! two schemas are the same.
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
use std::sync::LazyLock;

use openapiv3::{Components, MediaType, OpenAPI, PathItem, ReferenceOr, Schema, StatusCode};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use schemars::schema::{Schema as JsonSchema, SchemaObject};
use syn::visit_mut::VisitMut;
use typify::{Type, TypeDetails, TypeId, TypeSpace};

use super::GenerateError;
use crate::model::Body;
use crate::schema::{RefError, is_json, resolve_request_body, resolve_response};
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

    /// Whether this is the request body of an operation, and not its response.
    pub(super) fn is_body(&self) -> bool {
        self.role == Role::Body
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
    /// A schema an operation states where it uses it, and the schema it states.
    Inline(Site, Box<JsonSchema>),
}

impl Origin {
    fn described(&self) -> String {
        match self {
            Origin::Named(schema) => format!("the schema `{schema}`"),
            Origin::Inline(site, _) => site.described(),
        }
    }

    /// The schema, where an operation states it.
    fn stated(&self) -> Option<&JsonSchema> {
        match self {
            Origin::Named(_) => None,
            Origin::Inline(_, schema) => Some(schema.as_ref()),
        }
    }
}

/// The Rust names asked for so far, and who asked first.
#[derive(Default)]
struct Claims(BTreeMap<String, Claim>);

impl Claims {
    /// Record that `origin` asked for the type `id`, which typify spells `rust`.
    ///
    /// A second claimant of a name is refused, unless it is the first asking
    /// again.
    fn record(&mut self, rust: String, id: TypeId, origin: &Origin) -> Result<(), GenerateError> {
        let asked = origin.described();
        let Some(first) = self.0.get(&rust) else {
            let stated = origin.stated().cloned();
            self.0.insert(rust, Claim { id, asked, stated });
            return Ok(());
        };
        if first.is_asked_again(&id, origin) {
            return Ok(());
        }
        Err(GenerateError::OneType {
            first: first.asked.clone(),
            second: asked,
            // typify's own spelling of the name, rather than the token stream's,
            // because this is a sentence an adopter reads.
            rust,
            // The declared schemas are resolved before the inline ones, so the
            // later claimant is inline whenever either is.
            inline: matches!(origin, Origin::Inline(..)),
        })
    }
}

/// Who asked for a Rust name first.
struct Claim {
    /// The type typify answered with.
    id: TypeId,
    /// The sentence that names the claimant.
    asked: String,
    /// The schema, where the claimant is stated inline.
    stated: Option<JsonSchema>,
}

impl Claim {
    /// Whether `origin` asking for the type `id` is the claimant asking again.
    ///
    /// typify hands back the type it holds for a name it holds, so the same id
    /// does not say that the schema is the same. The schema is the same where one
    /// schema is stated in one place and reached from two, as a component that
    /// two operations share is.
    fn is_asked_again(&self, id: &TypeId, origin: &Origin) -> bool {
        self.id == *id && self.stated.as_ref() == origin.stated()
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
                .map_err(|error| GenerateError::Typify(Box::new(error)))?;
            resolved.push((Origin::Named(schema.to_owned()), id));
        }
        for (site, schema) in inline {
            let id = space
                .add_type_with_name(&schema, Some(site.suggested()))
                .map_err(|error| GenerateError::Typify(Box::new(error)))?;
            resolved.push((Origin::Inline(site, Box::new(schema)), id));
        }

        let defined = defined_in(space);
        let mut claimed = Claims::default();
        let mut names = Self {
            named: BTreeMap::new(),
            inline: BTreeMap::new(),
        };
        for (origin, id) in resolved {
            let ty = space
                .get_type(&id)
                .map_err(|error| GenerateError::Typify(Box::new(error)))?;
            if let Origin::Named(schema) = &origin
                && !defined_by_typify(&ty)
            {
                return Err(GenerateError::Reserved {
                    schema: schema.clone(),
                });
            }
            let path = path_of(&ty, &defined)?;
            if defined_by_typify(&ty) {
                claimed.record(ty.name(), id, &origin)?;
            }
            match origin {
                Origin::Named(schema) => names.named.insert(schema, path),
                Origin::Inline(site, _) => names.inline.insert(site, path),
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
    let components = components(api);
    let mut found = Vec::new();
    for op in model {
        let Some((_, operation)) = find(api, op) else {
            continue;
        };
        if let Some(ReferenceOr::Item(schema)) = body_of(operation, op, components) {
            found.push((Site::body(op.id()), schema));
        }
        // An answer that cannot be read states no schema here; the emitter of the
        // wrappers meets the same refusal and names it.
        if let Ok(Some(Answer::Stated(ReferenceOr::Item(schema)))) = success(operation, components)
        {
            found.push((Site::response(op.id()), schema));
        }
    }
    found
}

/// The `components` of the document, or none where it holds none: a `$ref` then
/// leads nowhere, as it does where the reduction reads the same document.
///
/// One empty set for the process, so that what is read through it lives as long
/// as the document does and a document without `components` needs no owner for
/// its stand-in.
pub(super) fn components(api: &OpenAPI) -> &Components {
    static NONE: LazyLock<Components> = LazyLock::new(Components::default);
    api.components.as_ref().unwrap_or(&NONE)
}

/// The schema of every JSON request body a wrapper takes, named or stated, and
/// the site of the operation that takes it.
///
/// What a request body reaches is read from here, so the reach is the one the
/// wrappers have: the entry that is sent, as the model keys it.
pub(super) fn bodies<'a>(
    api: &'a OpenAPI,
    model: &Document,
) -> Vec<(Site, &'a ReferenceOr<Schema>)> {
    let components = components(api);
    model
        .into_iter()
        .filter_map(|op| {
            let (_, operation) = find(api, op)?;
            Some((Site::body(op.id()), body_of(operation, op, components)?))
        })
        .collect()
}

/// The schema of the JSON body that `op` sends, where the document states one.
///
/// A body that cannot be followed states no schema here. The emitter of the
/// wrappers reads it through [`json_body`] too, and names the refusal.
fn body_of<'a>(
    operation: &'a openapiv3::Operation,
    op: &Operation,
    components: &'a Components,
) -> Option<&'a ReferenceOr<Schema>> {
    let (Body::JsonFields { media_type, .. } | Body::JsonWhole { media_type, .. }) = op.body()
    else {
        return None;
    };
    match json_body(operation, media_type, components) {
        Ok(JsonBody::Stated(schema)) => Some(schema),
        Ok(JsonBody::Unkeyed | JsonBody::Shapeless) | Err(_) => None,
    }
}

/// What an operation says its JSON request body is.
///
/// One reading, because two of them would let the schema a type is generated
/// from and the schema a wrapper names come apart. The variants an emitter
/// refuses are named here rather than collapsed into "no body", so the refusal
/// can say which of them it was.
pub(super) enum JsonBody<'a> {
    /// The operation declares no `requestBody`, or its `content` has no entry
    /// under the key the model sends it under: the model and the document it was
    /// reduced from disagree.
    Unkeyed,
    /// It declares a JSON body and states no schema for it: the document
    /// describes no shape.
    Shapeless,
    /// It declares a JSON body and states this schema for it.
    Stated(&'a ReferenceOr<Schema>),
}

/// The JSON request body an operation declares, read from the `content` entry
/// under `media_type`, the key the model sends it under.
///
/// The key is the model's and is not picked again here: the reduction chooses
/// the entry a body is sent under in one place, and the type of a wrapper's
/// argument is read from the entry that is sent. A `requestBody` that is a
/// `$ref` is followed as the reduction follows it ([`resolve_request_body`]), so
/// a body is the same entry on both roads.
pub(super) fn json_body<'a>(
    operation: &'a openapiv3::Operation,
    media_type: &str,
    components: &'a Components,
) -> Result<JsonBody<'a>, RefError> {
    let Some(body) = &operation.request_body else {
        return Ok(JsonBody::Unkeyed);
    };
    let Some(media) = resolve_request_body(body, components)?
        .content
        .get(media_type)
    else {
        return Ok(JsonBody::Unkeyed);
    };
    Ok(match &media.schema {
        None => JsonBody::Shapeless,
        Some(schema) => JsonBody::Stated(schema),
    })
}

/// What the JSON body of a success response says it is.
#[derive(Debug, PartialEq)]
pub(super) enum Answer<'a> {
    /// The body states no schema: any JSON is the answer, as it is for a request
    /// body that states none.
    Shapeless,
    /// The schema the body states.
    Stated(&'a ReferenceOr<Schema>),
}

impl Answer<'_> {
    /// How a refusal names what a response says, beside its status.
    fn described(&self) -> String {
        match self {
            Answer::Shapeless => "a JSON body with no schema".to_owned(),
            Answer::Stated(ReferenceOr::Reference { reference }) => format!("`{reference}`"),
            Answer::Stated(ReferenceOr::Item(_)) => "a schema stated inline".to_owned(),
        }
    }
}

/// What the success responses of an operation answer with, or `None` where none
/// of them has a JSON body and the call answers
/// [`NoContent`](crate::NoContent).
///
/// A success response is a status in `200` to `299` and the range `2XX`, with a
/// `$ref` to `#/components/responses/` followed as the reduction follows a
/// request body. The ones that have a JSON body are the answer, and they have
/// to agree: a wrapper has one return type, and choosing one of several here
/// would be the generator choosing for the adopter. Two responses agree when
/// they state the same schema, a `$ref` to the same name or a schema that reads
/// the same. A response with no JSON content has no body to type, whatever its
/// status, so a `204` beside a `200` takes nothing from it. `default` is not a
/// success response and is not read.
///
/// A response that cannot be followed is refused, and so are responses that
/// disagree: an answer that cannot be typed is never `NoContent`, which
/// accepts any body and drops it.
pub(super) fn success<'a>(
    operation: &'a openapiv3::Operation,
    components: &'a Components,
) -> Result<Option<Answer<'a>>, GenerateError> {
    let mut first: Option<(String, Answer<'a>)> = None;
    for (status, response) in &operation.responses.responses {
        if !matches!(status, StatusCode::Code(200..=299) | StatusCode::Range(2)) {
            continue;
        }
        let response = resolve_response(response, components).map_err(|source| {
            GenerateError::Unsupported(format!(
                "its success response `{status}` cannot be followed: {source}"
            ))
        })?;
        let Some(media) = json(&response.content) else {
            continue;
        };
        let stated = media
            .schema
            .as_ref()
            .map_or(Answer::Shapeless, Answer::Stated);
        let Some((earlier, kept)) = &first else {
            first = Some((status.to_string(), stated));
            continue;
        };
        if *kept != stated {
            return Err(GenerateError::Unsupported(format!(
                "its success responses `{earlier}` and `{status}` name different JSON \
                 schemas, {} and {}, so the wrapper has no one type to return; an Overlay \
                 that gives both one schema, or removes one of them, mends it",
                kept.described(),
                stated.described()
            )));
        }
    }
    Ok(first.map(|(_, answer)| answer))
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

/// The JSON entry of a response's `content` map, whichever spelling of the
/// media type the document used.
fn json<'a>(
    content: impl IntoIterator<Item = (&'a String, &'a MediaType)>,
) -> Option<&'a MediaType> {
    content
        .into_iter()
        .find_map(|(name, media)| is_json(name).then_some(media))
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
