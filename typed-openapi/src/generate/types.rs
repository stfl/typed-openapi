//! `components.schemas` as Rust types, from typify, with two things typify
//! leaves out.
//!
//! A named schema that states a `pattern` becomes a newtype whose `FromStr`
//! enforces it, which is the whole reason to name a schema rather than repeat a
//! rule. typify gives that newtype `Deref<Target = String>`, `FromStr` and two
//! `TryFrom`s — and no `Display`, so a value that came back from the API can be
//! dereferenced into a `format!` but not written to one. [`display_impls`] is
//! the missing half, for the newtypes that are missing it. typify also writes
//! `::regress::Regex` into every such
//! check; [`ThroughThisCrate`] points those at this crate's re-export, so the
//! crate holding the generated code adds no dependency of its own.
//!
//! # A type the adopter owns is reached through a placeholder
//!
//! [`Settings::replace`](super::Settings::replace) says that a vendor *format*
//! stands for a Rust type the adopter already has. Every schema declaring that
//! format is rewritten into a reference to a [placeholder](PLACEHOLDER)
//! definition, and that definition is the adopter's type: typify substitutes a
//! definition named in
//! [`with_replacement`](typify::TypeSpaceSettings::with_replacement) and writes
//! nothing for it. Keying the rewrite on the format rather than on a name is
//! what keeps the substitution honest — the shape being replaced is read out of
//! the document.
//!
//! Going through a reference is what makes one rule out of what would otherwise
//! be two. A named schema is then a definition that is a *bare reference*,
//! which typify reads as a newtype over what the reference resolves to — so a
//! named schema carrying the format is a `#[serde(transparent)]` wrapper over
//! the adopter's type whatever the schema is called, and a schema that merely
//! *mentions* the format on a property is the adopter's type itself. The
//! alternative is [`with_conversion`], which substitutes on the shape and then
//! declines to wrap a definition whose name happens to be what the adopter's
//! path ends in: schema `Cents` against `money::Cents` would stand alone where
//! schema `Amount` against the same type wraps. Which of the two an adopter got
//! would turn on a coincidence between a name the vendor chose and a name they
//! chose, and a schema name is not theirs to move — it is what goes back to the
//! vendor and out to every other consumer of the document.
//!
//! Rewriting typify's output afterwards is the other way to one rule, and it is
//! the worse one: the emitted file spells the adopter's path the same way
//! wherever it stands, so repointing the uses that came from a reference —
//! and only those — would mean knowing which generated field came from which
//! property. That is typify's naming rule, copied, which is the thing
//! [`names`](super::names) exists to keep this crate from owning.
//!
//! # typify answers what each schema is called
//!
//! typify chooses the Rust name of every generated type, and it chooses it
//! here — so this is where [`Names`](super::names::Names) is read off the
//! `TypeSpace`, and it travels out beside the source for `ops` to look a
//! schema up in.
//!
//! # A schema stated inline is converted here too
//!
//! [`names::sites`](super::names::sites) finds every schema an operation
//! states where it uses it rather than under a name, and each one is handed to
//! typify beside the named ones. That is the whole of what makes an inline
//! request body a type: typify converts any schema it is given, and the named
//! schemas were the only ones being offered. The rewrite reaches them too, so a
//! `format` declared inside an inline body becomes the adopter's type exactly
//! as it does inside a named schema.
//!
//! [`with_conversion`]: typify::TypeSpaceSettings::with_conversion

use std::collections::BTreeSet;

use proc_macro2::Span;
use quote::quote;
use serde_json::Value;
use syn::visit_mut::VisitMut;
use typify::{TypeSpace, TypeSpaceImpl, TypeSpaceSettings};

use super::GenerateError;
use super::names::{Names, Site};
use crate::Document;

/// One schema of the document, under its own name, in typify's dialect.
type Definition = (String, Value);

/// One schema an operation states inline, in typify's dialect.
type Stated = (Site, Value);

/// The crate typify writes into a generated `pattern` check.
const ENGINE: &str = "regress";

