//! `OperationId`, one typed method per operation, and the inventory a
//! hand-written operation asserts against.
//!
//! A wrapper is four lines and holds no path template, no query rule and no
//! encoder: it names an `OperationId` variant, names the arguments under the
//! document's own names, and hands the result to the one request builder a CLI
//! also uses. That is why nothing here restates what `types.rs` already says.
//!
//! `OperationId` is the reason the wrappers cannot name an operation the
//! document lacks: it is generated from the document, in the document's own
//! order, so it is a closed set that `Api::new` pairs with the embedded
//! document once. It also carries the one fact the runtime crate cannot know —
//! which Rust type each operation's body is — as `check_body`, so a
//! `--json-body` file is held to the same schema a Rust caller is.

use heck::{ToPascalCase, ToSnakeCase};
use openapiv3::{OpenAPI, ReferenceOr, Schema, SchemaKind, StatusCode, Type};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};

use super::GenerateError;
use crate::model::{Body, Shape};
use crate::{Document, Operation};

/// What one operation contributes to the generated file.
struct Emitted {
    /// `(operationId, method, path)` — one row of the generated `OPERATIONS`.
    row: TokenStream,
    /// The `OperationId` variant.
    variant: Ident,
    /// The `operationId`, as the document spells it.
    id: String,
    /// The group a CLI mounts it under.
    group: String,
    /// The subcommand name under that group.
    command: String,
    /// The Rust type of its JSON request body, when it has one.
    body: Option<TokenStream>,
    /// The typed wrapper, `impl Api`.
    method: TokenStream,
    /// The same call with its arguments named, in the `builder` block beside
    /// it. It delegates rather than repeating the body, so the two cannot
    /// describe different requests.
    builder_method: TokenStream,
}

/// A document element this generator has no Rust spelling for.
fn unsupported(reason: impl Into<String>) -> GenerateError {
    GenerateError::Unsupported(reason.into())
}

pub(super) fn emit(api: &OpenAPI, model: &Document, header: &str) -> Result<String, GenerateError> {
    let ops = gather(api, model)?;
    let operation_id = operation_id(&ops);
    let inventory = inventory(&ops);
    let methods = ops.iter().map(|op| &op.method);
    let builder_methods = ops.iter().map(|op| &op.builder_method);
    let file: syn::File = syn::parse2(quote! {
        use typed_openapi::{Part, Values};

        use crate::{Api, Call, Error, NoContent};

        #operation_id
        #inventory

        impl Api {
            #(#methods)*
        }

        #[cfg(feature = "builder")]
        #[::typed_openapi::bon::bon(crate = ::typed_openapi::bon)]
        impl Api {
            #(#builder_methods)*
        }
    })
    .map_err(|source| GenerateError::NotRust {
        file: "ops.rs",
        source,
    })?;
    Ok(format!("{header}{}", prettyplease::unparse(&file)))
}

