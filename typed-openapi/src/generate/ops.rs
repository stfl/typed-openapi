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
use openapiv3::{OpenAPI, ReferenceOr, Schema, SchemaKind, Type};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use syn::visit_mut::VisitMut as _;

use super::GenerateError;
use super::names::{JsonBody, Names, Site};
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

pub(super) fn emit(
    api: &OpenAPI,
    model: &Document,
    header: &str,
    names: &Names,
) -> Result<String, GenerateError> {
    let ops = gather(api, model, names)?;
    let operation_id = operation_id(&ops);
    let inventory = inventory(&ops);
    let methods = ops.iter().map(|op| &op.method);
    let builder_methods = ops.iter().map(|op| &op.builder_method);
    let mut file: syn::File = syn::parse2(quote! {
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
    // A summary and a description are the vendor's prose, and a wrapper's doc
    // is where they land.
    super::Prose.visit_file_mut(&mut file);
    Ok(format!("{header}{}", prettyplease::unparse(&file)))
}

/// One pass over the document, in its order — which is the order every
/// generated list below is in, and the order `Api::new` checks.
fn gather(api: &OpenAPI, model: &Document, names: &Names) -> Result<Vec<Emitted>, GenerateError> {
    model
        .iter()
        .map(|op| {
            let (path_item, operation) = super::names::find(api, op).ok_or_else(|| {
                unsupported(format!("`{}` is not in the overlaid document", op.id()))
            })?;
            let (id, method, path) = (op.id(), op.method().as_str(), op.path());
            // Every failure below is about this one operation, and a generated
            // file is far too large to bisect by hand — so the operation is
            // named here, once, rather than at each of the places that can
            // fail.
            let emitted = || {
                Ok(Emitted {
                    row: quote! { (#id, #method, #path) },
                    variant: variant_of(op)?,
                    id: id.to_owned(),
                    group: op.group().as_str().to_owned(),
                    command: op.command().as_str().to_owned(),
                    body: json_body_type(op, operation, names)?,
                    method: wrapper(op, path_item, operation, names)?,
                    builder_method: builder_wrapper(op, path_item, operation, names)?,
                })
            };
            emitted().map_err(|source| GenerateError::Operation {
                op: id.to_owned(),
                source: Box::new(source),
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
        #[doc = "Every operation that sends JSON has such a type, whether the"]
        #[doc = "document named the schema or stated it where it is used. The"]
        #[doc = "operations that answer for any value are the ones that send no"]
        #[doc = "JSON at all — a multipart or verbatim body, or no body — and"]
        #[doc = "the ones whose JSON body the document states no schema for."]
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
fn variant_of(op: &Operation) -> Result<Ident, GenerateError> {
    operation_ident(&op.id().to_pascal_case())
}

/// An identifier built from an operationId. [`gather`] names the operation, so
/// this says only what about it could not be spelled.
fn operation_ident(word: &str) -> Result<Ident, GenerateError> {
    ident(word).ok_or_else(|| unsupported("the operationId has no spelling as a Rust identifier"))
}

/// `word` as an identifier the generated source can carry.
///
/// A document is free to name a parameter `type` or an operation `match`, and a
/// keyword is not an identifier. A raw identifier is what keeps the document's
/// own word: `r#type` reads as what the document said, where a mangled `type_`
/// reads as something this generator invented. Four words cannot be written raw
/// at all — `crate`, `self`, `Self` and `super` — and those take the underscore
/// instead, which is the one place a name is changed rather than quoted.
///
/// Nothing about a request depends on which of the two a name gets. The wire
/// name travels beside the argument, as the literal the request builder is
/// given, so an argument is free to be spelled however Rust requires.
///
/// A word with no spelling at all — one that starts with a digit, or that
/// case-conversion emptied — is `None` rather than a panic, because
/// `format_ident!` panics and a bless step is a library call. The parse is what
/// answers that, and it is not the arm below it: `accept_as_ident` asks whether
/// a word may stand unquoted, so it says yes to `3rd_firing` and to the empty
/// string, and only `syn` refuses a word that is no identifier in any spelling.
fn ident(word: &str) -> Option<Ident> {
    if typify::accept_as_ident(word) {
        return syn::parse_str(word).ok();
    }
    match word {
        "crate" | "self" | "Self" | "super" => Some(format_ident!("{word}_")),
        _ => syn::parse_str(&format!("r#{word}")).ok(),
    }
}

/// The Rust type of an operation's JSON request body, when it has one.
///
/// `None` covers an operation with no body and one whose body a CLI sends
/// verbatim — neither has a type to hold a `--json-body` file to.
fn json_body_type(
    op: &Operation,
    operation: &openapiv3::Operation,
    names: &Names,
) -> Result<Option<TokenStream>, GenerateError> {
    match op.body() {
        Body::JsonFields(_) | Body::JsonWhole { .. } => body_type(op, operation, names).map(Some),
        Body::None | Body::Opaque { .. } | Body::Multipart { .. } => Ok(None),
    }
}

fn wrapper(
    op: &Operation,
    item: &openapiv3::PathItem,
    operation: &openapiv3::Operation,
    names: &Names,
) -> Result<TokenStream, GenerateError> {
    let name = operation_ident(&op.id().to_snake_case())?;
    let summary = op.summary().unwrap_or(op.id());
    let signature = format!("{} {}", op.method(), op.path());
    let gate = gate_note(op);

    let Signature {
        args,
        builder,
        notes,
        // The delegate beside this one forwards the argument names; a
        // positional call has no use for them.
        names: _,
    } = signature_of(op, item, operation, names)?;
    let response = response_type(op, operation, names)?;
    let variant = variant_of(op)?;
    let doc = paragraphs(
        [summary, &signature, &gate]
            .into_iter()
            .map(str::to_owned)
            .chain(notes),
    );
    parses(quote! {
        #[doc = #doc]
        pub fn #name(&self, #(#args),*) -> Result<Call<'_, #response>, Error> {
            self.call(OperationId::#variant, Values::new() #(#builder)*)
        }
    })
}

/// One doc comment out of the paragraphs it is made of.
///
/// One rather than several, and this is the reason: rustc rebuilds a doc
/// comment's text by stripping the indentation *all* of an item's doc
/// fragments share, and a `/* */` fragment carries the indentation the item
/// sits at while a `///` fragment carries none. Mixing them leaves nothing to
/// strip, and a vendor's summary arrives four spaces in — a code block, which
/// rustdoc then compiles. A single fragment cannot be mixed with anything.
///
/// A summary that is itself several paragraphs is what makes this reachable,
/// and one attribute per paragraph is what would reach it: the multi-line
/// summary becomes the `/* */` fragment and the generated paragraphs beside it
/// the `///` ones. `PROSE` carries such a summary, so the doctest runner is
/// what holds this rather than the argument above it.
fn paragraphs(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .map(|part| part.trim().to_owned())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A wrapper, checked to be Rust before it joins six thousand lines of its
/// kind.
///
/// The whole file is parsed once at the end anyway, and a failure there names a
/// position in a token stream nobody can open. Parsing each wrapper as it is
/// built costs one small parse per operation and puts the failure inside the
/// operation that caused it, where [`gather`] names it.
fn parses(wrapper: TokenStream) -> Result<TokenStream, GenerateError> {
    syn::parse2::<syn::ImplItemFn>(wrapper.clone()).map_err(|source| GenerateError::NotRust {
        file: "ops.rs",
        source,
    })?;
    Ok(wrapper)
}

/// What a wrapper's doc says about the gate a command line holds the operation
/// behind.
///
/// A Rust caller is trusted and is stopped by nothing here, so this is the one
/// place the hazard is written down for them: the doc names every word the CLI
/// demands, because a caller reading the wrapper is deciding whether to make
/// the call at all.
fn gate_note(op: &Operation) -> String {
    let named: Vec<String> = op
        .gates()
        .iter()
        .map(|gate| format!("`--{gate}`"))
        .collect();
    match (op.effect(), named.is_empty()) {
        (crate::Effect::Read, _) => "A read.".to_owned(),
        (crate::Effect::Write, true) => format!(
            "This operation writes. A Rust caller is trusted; the CLI holds it behind `--{}`.",
            op.commit()
        ),
        (crate::Effect::Write, false) => format!(
            "This operation writes. A Rust caller is trusted; the CLI holds it behind \
             `--{}` and {}.",
            op.commit(),
            named.join(" and ")
        ),
    }
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
    names: &Names,
) -> Result<TokenStream, GenerateError> {
    let plain = operation_ident(&op.id().to_snake_case())?;
    let name = format_ident!("{plain}_builder");
    let Signature {
        args,
        names: arguments,
        ..
    } = signature_of(op, item, operation, names)?;
    let response = response_type(op, operation, names)?;
    let doc = format!(
        "The same call as [`Api::{plain}`], with its arguments named. A missing \
         required argument is a compile error."
    );
    parses(quote! {
        #[doc = #doc]
        #[builder]
        pub fn #name(&self, #(#args),*) -> Result<Call<'_, #response>, Error> {
            self.#plain(#(#arguments),*)
        }
    })
}

/// One wrapper's arguments, the `Values` builder chain they feed, and whatever
/// about them the document can explain but the types cannot.
///
/// `names` is the identifiers the arguments bind, in order. It is what the
/// delegate beside a wrapper forwards, and it is also the set [`Signature::bind`]
/// asks before it hands out another one — a wrapper's argument list is the whole
/// of what an identifier has to be unique against, so the list is the namespace.
struct Signature {
    args: Vec<TokenStream>,
    names: Vec<Ident>,
    builder: Vec<TokenStream>,
    notes: Vec<String>,
}

impl Signature {
    /// The identifier one argument binds: `plain` where this wrapper has not
    /// spent it, and `word_2`, `word_3`, … where it has.
    ///
    /// A document may name two of an operation's values so that they reduce to
    /// one Rust word. `self` and `Self` are one such pair, and so is a wire name
    /// used once in the path and again in the query — both of which OpenAPI
    /// allows and neither of which a function signature does. Two arguments
    /// under one identifier is `E0415`: the generated crate does not compile,
    /// and a bless step that emitted it reports success, because `syn` parses a
    /// signature that binds a name twice without complaint. Were it to compile,
    /// the second `.maybe` would send the first argument's value under the
    /// second's wire name.
    ///
    /// So the later claimant moves aside, which is what
    /// [`Namespace`](crate::names::Namespace) already does to the same document
    /// on the command line — one operation cannot be two things to its two
    /// consumers. It moves aside *here* rather than in the reduced model because
    /// the name settled here reaches nothing else: a flag travels in the blob
    /// and is spent at run time, while an argument is spelled once, in Rust's
    /// alphabet, for an adopter's compiler.
    ///
    /// The suffix costs the request nothing. The wire name travels beside the
    /// argument as the literal the request builder is given, so what a value is
    /// sent under does not depend on what Rust had to call the binding.
    ///
    /// `word` is what the alternatives are built from rather than `plain`, so
    /// that a second `type` reads as `type_2` and not as `r#type_2`. Appending
    /// to a word that already spells an identifier spells one too, which is why
    /// nothing here can fail.
    ///
    /// This decides a name and says nothing about it. [`Signature::argument`] is
    /// the door every argument comes through, and it is where a name that moved
    /// is accounted for.
    fn bind(&mut self, plain: Ident, word: &str) -> Ident {
        let mut candidate = plain;
        let mut suffix = 2;
        while self.names.contains(&candidate) {
            candidate = format_ident!("{word}_{suffix}");
            suffix += 1;
        }
        self.names.push(candidate.clone());
        candidate
    }

    /// One argument's identifier, and the note that accounts for it where it is
    /// not the plain spelling of the document's own word.
    ///
    /// An argument that moved is the one thing in a generated signature an
    /// adopter cannot look up: `ref_2` is nowhere in their document, and with
    /// the `builder` feature on it is a setter they have to type. The command
    /// line meets the same collision and answers it — `tree`'s `wire` puts
    /// "sends `ref`" on the flag that moved aside — so a wrapper that said
    /// nothing would be the two consumers disagreeing about whether a rename is
    /// worth mentioning, which is the asymmetry [`Signature::bind`] exists to
    /// end rather than to move.
    ///
    /// `carries` says what the argument is in the document's own terms, because
    /// that is the half the reader is missing: the identifier is in front of
    /// them and the thing it stands for is not.
    ///
    /// Every argument comes through here rather than through [`Signature::bind`],
    /// so an argument added later cannot be given a name without being
    /// accounted for under it.
    fn argument(&mut self, plain: &Ident, word: &str, carries: &str) -> Ident {
        let bound = self.bind(plain.clone(), word);
        if bound != *plain {
            self.notes.push(format!(
                "`{bound}` {carries}, under a name of its own: an argument \
                 declared before it had already spent the plain spelling."
            ));
        }
        bound
    }
}

fn signature_of(
    op: &Operation,
    item: &openapiv3::PathItem,
    operation: &openapiv3::Operation,
    names: &Names,
) -> Result<Signature, GenerateError> {
    let mut out = Signature {
        args: Vec::new(),
        names: Vec::new(),
        builder: Vec::new(),
        notes: Vec::new(),
    };
    for param in op.params() {
        add_param(&mut out, param, item, operation, names)?;
    }
    match op.body() {
        Body::None => {}
        Body::JsonFields(_) | Body::JsonWhole { .. } => {
            let ty = body_type(op, operation, names)?;
            let body = out.argument(&format_ident!("body"), "body", "is the request body");
            out.args.push(quote! { #body: &#ty });
            out.builder.push(quote! { .json(crate::to_json(#body)?) });
        }
        Body::Opaque { media_type, .. } => {
            let body = out.argument(&format_ident!("body"), "body", "is the request body");
            out.notes.push(format!(
                "`{body}` is sent verbatim under the document's own `{media_type}`, \
                 which this crate does not assemble."
            ));
            out.args.push(quote! { #body: Vec<u8> });
            out.builder.push(quote! { .raw(#body) });
        }
        Body::Multipart { names, .. } => {
            let parts = out.argument(&format_ident!("parts"), "parts", "carries the body's parts");
            out.notes.push(multipart_note(&parts, names));
            out.args.push(quote! { #parts: Vec<Part> });
            out.builder.push(quote! { .multipart(#parts) });
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
    names: &Names,
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
    let word = param.name().to_snake_case();
    let plain = ident(&word).ok_or_else(|| {
        unsupported(format!(
            "parameter `{}` has no spelling as a Rust identifier",
            param.name()
        ))
    })?;
    let ident = out.argument(
        &plain,
        &word,
        &format!("sends the document's `{}`", param.name()),
    );
    let schema = param_schema(item, operation, param.name())?;
    let wire = param.name();
    if join.is_some() {
        // A list parameter is the wire name given once per value, which is the
        // repetition a repeated flag reaches the request builder with.
        let ty = list_type(param.name(), schema, names)?;
        out.args.push(quote! { #ident: Vec<#ty> });
        out.builder.push(quote! { .each(#wire, #ident) });
    } else if param.required() {
        let ty = scalar_type(schema, names)?;
        out.args.push(quote! { #ident: #ty });
        out.builder.push(quote! { .param(#wire, #ident) });
    } else {
        let ty = scalar_type(schema, names)?;
        out.args.push(quote! { #ident: Option<#ty> });
        out.builder.push(quote! { .maybe(#wire, #ident) });
    }
    Ok(())
}

fn multipart_note(parts: &Ident, names: &[String]) -> String {
    let assembled = format!("`{parts}` are assembled into a `multipart/form-data` body.");
    if names.is_empty() {
        assembled
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
fn list_type(
    name: &str,
    schema: &ReferenceOr<Schema>,
    names: &Names,
) -> Result<TokenStream, GenerateError> {
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
    scalar_type(&items.clone().unbox(), names)
}

/// A scalar schema's Rust spelling. A `$ref` to a named schema keeps its name,
/// so an enumerated parameter is the generated enum rather than a string.
fn scalar_type(schema: &ReferenceOr<Schema>, names: &Names) -> Result<TokenStream, GenerateError> {
    if let Some(name) = ref_name(schema) {
        return names.get(name).cloned();
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

/// The type of a JSON request body.
///
/// A `$ref` keeps the named schema's type and a schema the operation states
/// inline gets one of its own, so a body the document describes is a body the
/// generated code holds a caller to. `serde_json::Value` is left for the one
/// case that earns it: a JSON body the document states no schema for at all.
fn body_type(
    op: &Operation,
    operation: &openapiv3::Operation,
    names: &Names,
) -> Result<TokenStream, GenerateError> {
    match super::names::json_body(operation) {
        JsonBody::Referenced => Err(unsupported("requestBody $refs are not followed")),
        JsonBody::NotJson => Err(unsupported("no JSON request body")),
        JsonBody::Shapeless => Ok(quote!(serde_json::Value)),
        JsonBody::Stated(schema) => named_or_stated(schema, &Site::body(op.id()), names),
    }
}

/// The type a successful response deserialises into.
fn response_type(
    op: &Operation,
    operation: &openapiv3::Operation,
    names: &Names,
) -> Result<TokenStream, GenerateError> {
    let Some(schema) = super::names::success(operation) else {
        return Ok(quote!(NoContent));
    };
    named_or_stated(schema, &Site::response(op.id()), names)
}

/// The type a schema became, whether the document named it or stated it where
/// it is used.
///
/// A `$ref` is answered from the named schemas so that one pointing nowhere is
/// refused by the reference it names, which is what an adopter can act on.
/// Everything else was converted under [`Site`], and a site with no type would
/// be this emitter and [`names::sites`](super::names::sites) disagreeing about
/// what the document states — which they read through the same two functions,
/// so they do not.
fn named_or_stated(
    schema: &ReferenceOr<Schema>,
    site: &Site,
    names: &Names,
) -> Result<TokenStream, GenerateError> {
    if let Some(name) = ref_name(schema) {
        return names.get(name).cloned();
    }
    names
        .at(site)
        .cloned()
        .ok_or_else(|| unsupported("the schema it states inline has no Rust spelling"))
}

fn ref_name(schema: &ReferenceOr<Schema>) -> Option<&str> {
    match schema {
        ReferenceOr::Reference { reference } => reference.strip_prefix("#/components/schemas/"),
        ReferenceOr::Item(_) => None,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    /// The two halves of [`ident`] answer different questions, and only one of
    /// them refuses a word that is no identifier.
    ///
    /// `accept_as_ident` asks whether a word may stand unquoted, so it says yes
    /// to a word beginning with a digit and yes to the empty string; the parse
    /// beside it is what turns both into `None`. Pinning that here is what keeps
    /// a reader of the keyword arm from taking it for the check — the two look
    /// alike from the call site, and only one of them holds.
    #[test]
    fn a_word_that_is_no_identifier_is_refused_by_the_parse() {
        for word in ["3RdFiring", "3rd_firing", ""] {
            assert!(typify::accept_as_ident(word), "{word:?} stands unquoted");
            assert_eq!(ident(word), None, "{word:?} is no identifier");
        }
        assert_eq!(
            ident("type").map(|i| i.to_string()),
            Some("r#type".to_owned())
        );
        assert_eq!(
            ident("self").map(|i| i.to_string()),
            Some("self_".to_owned())
        );
    }

    /// Two values of one operation that reduce to one Rust word bind two
    /// arguments, and the alternative is built from the document's word rather
    /// than from the raw identifier that spells it.
    #[test]
    fn a_wrapper_binds_each_of_its_arguments_to_a_name_of_its_own() {
        let mut signature = Signature {
            args: Vec::new(),
            names: Vec::new(),
            builder: Vec::new(),
            notes: Vec::new(),
        };
        let bound = |signature: &mut Signature, word: &str| {
            signature
                .bind(ident(word).expect("a word that spells one"), word)
                .to_string()
        };
        assert_eq!(bound(&mut signature, "ref"), "r#ref");
        assert_eq!(bound(&mut signature, "ref"), "ref_2");
        assert_eq!(bound(&mut signature, "ref"), "ref_3");
        assert_eq!(bound(&mut signature, "self"), "self_");
        assert_eq!(bound(&mut signature, "self"), "self_2");
        // The body claims its name after the parameters, so a document that
        // spends `body` on one of them moves the body aside and not the
        // parameter the document named.
        assert_eq!(bound(&mut signature, "body"), "body");
        assert_eq!(bound(&mut signature, "body"), "body_2");
    }
}