/// What the definition standing for an adopter's type is declared under, before
/// the index that tells one replacement from the next.
///
/// It reaches nothing a reader sees: typify replaces a definition of this name
/// with the adopter's own path and writes no type for it, so the name is a key
/// in the schema set handed to typify and in nothing else. It is spelled as
/// PascalCase words because that is what typify's own sanitiser hands back for
/// it — the registration is keyed by the sanitised name, so a name that
/// sanitises to itself is what makes the registration and the definition agree
/// without this crate owning a second copy of the sanitising rule.
///
/// A document is free to declare a schema that reduces to the same Rust name,
/// and [`Names::read`](super::names::Names::read) refuses that by name rather
/// than letting the schema be quietly substituted.
const PLACEHOLDER: &str = "TypedOpenapiReplaced";

/// The generated source, and the name every schema in it ended up with.
///
/// The names come back with the source because this is the only place that
/// knows them: typify chooses them, and it chooses them here. `ops` is handed
/// the result rather than deriving its own — see [`Names`].
pub(super) fn emit(
    api: &openapiv3::OpenAPI,
    model: &Document,
    header: &str,
    replacements: &[(String, String)],
) -> Result<(String, Names), GenerateError> {
    let mut definitions = definitions(api)?;
    let mut stated = stated(api, model)?;

    let owned = owned(replacements);
    for (_, schema) in &mut definitions {
        behind_a_reference(schema, &owned)?;
    }
    for (_, schema) in &mut stated {
        behind_a_reference(schema, &owned)?;
    }

    // Read before the placeholders join them: `Names` answers for the schemas
    // the document declares, and a placeholder is this crate's own.
    let declared: Vec<String> = definitions.iter().map(|(name, _)| name.clone()).collect();

    let mut settings = TypeSpaceSettings::default();
    settings.with_derive("PartialEq".to_owned());
    for owned in &owned {
        settings.with_replacement(
            &owned.placeholder,
            &owned.rust,
            [TypeSpaceImpl::Display, TypeSpaceImpl::FromStr].into_iter(),
        );
        definitions.push((owned.placeholder.clone(), Value::Bool(true)));
    }

    let mut space = TypeSpace::new(&settings);
    space
        .add_ref_types(schemas(definitions)?)
        .map_err(GenerateError::Typify)?;
    let names = Names::read(
        &mut space,
        declared.iter().map(String::as_str),
        inline(stated)?,
    )?;

    let mut file: syn::File =
        syn::parse2(space.to_stream()).map_err(|source| GenerateError::NotRust {
            file: "types.rs",
            source,
        })?;
    ThroughThisCrate.visit_file_mut(&mut file);
    super::Prose.visit_file_mut(&mut file);
    let mut displays = display_impls(&file)?;
    file.items.append(&mut displays);
    for promise in promises(&file, replacements)?.into_iter().rev() {
        file.items.insert(0, promise);
    }
    Ok((format!("{header}{}", prettyplease::unparse(&file)), names))
}