/// One pass over the document, in its order — which is the order every
/// generated list below is in, and the order `Api::new` checks.
fn gather(api: &OpenAPI, model: &Document) -> Result<Vec<Emitted>, GenerateError> {
    model
        .iter()
        .map(|op| {
            let (path_item, operation) = find(api, op).ok_or_else(|| {
                unsupported(format!("`{}` is not in the overlaid document", op.id()))
            })?;
            let (id, method, path) = (op.id(), op.method().as_str(), op.path());
            Ok(Emitted {
                row: quote! { (#id, #method, #path) },
                variant: variant_of(op),
                id: id.to_owned(),
                group: op.group().as_str().to_owned(),
                command: op.command().as_str().to_owned(),
                body: json_body_type(op, operation)?,
                method: wrapper(op, path_item, operation)?,
                builder_method: builder_wrapper(op, path_item, operation)?,
            })
        })
        .collect()
}

/// The closed set of operations, and the two things only generated code knows
/// about each one: what it is called on a command line, and what type its body
/// is.
fn operation_id(ops: &[Emitted]) -> TokenStream {
    let variants = ops.iter().map(|op| {
        let (variant, id) = (&op.variant, &op.id);
        quote! { #[doc = #id] #variant }
    });
    let idents = ops.iter().map(|op| &op.variant);
    let from_command = ops.iter().map(|op| {
        let (group, command, variant) = (&op.group, &op.command, &op.variant);
        quote! { (#group, #command) => Some(Self::#variant) }
    });
    let check_body = check_body(ops);

    quote! {
        #[doc = "Every operation the overlaid document declares, in its order."]
        #[doc = ""]
        #[doc = "A wrapper names one of these rather than a string, so the"]
        #[doc = "wrappers cannot ask for an operation the document lacks; and"]
        #[doc = "`Api::new` pairs the whole set with the embedded document"]
        #[doc = "once, so a stale artefact is a named error at startup rather"]
        #[doc = "than a subcommand that cannot run."]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum OperationId {
            #(#variants),*
        }

        impl OperationId {
            #[doc = "Every variant, in the document's order — which is also"]
            #[doc = "`OPERATIONS`' order and this enum's discriminant order."]
            pub const ALL: &'static [OperationId] = &[#(OperationId::#idents),*];

            #[doc = "The `<group> <command>` pair the CLI mounts this"]
            #[doc = "operation under."]
            #[doc = ""]
            #[doc = "This is the one place a name off the command line becomes"]
            #[doc = "a typed operation; everything past it is exhaustive."]
            #[must_use]
            pub fn from_command(group: &str, command: &str) -> Option<Self> {
                match (group, command) {
                    #(#from_command,)*
                    _ => None,
                }
            }

            #check_body
        }
    }
}

/// The one fact about an operation that only the generated types can supply:
/// which Rust type its request body is.
///
/// A body assembled on a command line goes through this before a request is
/// built, so the CLI path is held to the same schema as the typed path.
fn check_body(ops: &[Emitted]) -> TokenStream {
    let typed = ops.iter().filter_map(|op| {
        let (variant, id, ty) = (&op.variant, &op.id, op.body.as_ref()?);
        Some(quote! { Self::#variant => typed_openapi::client::fits::<#ty>(#id, body) })
    });
    let untyped: Vec<&Ident> = ops
        .iter()
        .filter(|op| op.body.is_none())
        .map(|op| &op.variant)
        .collect();
    let untyped = (!untyped.is_empty()).then(|| quote! { #(Self::#untyped)|* => Ok(()) });
    quote! {
        #[doc = "Does `body` fit the type this operation's wrapper takes?"]
        #[doc = ""]
        #[doc = "An operation whose body this crate has no type for accepts"]
        #[doc = "anything, which is the document's own position on it."]
        pub fn check_body(
            self,
            body: &serde_json::Value,
        ) -> Result<(), typed_openapi::client::BodyError> {
            match self {
                #(#typed,)*
                #untyped
            }
        }
    }
}

/// The inventory a hand-written operation asserts against, and `Api::new`
/// pairs the embedded document with.
fn inventory(ops: &[Emitted]) -> TokenStream {
    let rows = ops.iter().map(|op| &op.row);
    let count = ops.len();
    quote! {
        #[doc = "Every `(operationId, method, path)` the overlaid document declares."]
        #[doc = ""]
        #[doc = "Row `n` describes `OperationId::ALL[n]`. `Api::new` checks that"]
        #[doc = "against the document it embeds before handing out an `Api`."]
        pub const OPERATIONS: &[(&str, &str, &str)] = &[#(#rows),*];

        #[doc = "How many operations the document declares. An operation *added*"]
        #[doc = "upstream moves this number and nothing else would have noticed."]
        pub const OPERATION_COUNT: usize = #count;

        const fn str_eq(a: &str, b: &str) -> bool {
            let (a, b) = (a.as_bytes(), b.as_bytes());
            if a.len() != b.len() {
                return false;
            }
            let mut i = 0;
            while i < a.len() {
                if a[i] != b[i] {
                    return false;
                }
                i += 1;
            }
            true
        }

        #[doc = "`const _: () = assert!(documented(..));` beside a hand-written"]
        #[doc = "operation, or beside code that depends on one, turns that"]
        #[doc = "operation's disappearance from the document into a compile error."]
        #[must_use]
        pub const fn documented(id: &str, method: &str, path: &str) -> bool {
            let mut i = 0;
            while i < OPERATIONS.len() {
                let (oid, m, p) = OPERATIONS[i];
                if str_eq(oid, id) && str_eq(m, method) && str_eq(p, path) {
                    return true;
                }
                i += 1;
            }
            false
        }
    }
}

/// The `OperationId` variant for an operation: the `operationId`, PascalCased.
fn variant_of(op: &Operation) -> Ident {
    format_ident!("{}", op.id().to_pascal_case())
}

/// The Rust type of an operation's JSON request body, when it has one.
///
/// `None` covers an operation with no body and one whose body a CLI sends
/// verbatim — neither has a type to hold a `--json-body` file to.
fn json_body_type(
    op: &Operation,
    operation: &openapiv3::Operation,
) -> Result<Option<TokenStream>, GenerateError> {
    match op.body() {
        Body::JsonFields(_) | Body::JsonWhole { .. } => body_type(operation).map(Some),
        Body::None | Body::Opaque { .. } | Body::Multipart { .. } => Ok(None),
    }
}

/// The document's own entry for an operation the model already accepted.
fn find<'a>(
    api: &'a OpenAPI,
    op: &Operation,
) -> Option<(&'a openapiv3::PathItem, &'a openapiv3::Operation)> {
    let item = api.paths.paths.get(op.path())?.as_item()?;
    let operation = item.iter().find_map(|(_, candidate)| {
        (candidate.operation_id.as_deref() == Some(op.id())).then_some(candidate)
    })?;
    Some((item, operation))
}

fn wrapper(
    op: &Operation,
    item: &openapiv3::PathItem,
    operation: &openapiv3::Operation,
) -> Result<TokenStream, GenerateError> {
    let name = format_ident!("{}", op.id().to_snake_case());
    let summary = op.summary().unwrap_or(op.id());
    let signature = format!("{} {}", op.method(), op.path());
    let gate = match op.effect() {
        crate::Effect::Write => {
            "This operation writes. A Rust caller is trusted; the CLI holds it behind `--commit`."
        }
        crate::Effect::Read => "A read.",
    };

    let Signature {
        args,
        builder,
        notes,
        // The delegate beside this one forwards the argument names; a
        // positional call has no use for them.
        names: _,
    } = signature_of(op, item, operation)?;
    let response = response_type(operation);
    let notes = notes
        .iter()
        .map(|note| quote! { #[doc = ""] #[doc = #note] });
    let variant = variant_of(op);
    Ok(quote! {
        #[doc = #summary]
        #[doc = ""]
        #[doc = #signature]
        #[doc = ""]
        #[doc = #gate]
        #(#notes)*
        pub fn #name(&self, #(#args),*) -> Result<Call<'_, #response>, Error> {
            self.call(OperationId::#variant, Values::new() #(#builder)*)
        }
    })
}

/// The same operation with its arguments named at the call site.
///
/// The name carries a `_builder` suffix because this is an *addition*: the
/// `builder` feature must not change what an existing call site means, and a
/// feature that replaced `update_voucher` would break every crate that shares
/// the generated code, since cargo resolves features once for a whole build.
///
/// It delegates to the plain wrapper rather than repeating its body, so the two
/// cannot come to describe different requests.
fn builder_wrapper(
    op: &Operation,
    item: &openapiv3::PathItem,
    operation: &openapiv3::Operation,
) -> Result<TokenStream, GenerateError> {
    let plain = format_ident!("{}", op.id().to_snake_case());
    let name = format_ident!("{}_builder", op.id().to_snake_case());
    let Signature { args, names, .. } = signature_of(op, item, operation)?;
    let response = response_type(operation);
    let doc = format!(
        "The same call as [`Api::{plain}`], with its arguments named. A missing \
         required argument is a compile error."
    );
    Ok(quote! {
        #[doc = #doc]
        #[builder]
        pub fn #name(&self, #(#args),*) -> Result<Call<'_, #response>, Error> {
            self.#plain(#(#names),*)
        }
    })
}

/// One wrapper's arguments, the `Values` builder chain they feed, and whatever
/// about them the document can explain but the types cannot.
struct Signature {
    args: Vec<TokenStream>,
    names: Vec<Ident>,
    builder: Vec<TokenStream>,
    notes: Vec<String>,
}

fn signature_of(
    op: &Operation,
    item: &openapiv3::PathItem,
    operation: &openapiv3::Operation,
) -> Result<Signature, GenerateError> {
    let mut out = Signature {
        args: Vec::new(),
        names: Vec::new(),
        builder: Vec::new(),
        notes: Vec::new(),
    };
    for param in op.params() {
        add_param(&mut out, param, item, operation)?;
    }
    match op.body() {
        Body::None => {}
        Body::JsonFields(_) | Body::JsonWhole { .. } => {
            let ty = body_type(operation)?;
            out.args.push(quote! { body: &#ty });
            out.names.push(format_ident!("body"));
            out.builder.push(quote! { .json(crate::to_json(body)?) });
        }
        Body::Opaque { media_type, .. } => {
            out.notes.push(format!(
                "`body` is sent verbatim under the document's own `{media_type}`, \
                 which this crate does not assemble."
            ));
            out.args.push(quote! { body: Vec<u8> });
            out.names.push(format_ident!("body"));
            out.builder.push(quote! { .raw(body) });
        }
        Body::Multipart { names, .. } => {
            out.notes.push(multipart_note(names));
            out.args.push(quote! { parts: Vec<Part> });
            out.names.push(format_ident!("parts"));
            out.builder.push(quote! { .multipart(parts) });
        }
    }
    Ok(out)
}

/// What one parameter adds to a wrapper: an argument and the `Values` call that
/// fills it — or, for a parameter with no command-line spelling, a note saying
/// the wrapper does not carry it either. A Rust caller and a command line reach
/// one request builder, so what neither can supply is missing from both.
fn add_param(
    out: &mut Signature,
    param: &crate::Param,
    item: &openapiv3::PathItem,
    operation: &openapiv3::Operation,
) -> Result<(), GenerateError> {
    let join = match param.shape() {
        Shape::Flag { join, .. } => join,
        Shape::Unreachable(why) => {
            out.notes.push(format!(
                "The document's `{}` parameter is not an argument: it is {why}. \
                 A request built here does not carry it.",
                param.name()
            ));
            return Ok(());
        }
    };
    let ident = format_ident!("{}", param.name().to_snake_case());
    let schema = param_schema(item, operation, param.name())?;
    let wire = param.name();
    out.names.push(ident.clone());
    if join.is_some() {
        // A list parameter is the wire name given once per value, which is the
        // repetition a repeated flag reaches the request builder with.
        let ty = list_type(param.name(), schema)?;
        out.args.push(quote! { #ident: Vec<#ty> });
        out.builder.push(quote! { .each(#wire, #ident) });
    } else if param.required() {
        let ty = scalar_type(schema)?;
        out.args.push(quote! { #ident: #ty });
        out.builder.push(quote! { .param(#wire, #ident) });
    } else {
        let ty = scalar_type(schema)?;
        out.args.push(quote! { #ident: Option<#ty> });
        out.builder.push(quote! { .maybe(#wire, #ident) });
    }
    Ok(())
}

fn multipart_note(names: &[String]) -> String {
    let assembled = "`parts` are assembled into a `multipart/form-data` body.";
    if names.is_empty() {
        assembled.to_owned()
    } else {
        format!("{assembled} The document declares: {}.", names.join(", "))
    }
}

/// The schema one parameter declares, keyed on the document's own name.
fn param_schema<'d>(
    item: &'d openapiv3::PathItem,
    operation: &'d openapiv3::Operation,
    name: &str,
) -> Result<&'d ReferenceOr<Schema>, GenerateError> {
    let declared = item
        .parameters
        .iter()
        .chain(&operation.parameters)
        .find_map(|p| {
            let ReferenceOr::Item(p) = p else { return None };
            (p.parameter_data_ref().name == name).then_some(p)
        })
        .ok_or_else(|| unsupported(format!("`{name}` is not declared on this operation")))?;
    let openapiv3::ParameterSchemaOrContent::Schema(schema) = &declared.parameter_data_ref().format
    else {
        return Err(unsupported(format!(
            "`{name}` is declared with `content`, not `schema`"
        )));
    };
    Ok(schema)
}

/// The Rust spelling of a list parameter's items.
///
/// Only an inline `type: array` has one here. Following a `$ref` to an array
/// schema would mean resolving against `components`, which this generator does
/// not carry, so the parameter names itself rather than being guessed at.
fn list_type(name: &str, schema: &ReferenceOr<Schema>) -> Result<TokenStream, GenerateError> {
    let ReferenceOr::Item(schema) = schema else {
        return Err(unsupported(format!(
            "`{name}` is a list, and a list parameter must declare `items` inline"
        )));
    };
    let SchemaKind::Type(Type::Array(array)) = &schema.schema_kind else {
        return Err(unsupported(format!(
            "`{name}` is a list that is not an array"
        )));
    };
    let Some(items) = &array.items else {
        return Err(unsupported(format!(
            "`{name}` is an array declaring no `items`"
        )));
    };
    scalar_type(&items.clone().unbox())
}

/// A scalar schema's Rust spelling. A `$ref` to a named schema keeps its name,
/// so an enumerated parameter is the generated enum rather than a string.
fn scalar_type(schema: &ReferenceOr<Schema>) -> Result<TokenStream, GenerateError> {
    if let Some(name) = ref_name(schema) {
        let ident = format_ident!("{name}");
        return Ok(quote!(crate::types::#ident));
    }
    let ReferenceOr::Item(schema) = schema else {
        return Err(unsupported(
            "only `#/components/schemas/` references are followed",
        ));
    };
    let SchemaKind::Type(kind) = &schema.schema_kind else {
        return Err(unsupported("only `type:` schemas have a scalar spelling"));
    };
    Ok(match kind {
        Type::String(_) => quote!(&str),
        Type::Integer(_) => quote!(i64),
        Type::Number(_) => quote!(f64),
        Type::Boolean(_) => quote!(bool),
        Type::Object(_) | Type::Array(_) => return Err(unsupported("not a scalar")),
    })
}

/// The type of a JSON request body. A `$ref` keeps its name; anything else is
/// a `serde_json::Value`, because the document did not name a shape to generate.
fn body_type(operation: &openapiv3::Operation) -> Result<TokenStream, GenerateError> {
    let Some(ReferenceOr::Item(body)) = &operation.request_body else {
        return Err(unsupported("requestBody $refs are not followed"));
    };
    let Some(media) = body
        .content
        .iter()
        .find_map(|(name, media)| crate::schema::is_json(name).then_some(media))
    else {
        return Err(unsupported("no JSON request body"));
    };
    let Some(schema) = &media.schema else {
        return Ok(quote!(serde_json::Value));
    };
    Ok(named_or_value(schema))
}

/// The type a successful response deserialises into.
fn response_type(operation: &openapiv3::Operation) -> TokenStream {
    let success =
        operation.responses.responses.iter().find(
            |(status, _)| matches!(status, StatusCode::Code(code) if (200..300).contains(code)),
        );
    let Some((_, ReferenceOr::Item(success))) = success else {
        return quote!(NoContent);
    };
    let Some(media) = success
        .content
        .iter()
        .find_map(|(name, media)| crate::schema::is_json(name).then_some(media))
    else {
        return quote!(NoContent);
    };
    let Some(schema) = &media.schema else {
        return quote!(NoContent);
    };
    if let Some(name) = ref_name(schema) {
        let ident = format_ident!("{name}");
        return quote!(crate::types::#ident);
    }
    let ReferenceOr::Item(schema) = schema else {
        return quote!(serde_json::Value);
    };
    let SchemaKind::Type(Type::Array(array)) = &schema.schema_kind else {
        return quote!(serde_json::Value);
    };
    let Some(items) = &array.items else {
        return quote!(serde_json::Value);
    };
    let items = items.clone().unbox();
    let inner = named_or_value(&items);
    quote!(Vec<#inner>)
}

fn named_or_value(schema: &ReferenceOr<Schema>) -> TokenStream {
    if let Some(name) = ref_name(schema) {
        let ident = format_ident!("{name}");
        return quote!(crate::types::#ident);
    }
    quote!(serde_json::Value)
}

fn ref_name(schema: &ReferenceOr<Schema>) -> Option<&str> {
    match schema {
        ReferenceOr::Reference { reference } => reference.strip_prefix("#/components/schemas/"),
        ReferenceOr::Item(_) => None,
    }
}
