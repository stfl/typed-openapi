//! `components.schemas` as Rust types, from typify, with two things typify
//! leaves out.
//!
//! A named schema that states a `pattern` becomes a newtype whose `FromStr`
//! enforces it, which is the whole reason to name a schema rather than repeat a
//! rule. typify gives that newtype `Deref<Target = String>`, `FromStr` and two
//! `TryFrom`s — and no `Display`, so a value that came back from the API can be
//! dereferenced into a `format!` but not written to one. [`display_impls`] is
//! the missing half. typify also writes `::regress::Regex` into every such
//! check; [`ThroughThisCrate`] points those at this crate's re-export, so the
//! crate holding the generated code adds no dependency of its own.
//!
//! The one setting that matters is [`with_conversion`]: the adopter owns a Rust
//! type for a vendor *format*, and typify emits that type wherever the document
//! declares the format —
//! [`Settings::replace`](super::Settings::replace) is the call that says so.
//! Keying on the format rather than on a name is what keeps that honest: the
//! shape typify is told to replace is read out of the document.
//!
//! [`with_conversion`]: typify::TypeSpaceSettings::with_conversion

use proc_macro2::Span;
use quote::quote;
use serde_json::Value;
use syn::visit_mut::VisitMut;
use typify::{TypeSpace, TypeSpaceImpl, TypeSpaceSettings};

use super::GenerateError;

/// One schema of the document, under its own name, in typify's dialect.
type Definition = (String, Value);

/// The crate typify writes into a generated `pattern` check.
const ENGINE: &str = "regress";

pub(super) fn emit(
    api: &openapiv3::OpenAPI,
    header: &str,
    replacements: &[(String, String)],
) -> Result<String, GenerateError> {
    let definitions = definitions(api)?;

    let mut settings = TypeSpaceSettings::default();
    settings.with_derive("PartialEq".to_owned());
    for (format, rust) in replacements {
        for shape in shapes_declaring(&definitions, format) {
            let shape: schemars::schema::SchemaObject =
                serde_json::from_value(shape).map_err(|source| {
                    GenerateError::Unsupported(format!(
                        "a `format: {format}` shape typify does not accept: {source}"
                    ))
                })?;
            settings.with_conversion(
                shape,
                rust,
                [TypeSpaceImpl::Display, TypeSpaceImpl::FromStr].into_iter(),
            );
        }
    }

    let mut space = TypeSpace::new(&settings);
    space
        .add_ref_types(schemas(definitions)?)
        .map_err(GenerateError::Typify)?;

    let mut file: syn::File =
        syn::parse2(space.to_stream()).map_err(|source| GenerateError::NotRust {
            file: "types.rs",
            source,
        })?;
    ThroughThisCrate.visit_file_mut(&mut file);
    let mut displays = display_impls(&file)?;
    file.items.append(&mut displays);
    Ok(format!("{header}{}", prettyplease::unparse(&file)))
}

/// `Display` for every generated newtype that wraps a string.
///
/// It reads the emitted file rather than the schemas because the file is what
/// settles the question: typify decides which schemas become newtypes, and a
/// name it chose is the name the `impl` has to carry. Writing to `self.0` is
/// what makes the two travel together — the `impl` lands in the module holding
/// the struct, so the private field is in reach and no `Deref` is assumed.
fn display_impls(file: &syn::File) -> Result<Vec<syn::Item>, GenerateError> {
    file.items
        .iter()
        .filter_map(string_newtype)
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
fn definitions(api: &openapiv3::OpenAPI) -> Result<Vec<Definition>, GenerateError> {
    let components = api.components.as_ref().ok_or_else(|| {
        GenerateError::Unsupported("the overlaid document has no components".to_owned())
    })?;
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

/// Every distinct schema in the document that declares `format`, in document
/// order.
///
/// typify matches a conversion on the whole shape, so a document that spells
/// one format two ways gets one conversion per spelling rather than a silent
/// miss on the second.
fn shapes_declaring(definitions: &[Definition], format: &str) -> Vec<Value> {
    let mut found = Vec::new();
    for (_, schema) in definitions {
        collect(schema, format, &mut found);
    }
    found
}

fn collect(value: &Value, format: &str, found: &mut Vec<Value>) {
    let Value::Object(fields) = value else {
        if let Value::Array(items) = value {
            for item in items {
                collect(item, format, found);
            }
        }
        return;
    };
    if fields.get("format") == Some(&Value::String(format.to_owned())) && !found.contains(value) {
        found.push(value.clone());
    }
    for nested in fields.values() {
        collect(nested, format, found);
    }
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