/// What [`Settings::replace`] promised typify, checked by the generated crate.
///
/// `with_conversion` tells typify that the adopter's type parses from a string
/// and prints to one, and typify writes the newtype's `Display`, `FromStr` and
/// both `TryFrom`s in terms of that promise. An adopter who breaks it does not
/// read that they broke it: a missing `FromStr` is four `E0271`s and two
/// `E0276`s about an associated type that cannot be resolved, and a missing
/// `Display` is `E0599: no method named `fmt``, all of them tens of thousands
/// of lines inside a file they did not write.
///
/// The promise is only owed where typify *wrapped* the type, which is at a
/// named schema. Where it emitted the type directly — a property, a list's
/// items, anything the document describes without naming — nothing is written
/// in terms of it and the adopter's type needs neither trait. So the emitted
/// file is what decides who is asked, and a type nobody wrapped is asked for
/// nothing.
///
/// The traits the newtype *derives* are deliberately not here. `Clone`,
/// `Debug`, `PartialEq`, serde's pair and whatever else typify adds for the
/// shape already fail one at a time, naming the type and the trait, which is
/// as good as this could make them — and which of them are derived varies with
/// the shape, so a fixed list would eventually fail for a reason that is not
/// the reason.
///
/// [`Settings::replace`]: super::Settings::replace
fn promises(
    file: &syn::File,
    replacements: &[(String, String)],
) -> Result<Vec<syn::Item>, GenerateError> {
    let wrapped: BTreeSet<String> = file.items.iter().filter_map(wraps).collect();
    let mut asked = BTreeSet::new();
    let mut promises = Vec::new();
    for (format, rust) in replacements {
        let ty: syn::Type = syn::parse_str(rust).map_err(|source| {
            GenerateError::Unsupported(format!(
                "`{rust}`, named for `format: {format}`, is not a Rust type: {source}"
            ))
        })?;
        let spelling = quote!(#ty).to_string();
        if !wrapped.contains(&spelling) || !asked.insert(spelling) {
            continue;
        }
        let said = format!(
            "`Settings::replace(\"{format}\", \"{rust}\")` promises that this type parses \
             from a string and prints to one, and the newtype above is written in terms \
             of both."
        );
        promises.push(
            syn::parse2(quote! {
                #[doc = #said]
                const _: () = {
                    fn parses_from_a_string<T: ::std::str::FromStr>() {}
                    fn prints_to_a_string<T: ::std::fmt::Display>() {}
                    fn a_type_named_by_settings_replace() {
                        parses_from_a_string::<#ty>();
                        prints_to_a_string::<#ty>();
                    }
                };
            })
            .map_err(|source| GenerateError::NotRust {
                file: "types.rs",
                source,
            })?,
        );
    }
    Ok(promises)
}

/// The type this item is a newtype over, if that is what it is.
fn wraps(item: &syn::Item) -> Option<String> {
    let syn::Item::Struct(item) = item else {
        return None;
    };
    let syn::Fields::Unnamed(fields) = &item.fields else {
        return None;
    };
    if !item.generics.params.is_empty() || fields.unnamed.len() != 1 {
        return None;
    }
    let wrapped = &fields.unnamed.first()?.ty;
    Some(quote!(#wrapped).to_string())
}

/// `Display` for every generated newtype that wraps a string and has none.
///
/// typify writes one itself for a newtype it left *unconstrained* — a named
/// schema that is a bare `type: string`, with no `pattern`, no `enum` and no
/// `format` — and omits it only for the ones whose `FromStr` enforces a rule.
/// Which of the two a schema became is not a thing to predict, so it is read
/// off the file: a struct that already carries a `Display` is skipped, and a
/// second `impl` for one is a generated crate that does not compile.
///
/// Reading the emitted file rather than the schemas settles the names too.
/// typify decides which schemas become newtypes, and a name it chose is the
/// name the `impl` has to carry. Writing to `self.0` is what makes the two
/// travel together — the `impl` lands in the module holding the struct, so the
/// private field is in reach and no `Deref` is assumed.
fn display_impls(file: &syn::File) -> Result<Vec<syn::Item>, GenerateError> {
    let printed: BTreeSet<String> = file.items.iter().filter_map(prints).collect();
    file.items
        .iter()
        .filter_map(string_newtype)
        .filter(|name| !printed.contains(&name.to_string()))
        .map(|name| {
            syn::parse2(quote! {
                impl ::std::fmt::Display for #name {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        ::std::fmt::Display::fmt(&self.0, f)
                    }
                }
            })
            .map(syn::Item::Impl)
            .map_err(|source| GenerateError::NotRust {
                file: "types.rs",
                source,
            })
        })
        .collect()
}

/// The type this item writes a `Display` for, if that is what this item is.
fn prints(item: &syn::Item) -> Option<String> {
    let syn::Item::Impl(item) = item else {
        return None;
    };
    let (path, _) = item.trait_.as_ref()?;
    if path.segments.last()?.ident != "Display" {
        return None;
    }
    let syn::Type::Path(printed) = item.self_ty.as_ref() else {
        return None;
    };
    Some(printed.path.segments.last()?.ident.to_string())
}

/// The name of a newtype over a string, if that is what this item is.
fn string_newtype(item: &syn::Item) -> Option<&syn::Ident> {
    let syn::Item::Struct(item) = item else {
        return None;
    };
    let syn::Fields::Unnamed(fields) = &item.fields else {
        return None;
    };
    if !item.generics.params.is_empty() || fields.unnamed.len() != 1 {
        return None;
    }
    let syn::Type::Path(wrapped) = &fields.unnamed.first()?.ty else {
        return None;
    };
    (wrapped.qself.is_none() && wrapped.path.segments.last()?.ident == "String")
        .then_some(&item.ident)
}

/// Every `::regress::…` path the generator emitted, pointed at this crate's
/// re-export instead.
///
/// The alternative is a generated crate that declares the engine typify
/// happened to choose — a dependency its adopter never asked for, on a version
/// nothing holds to the one the generated syntax was written against. Going
/// through the re-export settles both, the way `bon` is reached for the
/// builder.
struct ThroughThisCrate;

impl VisitMut for ThroughThisCrate {
    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        syn::visit_mut::visit_path_mut(self, path);
        if path.leading_colon.is_some()
            && path.segments.first().is_some_and(|it| it.ident == ENGINE)
        {
            path.segments.insert(
                0,
                syn::Ident::new("typed_openapi", Span::call_site()).into(),
            );
        }
    }
}

/// Every named schema the document declares, in document order.
///
/// A document that names none is a document with no `components` block, or one
/// whose block holds only security schemes. Both are ordinary and both
/// generate: the wrappers name no type and the types file carries typify's
/// error module and nothing else.
fn definitions(api: &openapiv3::OpenAPI) -> Result<Vec<Definition>, GenerateError> {
    let Some(components) = api.components.as_ref() else {
        return Ok(Vec::new());
    };
    components
        .schemas
        .iter()
        .map(|(name, schema)| {
            let value = serde_json::to_value(schema).map_err(|source| GenerateError::Schema {
                name: name.clone(),
                source,
            })?;
            Ok((name.clone(), as_json_schema(value)))
        })
        .collect()
}

/// The same schemas as typify's own type, which is the last point at which a
/// shape it cannot read is still attributable to a name.
fn schemas(
    definitions: Vec<Definition>,
) -> Result<Vec<(String, schemars::schema::Schema)>, GenerateError> {
    definitions
        .into_iter()
        .map(|(name, value)| {
            let schema: schemars::schema::Schema =
                serde_json::from_value(value).map_err(|source| {
                    GenerateError::Unsupported(format!(
                        "schema `{name}` is not a JSON Schema typify accepts: {source}"
                    ))
                })?;
            Ok((name, schema))
        })
        .collect()
}

/// Every schema an operation states inline, in the model's order.
///
/// They are read in typify's dialect like the named ones, so a `$ref` on a
/// property of an inline body reaches the same definition a named schema's
/// property would.
fn stated(api: &openapiv3::OpenAPI, model: &Document) -> Result<Vec<Stated>, GenerateError> {
    super::names::sites(api, model)
        .into_iter()
        .map(|(site, schema)| {
            let value = serde_json::to_value(schema).map_err(|source| GenerateError::Schema {
                name: site.described(),
                source,
            })?;
            Ok((site, as_json_schema(value)))
        })
        .collect()
}

/// The same schemas as typify's own type.
fn inline(stated: Vec<Stated>) -> Result<Vec<(Site, schemars::schema::Schema)>, GenerateError> {
    stated
        .into_iter()
        .map(|(site, value)| {
            let schema: schemars::schema::Schema =
                serde_json::from_value(value).map_err(|source| {
                    GenerateError::Unsupported(format!(
                        "{} is not a JSON Schema typify accepts: {source}",
                        site.described()
                    ))
                })?;
            Ok((site, schema))
        })
        .collect()
}

/// One vendor format an adopter owns the Rust type for, and the definition
/// every schema declaring it is reached through.
struct Owned {
    /// The `format` keyword, in the document's own spelling.
    format: String,
    /// The definition typify substitutes the adopter's type for.
    placeholder: String,
    /// The adopter's own path, as [`Settings::replace`] spelled it.
    ///
    /// [`Settings::replace`]: super::Settings::replace
    rust: String,
}

/// One [`Owned`] per format the adopter named, in the order they were named.
///
/// A format named twice keeps the first, so that a later call cannot change
/// what an earlier one already decided for a document this run also reads.
fn owned(replacements: &[(String, String)]) -> Vec<Owned> {
    let mut owned: Vec<Owned> = Vec::new();
    for (format, rust) in replacements {
        if owned.iter().any(|it| it.format == *format) {
            continue;
        }
        owned.push(Owned {
            format: format.clone(),
            placeholder: format!("{PLACEHOLDER}{}", owned.len()),
            rust: rust.clone(),
        });
    }
    owned
}

/// Every schema under `value` that declares a format the adopter owns the type
/// for, rewritten into a reference to the placeholder standing for it.
///
/// The whole document is walked rather than its named schemas alone, because a
/// format is declared wherever a value is described — on a property of a named
/// schema, on a property of a body an operation states inline, on the items of
/// a list. Each of them is one substitution, and all of them are the same one.
fn behind_a_reference(value: &mut Value, owned: &[Owned]) -> Result<(), GenerateError> {
    match value {
        Value::Array(items) => items
            .iter_mut()
            .try_for_each(|item| behind_a_reference(item, owned)),
        Value::Object(fields) => {
            let declared = fields
                .get("format")
                .and_then(Value::as_str)
                .and_then(|format| owned.iter().find(|it| it.format == format));
            match declared {
                Some(owned) => {
                    *value = reference_to(owned, std::mem::take(value))?;
                    Ok(())
                }
                None => fields
                    .values_mut()
                    .try_for_each(|nested| behind_a_reference(nested, owned)),
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

/// A reference to `owned`'s placeholder, carrying what `schema` said about
/// itself and nothing it said about its shape.
///
/// typify reads a definition that is a *bare* reference as a newtype over what
/// the reference resolves to, and a bare one only: a `type` or a `pattern` left
/// beside the `$ref` is merged into the referenced shape instead, which is a
/// different type and not the one the adopter owns. So the shape goes and the
/// metadata stays — the title and description a generated doc comment is
/// written from describe the field or the schema rather than the shape, and
/// losing them would cost the generated type the vendor's own prose.
///
/// Which keys those are is schemars' answer rather than a list here, because
/// schemars is what typify reads the schema with.
fn reference_to(owned: &Owned, schema: Value) -> Result<Value, GenerateError> {
    let unreadable = |source: serde_json::Error| {
        GenerateError::Unsupported(format!(
            "a `format: {}` shape typify does not accept: {source}",
            owned.format
        ))
    };
    let stated: schemars::schema::SchemaObject =
        serde_json::from_value(schema).map_err(unreadable)?;
    serde_json::to_value(schemars::schema::SchemaObject {
        metadata: stated.metadata,
        reference: Some(format!("#/definitions/{}", owned.placeholder)),
        ..schemars::schema::SchemaObject::default()
    })
    .map_err(unreadable)
}

/// An OpenAPI schema object as the JSON Schema typify understands: only the
/// `$ref` spelling differs.
fn as_json_schema(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let value = match (key.as_str(), &value) {
                        ("$ref", Value::String(target)) => {
                            Value::String(target.replace("#/components/schemas/", "#/definitions/"))
                        }
                        _ => as_json_schema(value),
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(as_json_schema).collect()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::{PLACEHOLDER, owned};

    /// Every format the adopter named, as `(format, placeholder, type)`.
    fn read(named: &[(&str, &str)]) -> Vec<(String, String, String)> {
        let named: Vec<(String, String)> = named
            .iter()
            .map(|(format, rust)| ((*format).to_owned(), (*rust).to_owned()))
            .collect();
        owned(&named)
            .into_iter()
            .map(|it| (it.format, it.placeholder, it.rust))
            .collect()
    }

    /// One format is one decision, and the call that made it is the first one.
    ///
    /// A [`Settings`] is assembled a call at a time and read once, so a second
    /// call naming a format the first already decided arrives after everything
    /// that decision reached — the definition every schema declaring the format
    /// was rewritten into, and the promise the generated crate is held to. It is
    /// dropped whole rather than registered beside the first, because a format
    /// with two definitions standing for it has one more than it has types.
    ///
    /// [`Settings`]: super::super::Settings
    #[test]
    fn a_format_named_twice_keeps_the_first_call_and_one_definition() {
        assert_eq!(
            read(&[("cents", "money::Cents"), ("cents", "second::Cents")]),
            [(
                "cents".to_owned(),
                format!("{PLACEHOLDER}0"),
                "money::Cents".to_owned()
            )]
        );
    }

    /// Two formats are two decisions, each reached through a definition of its
    /// own — which is what says the rule above is about one format rather than
    /// about any second call.
    #[test]
    fn two_formats_are_two_definitions_in_the_order_they_were_named() {
        assert_eq!(
            read(&[("cents", "money::Cents"), ("amount", "money::Amount")]),
            [
                (
                    "cents".to_owned(),
                    format!("{PLACEHOLDER}0"),
                    "money::Cents".to_owned()
                ),
                (
                    "amount".to_owned(),
                    format!("{PLACEHOLDER}1"),
                    "money::Amount".to_owned()
                )
            ]
        );
    }
}
