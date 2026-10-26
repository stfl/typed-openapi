//! Reading an OpenAPI document into the model it reduces to.
//!
//! [`Document::load`] takes the two files an adopter ships — the vendor's
//! OpenAPI document and their Overlay of corrections — and hands back a list
//! of [`Operation`]s with every `$ref` already resolved. Nothing from
//! `openapiv3` escapes: once `load` returns, the parsed document is dropped,
//! and what is left is the model its parent module holds, which every build
//! has.
//!
//! The reading is a module rather than a set of gated items among those types
//! because one `#[cfg]` on the module declaration is the whole statement: the
//! feature adds this file whole and removes it whole, which is the shape every
//! feature in this crate has. An attribute per item says the same thing many
//! times over, and leaves a reader checking that no two of them disagree.
//!
//! [`LoadError`] is here because it is a failure of the reading and not of the
//! model — so the feature that brings the one brings the other, and a binary
//! that starts from `Document::from_blob` does not have to know the list
//! exists.

use std::borrow::Cow;

use http::header::HeaderName;
use http::{Method, Uri};
use openapiv3::{
    Components, ObjectType, OpenAPI, Parameter, ParameterData, ParameterSchemaOrContent, PathItem,
    PathStyle, QueryStyle, ReferenceOr, Schema, SchemaKind, Server,
};
use thiserror::Error;

use super::{
    Body, COMMIT, Document, Effect, Field, Gate, Join, List, Location, Operation, Param, Place,
    SPENT, Shape, Unmet, Unsupported, first_collision, gate_on_read, spends, spends_command, unmet,
};
use crate::markers::{COMMAND, GATES, GROUP, REQUIRES, WRITES};
use crate::names::{Clash, CommandName, Grouping, NameError, Namespace, Protected, spelled};
use crate::openapi::Misfit;
use crate::request::BaseUrlError;
use crate::required::{Step, spelled as target};
use crate::scalar::{Items, Scalar, ScalarError};
use crate::schema::{
    RefError, description_of, format_of, is_json, is_media_range, is_media_type, is_multipart,
    lists_only_null, resolve, resolve_schema, scalar_of, template,
};

/// What a refusal calls the request body.
///
/// An operation's values are the parameters and the body, and the document names
/// every one of them but the body — so the body is named by the key it is
/// written under, which is the word an adopter goes looking for.
const BODY: &str = "requestBody";

/// Reducing an OpenAPI document to [`Operation`]s failed.
///
/// Every variant names a way that reading a document goes wrong, so the whole
/// type is bless-time: a binary that starts from [`Document::from_blob`] cannot
/// produce one, does not compile one, and does not have to know the list
/// exists.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LoadError {
    /// The vendor's document could not be read as YAML or JSON. The cause
    /// holds the line and the column of the text where the reader stopped.
    #[error(transparent)]
    Overlay(crate::overlay::OverlayError),
    /// An Overlay layer could not be read or could not be applied. The message
    /// of the cause says which, and holds the line of the text or the index of
    /// the action where there is one.
    #[error("cannot use `overlays[{layer}]`")]
    Layer {
        /// The index of the layer in the `overlays` argument of
        /// [`Document::load`], counted from 0.
        layer: usize,
        /// What stopped the layer.
        #[source]
        source: crate::overlay::OverlayError,
    },
    /// The vendor's document says it is a version of OpenAPI that this crate does
    /// not read. 3.1 spells some rules differently from 3.0 and means others
    /// that 3.0 does not have, so reading it as 3.0 would drop a rule in silence
    /// or refuse a spelling with no place. Support for 3.1 is out of scope, and
    /// no Overlay holds a document to 3.0: one that changes `openapi` is refused.
    #[error("the document says `openapi: {version}`, and this crate reads OpenAPI 3.0 only")]
    Version {
        /// The `openapi` key as the document spells it.
        version: String,
    },
    /// The overlaid document is not an OpenAPI 3 document.
    #[error("the overlaid document is not an OpenAPI 3 document: refused at `{at}`")]
    Shape {
        /// The first node that the OpenAPI model refuses, from the root of the
        /// document: `info.title`, `components.schemas.Voucher`, a path item as
        /// `paths[/vouchers]`, or a node inside a path item as
        /// `paths[/vouchers].get.deprecated`. `.` is the document itself.
        at: String,
        /// `serde_json`'s message.
        #[source]
        source: serde_json::Error,
    },
    /// The document lists no server, so it has no default host to give.
    /// [`Document::base`] is the first server: the default that the command line
    /// and the typed client both start from, and that a caller replaces when the
    /// host is theirs to choose. An Overlay adds the server.
    #[error("the document lists no `servers` entry to send requests to; add one in an Overlay")]
    NoServer,
    /// The first server's URL is not a URL, once each variable in it is replaced
    /// by its default. A variable the server does not declare has no default
    /// and stays in the URL as it is written. A URL whose text with the
    /// defaults in it holds userinfo is [`LoadError::ServerBase`] whether it is
    /// a URL or not, and when the URL is refused the spelling is read too, so
    /// this message prints no credential. A text with an `@` in none of the
    /// places where an authority is read (see [`BaseUrlError::Userinfo`]), such
    /// as `http:/u@h/a b`, is refused here, and the message prints it.
    #[error("`servers[0].url` is not a URL: {}", refused_url(.url, .filled))]
    ServerUrl {
        /// `servers[0].url` as the document spells it.
        url: String,
        /// The URL with the `default` of each declared variable in place of its
        /// name, which is the text that was refused. It is `url` itself where
        /// the URL holds no variable.
        filled: String,
        /// The `http` crate's reason.
        #[source]
        source: http::uri::InvalidUri,
    },
    /// The first server's URL cannot be the base of a request: it holds userinfo,
    /// a query or a fragment, which a request would send, drop or leak. The cause
    /// says which, and neither it nor this message prints the URL, because a
    /// credential or a key would be in it. A URL that holds userinfo is refused
    /// for it also when the text is no URL, whatever stands before the `//` that
    /// begins its authority but a `/`, `?` or `#`, because
    /// [`LoadError::ServerUrl`] prints the URL. An Overlay corrects the URL. A
    /// caller who names the base is held to the same rule when the request is
    /// built.
    #[error("`servers[0].url` cannot be the base URL of a request; correct it in an Overlay")]
    ServerBase {
        /// What the URL holds that a request cannot keep.
        #[source]
        source: BaseUrlError,
    },
    /// An operation without an `operationId`, which is the name every later
    /// refusal and every generated wrapper uses for it. An Overlay adds one.
    #[error("{method} {path} has no operationId; add one in an Overlay")]
    NoOperationId {
        /// The method of the operation, lowercase, as the path item spells it.
        method: String,
        /// The path template that holds the operation.
        path: String,
    },
    /// A `servers` on a path item or on an operation. A request goes to
    /// [`Document::base`], the first server of the document, and the caller
    /// replaces it where the host is theirs to choose; a host that one operation
    /// names for itself (an upload endpoint, say) has no place in the model, so
    /// the request would go to another host than the document means. An Overlay
    /// removes the list.
    ///
    /// A path item's list is refused at an operation under it, which is the one
    /// named. A path item with no operation sends nothing anywhere, and its list
    /// is not refused.
    #[error(
        "{op}: `servers` stands on `{at}`, and this crate sends every request to the first \
         server of the document; remove it in an Overlay"
    )]
    Servers {
        /// The `operationId` of the operation the list applies to.
        op: String,
        /// The list, spelled as an Overlay names a target:
        /// `$.paths['/upload'].post.servers`.
        at: String,
    },
    /// A header parameter that would claim a flag and whose name is no header
    /// name: it holds a space or another character that a header name cannot, so
    /// no request could carry it and the `http` crate would refuse every one that
    /// was built. An Overlay corrects the name or removes the parameter. A
    /// parameter in another place has no header name to be, and one with no flag
    /// sends nothing.
    #[error(
        "{op}: the header parameter `{name}` is not a header name; correct or remove it in an \
         Overlay"
    )]
    HeaderName {
        /// The `operationId` of the operation.
        op: String,
        /// The parameter, as the document spells it.
        name: String,
    },
    /// A variable in the path template that no path parameter of the operation
    /// answers. The request would go out with the braces in it. An Overlay adds
    /// the parameter.
    #[error(
        "{op}: `{path}` has the variable `{{{name}}}`, and the operation declares no path \
         parameter of that name; add one in an Overlay"
    )]
    UnboundVariable {
        /// The `operationId` of the operation.
        op: String,
        /// The path template, as the document spells it.
        path: String,
        /// The variable, without its braces.
        name: String,
    },
    /// A path parameter that no variable of the path template names. Its flag
    /// would be required and its value thrown away. An Overlay removes it.
    #[error(
        "{op}: the path parameter `{name}` has no `{{{name}}}` in `{path}`; remove the parameter \
         in an Overlay"
    )]
    UnusedPathParameter {
        /// The `operationId` of the operation.
        op: String,
        /// The path template, as the document spells it.
        path: String,
        /// The parameter, as the document spells it.
        name: String,
    },
    /// A path parameter that is not `required`. OpenAPI says a path parameter is,
    /// and one that is not would leave its variable in the request when it is
    /// left out. An Overlay sets `required: true`.
    #[error(
        "{op}: the path parameter `{name}` is not `required`, and OpenAPI says a path parameter \
         is; set `required: true` in an Overlay"
    )]
    OptionalPathParameter {
        /// The `operationId` of the operation.
        op: String,
        /// The parameter, as the document spells it.
        name: String,
    },
    /// A group or a command that cannot be spelled as a word on the command
    /// line: a path segment, or the `x-cli-group` or `x-cli-command` that was
    /// meant to replace it. The two markers are the repair.
    #[error(
        "{op}: no command name can be read off this operation; \
         name it with `x-cli-group` and `x-cli-command` in an Overlay"
    )]
    Name {
        /// The `operationId` of the operation that cannot be placed.
        op: String,
        /// The word that cannot be spelled, and what it was read off.
        #[source]
        source: NameError,
    },
    /// A gate in `x-cli-gates` that cannot be spelled as a flag.
    #[error(
        "{op}: a gate in `x-cli-gates` is not a word that can be a flag; correct it in an Overlay"
    )]
    GateName {
        /// The `operationId` of the operation that names the gate.
        op: String,
        /// The word that cannot be spelled.
        #[source]
        source: NameError,
    },
    /// An `x-cli-command` or an `x-cli-group` that is not a string.
    #[error("{op}: `{key}` is not a string")]
    Override {
        /// The `operationId` of the operation that carries the marker.
        op: String,
        /// The marker, `x-cli-command` or `x-cli-group`.
        key: &'static str,
    },
    /// An `x-cli-writes` that is present and is not the boolean `true`. Refused
    /// rather than passed over, for the reason a gate list is: a marker that
    /// reads as no marker leaves a writing `GET` sent on sight, which is the one
    /// outcome a default-closed gate must never reach by accident. `false` is
    /// refused too, because the marker adds a write and never removes one.
    #[error(
        "{op}: `{WRITES}` is not the boolean `true`; \
         it marks a write, so write `true` or leave the marker out"
    )]
    Writes {
        /// The `operationId` of the operation that carries the marker.
        op: String,
    },
    /// A group or a command named `help`. clap declares a `help` subcommand on
    /// every command that has subcommands, and nothing can move it, so an
    /// operation under that name is two subcommands with one spelling. The
    /// markers are the repair, whichever of the two the name is.
    #[error(
        "{op}: the {what} `{name}` is a subcommand that clap declares itself; \
         name it with `x-cli-{what}` in an Overlay"
    )]
    NameSpent {
        /// The `operationId` of the operation under the name.
        op: String,
        /// Which of the two names it is: `group` or `command`.
        what: &'static str,
        /// The name, as the grouping rule or the marker spells it.
        name: String,
    },
    /// Two operations that reduce to one `<group> <command>`.
    #[error(
        "`{first}` and `{second}` are both `{group} {command}` on the command line; \
         give one of them an `x-cli-command`"
    )]
    DuplicateCommand {
        /// The group both operations are mounted under.
        group: CommandName,
        /// The command both operations are mounted under.
        command: CommandName,
        /// The `operationId` of the operation that comes first in the document.
        first: String,
        /// The `operationId` of the later one.
        second: String,
    },
    /// An `x-cli-gates` that is not a list of strings.
    #[error("{op}: `{key}` is not a list of names")]
    GateList {
        /// The `operationId` of the operation that carries the marker.
        op: String,
        /// The marker, `x-cli-gates`.
        key: &'static str,
    },
    /// A gate spelled like one of the six words every subcommand spends: a flag
    /// that carries the body, or `--help`.
    #[error("{op}: the gate `{gate}` is one of the flags every subcommand already spends")]
    ReservedGate {
        /// The `operationId` of the operation that names the gate.
        op: String,
        /// The gate as `x-cli-gates` spells it, after the spelling rule.
        gate: Gate,
    },
    /// A gate spelled like the confirmation. Both are words a person types to
    /// let something happen, and one flag cannot be two of them: answering the
    /// gate would answer the confirmation, which is the one thing a gate is
    /// there to make impossible.
    ///
    /// Either word may move, so the message names both doors.
    #[error(
        "{op}: the gate `{gate}` is the word this CLI confirms with; \
         rename the gate in `x-cli-gates`, or confirm with another word"
    )]
    GateIsTheConfirmation {
        /// The `operationId` of the operation that names the gate.
        op: String,
        /// The gate as `x-cli-gates` spells it, after the spelling rule.
        gate: Gate,
    },
    /// One operation lists the same gate twice.
    #[error("{op}: the gate `{gate}` is named twice")]
    DuplicateGate {
        /// The `operationId` of the operation that names the gate.
        op: String,
        /// The gate as `x-cli-gates` spells it, after the spelling rule.
        gate: Gate,
    },
    /// A document name that wanted the word a gate already spends. Refused
    /// rather than renamed: a flag carrying data must never be able to answer
    /// a gate, and a gate quietly moved aside is a hazard nobody typed.
    ///
    /// The gate's spelling belongs to the adopter's correction layer, so that
    /// is where the way out is — and the message names it, because the person
    /// reading this refusal is the person who wrote the layer.
    #[error(
        "{op}: the {what} `{name}` and the gate `{gate}` both want `--{gate}`; \
         rename the gate in `x-cli-gates`, to `gate-{gate}` or another word"
    )]
    GateTakenByName {
        /// The `operationId` of the operation that has both.
        op: String,
        /// What the document name is: `parameter` or `property`.
        what: &'static str,
        /// The document's name for it, as the document spells it.
        name: String,
        /// The gate's flag, without the leading `--`.
        gate: String,
    },
    /// The same, for the confirmation. Its spelling is the adopter's own —
    /// chosen when the document is loaded — so the way out is to choose
    /// another one rather than to edit the document.
    #[error(
        "{op}: the {what} `{name}` and the confirmation both want `--{commit}`; \
         confirm with another word — `Loading::commit`, or `Settings::commit_word` \
         where a bless step generates"
    )]
    CommitTakenByName {
        /// The `operationId` of the operation that has both.
        op: String,
        /// What the document name is: `parameter` or `property`.
        what: &'static str,
        /// The document's name for it, as the document spells it.
        name: String,
        /// The confirmation's flag, without the leading `--`.
        commit: String,
    },
    /// A read that names a gate is a document saying two things at once: a read
    /// is sent on sight, so there is nothing for the gate to hold back. Refused
    /// rather than mounted, because one of the two statements is a mistake and
    /// the document does not say which.
    #[error(
        "{op}: a read stands behind no gate, and this one names `{gate}`; \
         mark the operation `x-cli-writes: true` or drop the gate"
    )]
    GatedRead {
        /// The `operationId` of the read.
        op: String,
        /// The gate as `x-cli-gates` spells it, after the spelling rule.
        gate: Gate,
    },
    /// A `required` naming a key nothing describes, which is a mistake in the
    /// document — and the document reduced here is the vendor's with the
    /// adopter's Overlays applied, the one that has to be right. Refused rather
    /// than reduced, because what a generator makes of it is a required field
    /// of no stated type: the generated type then refuses the vendor's own
    /// responses, nothing downstream checks a value for the key, and the
    /// request goes out without the one its caller was told to send. The repair
    /// is a judgement about what the vendor meant, and a judgement belongs in
    /// an Overlay a reviewer can read.
    #[error(transparent)]
    Phantom(#[from] crate::required::PhantomKeys),
    /// An `x-cli-` key that this crate reads, standing on an object where it
    /// does not read it: an `x-cli-writes` on a path item, or an
    /// `x-cli-requires` beside a `$ref`. Nothing reads the key there, whether
    /// the object model keeps it (the extensions of a path item) or drops it
    /// (a key beside a `$ref`), so the gate, the name or the requirement the
    /// adopter wrote has no effect, and nothing says so.
    ///
    /// The scan asks the extensions of the objects that OpenAPI 3.0.3 gives
    /// fixed fields, and reads nothing inside a value of the vendor's own: an
    /// `example`, the `parameters` and the `requestBody` of a Link Object, or an
    /// extension holds JSON, and a key spelled like a marker in it is not one.
    /// It reads nothing under a key that 3.0.3 does not define either, such as
    /// `webhooks` or a schema's `$defs`: the model drops those, and nothing
    /// builds a request from them.
    #[error(
        "`{key}` stands on `{at}`, where this crate does not read it; {}",
        crate::markers::reading(.key)
    )]
    MisplacedMarker {
        /// The key, as the document spells it.
        key: String,
        /// The node that holds the key, spelled as an Overlay names a target:
        /// `$.paths['/vouchers/{id}']`.
        at: String,
    },
    /// An `x-cli-` key that this crate reads nowhere. The crate owns the prefix,
    /// so a key under it is either one of the five it reads, spelled wrong, or
    /// an extension that belongs under another prefix.
    #[error(
        "`{key}` stands on `{at}`, and this crate reads no such key; \
         it owns the `x-cli-` prefix and reads {}; \
         spell the key as one of them, or give an extension of your own another prefix",
        crate::markers::vocabulary()
    )]
    UnknownMarker {
        /// The key, as the document spells it.
        key: String,
        /// The node that holds the key, spelled as an Overlay names a target:
        /// `$.paths['/vouchers/{id}'].get`.
        at: String,
    },
    /// A path item that is a `$ref`. This crate follows none, whether the
    /// reference leads anywhere or not, so the item is refused with its path
    /// and the reference it holds, and no cause is given: the reference was
    /// never tried.
    #[error("{path}: the path item is the `$ref` `{reference}`, which this crate does not follow")]
    PathItem {
        /// The path template, as the document spells it.
        path: String,
        /// The reference, as the document spells it.
        reference: String,
    },
    /// A parameter whose `$ref` cannot be followed. There is no parameter yet to
    /// name it by, so the operation that lists it is named.
    #[error("{op}: a parameter is a `$ref` that cannot be followed")]
    ParameterRef {
        /// The `operationId` of the operation that lists the parameter.
        op: String,
        /// The reference that could not be followed.
        #[source]
        source: RefError,
    },
    /// The same failure met under one value of one operation, which is where
    /// almost all of them are met.
    ///
    /// Named rather than reported bare, because the invariant every other
    /// refusal here keeps is that a shape the reduction cannot read is refused
    /// *by name*: a document of a thousand operations is not searchable by the
    /// reference alone, and an adopter meeting one is being asked to go and
    /// correct it in an Overlay.
    #[error("{op}: `{name}` has a `$ref` that cannot be followed")]
    Unresolvable {
        /// The `operationId` of the operation that holds the value.
        op: String,
        /// The value the schema belongs to: a parameter's name, a body
        /// property's name, or `requestBody` for the body as a whole.
        name: String,
        /// The reference that could not be followed.
        #[source]
        source: RefError,
    },
    /// A parameter this CLI cannot supply that the document says a caller must.
    ///
    /// Every other unsupported parameter is carried as [`Shape::Unreachable`]
    /// and costs the document nothing. This one is an operation that could never
    /// be invoked correctly, so it is named here rather than mounted as a
    /// subcommand that is guaranteed to build a request the server refuses.
    ///
    /// The repair is an Overlay, and which one depends on where the parameter
    /// is and why it has no flag. A parameter that is not in the path can drop
    /// its `required`. A path parameter cannot, because
    /// [`LoadError::OptionalPathParameter`] refuses it then, so its shape is
    /// corrected.
    ///
    /// A path parameter is renamed together with its variable in the path when
    /// its name has no spelling, and when its namesake is not in the path: an
    /// Overlay moves the path item to the new path, and the request is the same,
    /// because in the `simple` style, the only one this CLI writes in a path, the
    /// name of a path parameter does not go on the wire. The same rename mends a
    /// parameter off the path whose namesake is a path parameter, with the
    /// namesake renamed.
    ///
    /// A rename gives up no parameter, so the message names it first. Where the
    /// fault also ends when a part of the document is given up, the message names
    /// that after it, because only the adopter knows if that part is the vendor's
    /// mistake: the namesake is removed, which gives this parameter its flag, or
    /// the `required` of a parameter off the path is dropped. A parameter off the
    /// path whose namesake is a path parameter is not told to remove it, because
    /// the template needs it. A rename does not part two parameters that are
    /// both off the path, because both names go on the wire and the request
    /// would change. It does not part two path parameters either, because the
    /// template has one variable for both, and the later one is mended by
    /// removing the earlier.
    ///
    /// A message that names two parameters says which one each repair acts on,
    /// by its place and its name, and by "earlier" and "later" where the place
    /// is the same. A pronoun would read as either of them, and the `required`
    /// of the other one is no repair.
    #[error(
        "{op}: parameter `{name}` is {why}, and the document requires it; {}",
        repair(*.place, .name, .why)
    )]
    Parameter {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter's name, as the document spells it.
        name: String,
        /// Where the document puts the parameter.
        place: Place,
        /// Why this crate has no flag for it.
        why: Unsupported,
    },
    /// A `requestBody` whose `content` key names no media type — `form-data`
    /// where `multipart/form-data` was meant — or holds a byte that no header
    /// carries, a control character in a parameter. Refused while the document
    /// is reduced, because the alternative is a request sent under a
    /// `Content-Type` no server can read, or no request at all. The message
    /// prints a control character as its escape, so a key that looks like a
    /// media type is not refused with nothing to show why.
    #[error(
        "{op}: `{}` is not a media type; \
         an Overlay is where a document's content type is corrected",
        .media_type.escape_debug()
    )]
    MediaType {
        /// The `operationId` of the operation that declares the body.
        op: String,
        /// The `content` key as the document spells it.
        media_type: String,
    },
    /// A `requestBody` whose `content` key is a media range — `*/*` or `image/*`
    /// — and is the key the operation would be sent under. Refused while the
    /// document is reduced, because a range names a set of types and a request
    /// goes out under one, so the alternative is a `Content-Type` that is the
    /// range itself.
    #[error(
        "{op}: `{media_type}` is a media range, and a body is sent under one media type; \
         an Overlay is where a document's content type is corrected"
    )]
    MediaRange {
        /// The `operationId` of the operation that declares the body.
        op: String,
        /// The `content` key as the document spells it.
        media_type: String,
    },
    /// A rule the document states about a value that cannot be run as the
    /// document means it: a `pattern` that no regex engine here reads, a
    /// `pattern` that holds `\p{`, `\P{` or `\u{`, a `multipleOf` that is not more
    /// than 0, or an `enum` that lists `null` and no other value. The value is one
    /// that `runnable_scalar` reads, with a flag or without one: a parameter sent
    /// in the path, the query or a header with a `schema` (its items, for a list),
    /// or a property of a JSON body whose schema is an object. Refused while the
    /// document is reduced, because a flag would refuse every value, admit every
    /// value, or read a rule that the vendor's server reads another way, and
    /// refused for a value that gets no flag too, so that the verdict does not
    /// depend on its style, its name or its siblings.
    #[error("{op}: `{name}` states a rule that cannot run")]
    Unrunnable {
        /// The `operationId` of the operation that states the rule.
        op: String,
        /// The parameter or body property the rule is stated about, as the
        /// document spells it.
        name: String,
        /// The rule that cannot run: the engine's reason for a `pattern` it
        /// cannot read, the `pattern` and the escape it holds, the step, or the
        /// `null` that an `enum` lists alone.
        #[source]
        source: crate::scalar::ScalarError,
    },
    /// A `pattern` of a schema that holds `\p{`, `\P{` or `\u{`, where no scalar
    /// that `runnable_scalar` reads carries it: a response, a named schema, the
    /// items of an array body, a property of a nested object, a cookie
    /// parameter, or a value whose scalar keeps its `enum` and drops the
    /// `pattern`. `Unrunnable` is the same refusal for the `pattern` that such a
    /// scalar carries. It means one thing with the Unicode flag and another
    /// without it, and the generated type cannot be given the flag, so the type
    /// refuses the vendor's own responses that a server running the pattern with
    /// the flag accepts. The walk reads every schema that the object model
    /// reads, and the model reads nothing beside a `$ref` that is text, in an
    /// object that can be a reference.
    #[error("{}`{at}` states a rule that cannot run", crate::patterns::held_by(.op.as_deref()))]
    UnrunnablePattern {
        /// The `operationId` of the operation the schema stands in, and `None`
        /// where it stands in no operation: a component, or a parameter that a
        /// path item shares. One schema can serve many operations, so a
        /// component names none, and `at` is where the repair is made.
        op: Option<String>,
        /// The schema, spelled as an Overlay names a target:
        /// `$.components.schemas.Person.properties.surname`.
        at: String,
        /// The `pattern` and the escape it holds.
        #[source]
        source: crate::scalar::ScalarError,
    },
    /// An `x-cli-requires` that is present and is not a list of names. Refused
    /// rather than passed over, for the reason a gate list is: one name written
    /// where a list goes would otherwise be no requirement at all.
    #[error("{op}: `{REQUIRES}` on `{name}` is not a list of parameter names")]
    RequiresList {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter that carries the `x-cli-requires`, as the document
        /// spells it.
        name: String,
    },
    /// A parameter whose `x-cli-requires` lists the parameter itself. It asks
    /// for nothing, and clap refuses an argument that requires itself, so the
    /// command line would panic at the first start of a debug build.
    #[error("{op}: `{name}` lists itself in `{REQUIRES}`; list only the other parameters it needs")]
    RequiresItself {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter that lists itself, as the document spells it.
        name: String,
    },
    /// A parameter that requires a name the operation declares no parameter
    /// under. Every run giving it would be refused, and the spelling that was
    /// meant is the adopter's to correct.
    #[error("{op}: `{name}` requires `{requires}`, which is not a parameter of this operation")]
    RequiresUnknown {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter that carries the `x-cli-requires`, as the document
        /// spells it.
        name: String,
        /// The name `x-cli-requires` lists that no parameter has.
        requires: String,
    },
    /// A parameter that requires one this crate cannot put in a request, so no
    /// request giving the first could ever be built.
    #[error("{op}: `{name}` requires `{requires}`, and `{requires}` is {why}")]
    RequiresUnreachable {
        /// The `operationId` of the operation that declares the parameter.
        op: String,
        /// The parameter that carries the `x-cli-requires`, as the document
        /// spells it.
        name: String,
        /// The listed parameter that has no flag.
        requires: String,
        /// Why this crate has no flag for it.
        why: Unsupported,
    },
}

/// The repair that [`LoadError::Parameter`] names, and only a repair that loads
/// the document when the parameter is the one fault in it. A document with more
/// than one fault is refused for one at a time, and each repair ends its own
/// refusal without bringing back one that an earlier repair ended.
///
/// One arm for each case of the place and the reason. A repair that keeps every
/// parameter the operation declares comes first. A repair that ends the fault by
/// giving up a part of the document follows it, because only the adopter knows
/// if that part is the vendor's mistake.
/// - Dropping `required` mends a parameter off the path only: for a path
///   parameter it gives [`LoadError::OptionalPathParameter`]. It gives up the
///   requirement.
/// - A path parameter is renamed together with its variable ([`rename`]) when
///   its name has no spelling, and when its namesake is not in the path. A
///   parameter off the path whose namesake is a path parameter is mended by the
///   same rename of the namesake. The rename gives up nothing: the name of a
///   path parameter does not go on the wire.
/// - A name that another parameter has is also mended by removing that
///   parameter, which gives this one its flag and gives the namesake up. The
///   exception is a parameter off the path whose namesake is a path parameter:
///   the template names the namesake, and without it the document is refused for
///   the variable ([`LoadError::UnboundVariable`]).
/// - Two path parameters of one name are not parted by a rename, because the
///   template has one variable for both. Two parameters off the path are not
///   either, because both names go on the wire and a rename would change the
///   request.
/// - A repair that names the `required` of a parameter says whose it is
///   ([`parameter`]), because the `required` of the namesake is no repair.
fn repair(place: Place, name: &str, why: &Unsupported) -> String {
    match (place, why) {
        (Place::Path, Unsupported::Shared(Place::Path)) => {
            format!("remove {} in an Overlay", parameter(Place::Path, name))
        }
        (Place::Path, Unsupported::Shared(other)) => {
            format!(
                "{}, or remove {} in an Overlay",
                rename(&parameter(Place::Path, name)),
                parameter(*other, name)
            )
        }
        (Place::Path, Unsupported::Unspellable) => rename("the parameter"),
        (
            Place::Path,
            Unsupported::Cookie
            | Unsupported::Encoded
            | Unsupported::Structured
            | Unsupported::Style(_),
        ) => "correct the parameter in an Overlay".to_owned(),
        (_, Unsupported::Shared(Place::Path)) => {
            format!(
                "{}, or drop the `required` of {} in an Overlay",
                rename(&parameter(Place::Path, name)),
                parameter(place, name)
            )
        }
        (_, Unsupported::Shared(other)) if *other == place => format!(
            "remove the earlier `{place}` parameter `{name}` in an Overlay, or drop the \
             `required` of the later one"
        ),
        (_, Unsupported::Shared(other)) => {
            format!(
                "remove {} in an Overlay, or drop the `required` of {}",
                parameter(*other, name),
                parameter(place, name)
            )
        }
        (
            _,
            Unsupported::Cookie
            | Unsupported::Encoded
            | Unsupported::Structured
            | Unsupported::Style(_)
            | Unsupported::Unspellable,
        ) => "correct the parameter in an Overlay, or drop its `required`".to_owned(),
    }
}

/// A parameter as a repair names it, by its place and its name. A message about
/// a shared name holds two parameters of one name, so a pronoun would read as
/// either of them; the place says which one a repair acts on.
fn parameter(place: Place, name: &str) -> String {
    format!("the `{place}` parameter `{name}`")
}

/// The repair that renames a path parameter together with its variable. The
/// template names the parameter, so no `update` of the parameter alone loads,
/// and no Overlay action renames a key: the path item is written again under the
/// new path and the old one is removed. Every message that names the rename uses
/// these words, and `parameter` says which one is renamed.
fn rename(parameter: &str) -> String {
    format!(
        "rename {parameter} and its variable in the path together, in an Overlay that writes \
         the path item again under the new path and removes the old one"
    )
}

/// The first server's URL as a refusal shows it: as the document spells it, and
/// where a variable was filled in, the text that was refused beside it.
fn refused_url(url: &str, filled: &str) -> String {
    if url == filled {
        format!("`{url}`")
    } else {
        format!("`{url}`, which is `{filled}` with the default of each variable")
    }
}

/// Why a word cannot be the confirmation.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ConfirmationError {
    /// Not a name a person could type as a flag.
    #[error(transparent)]
    Spelling(#[from] NameError),
    /// A name some other flag on the same subcommand already wears. The others
    /// cannot move — a body has to be reachable and `--help` is clap's — so
    /// the confirmation is what gives way.
    #[error(
        "`{word}` is a flag every subcommand already declares, \
         so it cannot also be the word this CLI confirms with"
    )]
    Taken {
        /// The word both of them wanted.
        word: String,
    },
}

/// What the CLI brings to a reduction, as against what the document brings.
///
/// A type rather than a bare argument, so that a caller reading
/// `Loading::new().commit("yes")?` at the call site can see which of the two
/// the word belongs to.
#[derive(Debug, Clone)]
pub struct Loading {
    commit: String,
}

impl Default for Loading {
    fn default() -> Self {
        Self::new()
    }
}

impl Loading {
    /// The defaults: `--commit` confirms a write.
    #[must_use]
    pub fn new() -> Self {
        Self {
            commit: COMMIT.to_owned(),
        }
    }

    /// Confirm writes with this word instead, spelled without the `--`.
    ///
    /// Held to the one spelling rule every flag is held to, and refused rather
    /// than mangled: a confirmation somebody cannot type is a confirmation
    /// nobody gives.
    ///
    /// Refused too where the word is one a subcommand already declares — a
    /// flag that carries the body, or the `--help` clap writes itself. Those
    /// cannot move, so a confirmation wearing one of their names is two flags
    /// with one spelling, which clap resolves at the user's startup and not at
    /// the adopter's bless. The same words are refused for a gate, in
    /// `gates_of`, and for the same reason.
    pub fn commit(mut self, word: &str) -> Result<Self, ConfirmationError> {
        let word = spelled("confirmation", word)?;
        if spends(&word) {
            return Err(ConfirmationError::Taken { word });
        }
        self.commit = word;
        Ok(self)
    }
}

impl Document {
    /// Parse the vendor's document, lay the adopter's Overlays over it in
    /// order, and resolve the result into operations.
    ///
    /// Every argument is a file's contents, YAML or JSON. `overlays` is a list
    /// because corrections come in layers — each one corrects the document the
    /// ones before it produced, so the order they are given in is the order
    /// they happen. An empty list runs a document that is already corrected.
    ///
    /// The document is OpenAPI 3.0. The vendor's `openapi` is asked first, before
    /// any Overlay, and any version but `3.0` and `3.0.x` is
    /// [`LoadError::Version`]. An Overlay that changes `openapi` is refused at
    /// its layer.
    ///
    /// This is the expensive door, and the `document` feature is what opens it.
    /// A bless step calls it once and writes [`Document::to_blob`] beside the
    /// rest of what it generates; a shipped binary compiles without the feature
    /// and reaches the same reduction through [`Document::from_blob`].
    ///
    /// ```
    /// use typed_openapi::{Document, Invocation, Values};
    ///
    /// let doc = Document::load(
    ///     include_str!("../../tests/fixtures/toy.yaml"),
    ///     &[
    ///         include_str!("../../tests/fixtures/corrections.yaml"),
    ///         include_str!("../../tests/fixtures/cli.yaml"),
    ///     ],
    /// )?;
    /// let op = doc.get("getVoucher").expect("the document describes it");
    /// let request = Invocation::new(op, Values::new().param("id", 5))?.request(doc.base())?;
    /// assert_eq!(request.uri().path(), "/vouchers/5");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn load(document: &str, overlays: &[&str]) -> Result<Self, LoadError> {
        Self::load_with(document, overlays, &Loading::new())
    }

    /// The same, with the CLI's own words chosen rather than defaulted.
    ///
    /// The one word this door adds is the confirmation. It is settable because
    /// it is this crate's and not the vendor's: a document whose own schema
    /// declares a property called `commit` has every right to, and the way out
    /// is for the CLI to say yes in a different word rather than for the
    /// document's property to be renamed behind its author's back.
    ///
    /// ```
    /// use typed_openapi::{Document, Loading};
    ///
    /// let doc = Document::load_with(
    ///     include_str!("../../tests/fixtures/toy.yaml"),
    ///     &[
    ///         include_str!("../../tests/fixtures/corrections.yaml"),
    ///         include_str!("../../tests/fixtures/cli.yaml"),
    ///     ],
    ///     &Loading::new().commit("yes")?,
    /// )?;
    /// let op = doc
    ///     .get("enshrineVoucher")
    ///     .expect("the document describes it");
    /// assert_eq!(op.commit(), "yes");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn load_with(
        document: &str,
        overlays: &[&str],
        loading: &Loading,
    ) -> Result<Self, LoadError> {
        let mut doc = crate::overlay::parse(document).map_err(LoadError::Overlay)?;
        // The version first, and the vendor's: a document that is not 3.0 is
        // refused for what it is, and not for the first spelling of it that the
        // readers below trip on, or worse, read as 3.0 and drop a rule from. No
        // layer changes it (`overlay::apply` refuses that), so the version that
        // is read is the one on the vendor's first line.
        crate::openapi::require_3_0(&doc)?;
        for (layer, overlay) in overlays.iter().enumerate() {
            doc = crate::overlay::apply(doc, overlay)
                .map_err(|source| LoadError::Layer { layer, source })?;
        }
        // Against the corrected document, so that an adopter's own Overlay is
        // what repairs a contradiction, and while it is still the text the
        // vendor wrote: the object model below normalises away shapes — an
        // inline body's schema among them — that this reading needs to see.
        crate::required::check(&doc)?;
        // On the raw document too, because the object model drops a marker
        // standing beside a `$ref` without a word.
        crate::markers::check(&doc)?;
        let model = crate::openapi::model_of(&doc)
            .map_err(|Misfit { at, source }| LoadError::Shape { at, source })?;
        let loaded = Self::from_openapi(&model, &loading.commit)?;
        // After the reduction, so that a `pattern` which `runnable_scalar` read
        // is refused there, with the value that holds it. This reads the
        // `pattern` of every other schema the model reads.
        crate::patterns::check(&doc)?;
        Ok(loaded)
    }

    fn from_openapi(doc: &OpenAPI, commit: &str) -> Result<Self, LoadError> {
        let base = base_of(doc.servers.first().ok_or(LoadError::NoServer)?)?;
        let empty = Components::default();
        // Which segment groups this document is a fact about all of its paths,
        // so it is read once and then applied one path at a time.
        let paths: Vec<&str> = doc.paths.paths.keys().map(String::as_str).collect();
        let whole = Reading {
            components: doc.components.as_ref().unwrap_or(&empty),
            grouping: Grouping::of(&paths),
            commit,
        };

        let mut ops: Vec<Operation> = Vec::new();
        for (path, item) in &doc.paths.paths {
            let item = match item {
                ReferenceOr::Item(item) => item,
                ReferenceOr::Reference { reference } => {
                    return Err(LoadError::PathItem {
                        path: path.clone(),
                        reference: reference.clone(),
                    });
                }
            };
            for (method, op) in item.iter() {
                ops.push(Operation::build(path, method, item, op, whole)?);
            }
        }
        if let Some(collision) = collision(&ops) {
            return Err(collision);
        }
        Ok(Self {
            base,
            commit: commit.to_owned(),
            ops,
        })
    }
}

/// The base of every request: the URL of the first server, once it is a URL that
/// a request can be built from.
///
/// The userinfo is read from the text before it is parsed, so a URL that holds
/// userinfo and is no URL is refused for the userinfo, and `ServerUrl` prints a
/// URL that holds none.
fn base_of(server: &Server) -> Result<Uri, LoadError> {
    let with_defaults = filled(server);
    BaseUrlError::userinfo_in(&with_defaults).map_err(|source| LoadError::ServerBase { source })?;
    let base = Uri::try_from(with_defaults.as_str())
        .map_err(|source| unparsed(server, with_defaults.clone(), source))?;
    BaseUrlError::of_server(&with_defaults, &base)
        .map_err(|source| LoadError::ServerBase { source })?;
    Ok(base)
}

/// The refusal of a first server whose URL does not parse: `ServerBase` when it
/// holds userinfo, and `ServerUrl` when it does not.
///
/// `ServerUrl` prints the URL as the document spells it beside the one with the
/// defaults in it, and a default that holds a `/` can move the `@` out of the
/// authority of the one and not of the other. So the spelling is read too, once
/// the filled text is refused. A filled text that parses is the URL a request
/// goes to, and the spelling is not read then: in `http://{h}alice:s3cret@host/`
/// with `h` defaulting to `x/` the authority is `x`.
fn unparsed(server: &Server, with_defaults: String, source: http::uri::InvalidUri) -> LoadError {
    match BaseUrlError::userinfo_in(&server.url) {
        Err(userinfo) => LoadError::ServerBase { source: userinfo },
        Ok(()) => LoadError::ServerUrl {
            url: server.url.clone(),
            filled: with_defaults,
            source,
        },
    }
}

/// The server's URL with each `{name}` in it replaced by the `default` of the
/// variable that `variables` declares under that name.
///
/// OpenAPI requires a default for every variable, and it is the value the
/// document says to send when the caller supplies none. A name that `variables`
/// does not declare has no default, so it stays as it is written and the URL
/// that results is not one.
fn filled(server: &Server) -> String {
    let mut url = String::with_capacity(server.url.len());
    let mut rest = server.url.as_str();
    loop {
        let Some((before, after)) = rest.split_once('{') else {
            url.push_str(rest);
            return url;
        };
        url.push_str(before);
        let Some((name, tail)) = after.split_once('}') else {
            url.push('{');
            url.push_str(after);
            return url;
        };
        let declared = server.variables.as_ref().and_then(|all| all.get(name));
        if let Some(variable) = declared {
            url.push_str(&variable.default);
        } else {
            url.push('{');
            url.push_str(name);
            url.push('}');
        }
        rest = tail;
    }
}

impl Operation {
    fn build(
        path: &str,
        method: &str,
        item: &PathItem,
        op: &openapiv3::Operation,
        whole: Reading<'_>,
    ) -> Result<Self, LoadError> {
        let id = op
            .operation_id
            .as_deref()
            .ok_or_else(|| LoadError::NoOperationId {
                method: method.to_owned(),
                path: path.to_owned(),
            })?;
        no_servers(id, path, method, item, op)?;
        let method = method_of(method);
        let (group, command) = placement(op, id, path, &method, whole.grouping)?;
        let effect = effect_of(&method, op, id)?;
        // The gates are read before the namespace exists, because their flags
        // belong in it: a body field the vendor happens to spell `enshrine`
        // moves aside rather than shadowing the word that stands in front of
        // the hazard.
        let gates = gates_of(op, id, effect, whole.commit)?;

        // One namespace per subcommand. The words that carry consent — this
        // operation's gates and the confirmation — are guarded, so a document
        // name that wants one refuses the load instead of pushing it aside.
        // The flags that carry the body and `--help` are merely spent, and
        // move over.
        //
        // The confirmation is guarded on a write and nowhere else, because a
        // read declares no confirmation to clash with: a document is free to
        // call a query parameter `commit`, and an operation sent on sight has
        // no word standing in front of it. A gate is refused on a read
        // earlier still, and for the same reason.
        let confirmation = (effect == Effect::Write).then_some((whole.commit, Protected::Commit));
        let mut flags = Namespace::guarding(
            gates
                .iter()
                .map(|gate| (gate.as_str(), Protected::Gate))
                .chain(confirmation),
            SPENT,
        );
        let declared =
            declared(item, op, whole.components).map_err(|source| LoadError::ParameterRef {
                op: id.to_owned(),
                source,
            })?;
        reconcile(id, path, &declared)?;
        let params = declared
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let earlier = declared.get(..index).unwrap_or_default();
                Param::build(
                    id,
                    param,
                    namesake(earlier, param),
                    whole.components,
                    &mut flags,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        companions(id, &params)?;
        let body = Body::build(id, op, whole.components, &mut flags)?;

        Ok(Self {
            id: id.to_owned(),
            group,
            command,
            commit: whole.commit.to_owned(),
            method,
            path: path.to_owned(),
            summary: op.summary.clone(),
            description: op.description.clone(),
            params,
            body,
            effect,
            gates,
        })
    }
}

/// Refuse a `servers` that applies to this operation, on its path item or on
/// itself, in that order.
fn no_servers(
    id: &str,
    path: &str,
    method: &str,
    item: &PathItem,
    op: &openapiv3::Operation,
) -> Result<(), LoadError> {
    let on_item = [Step::Key("paths"), Step::Key(path), Step::Key("servers")];
    let on_operation = [
        Step::Key("paths"),
        Step::Key(path),
        Step::Key(method),
        Step::Key("servers"),
    ];
    let stood_on = if !item.servers.is_empty() {
        &on_item[..]
    } else if !op.servers.is_empty() {
        &on_operation[..]
    } else {
        return Ok(());
    };
    Err(LoadError::Servers {
        op: id.to_owned(),
        at: target(stood_on),
    })
}

/// The header names OpenAPI 3.0.3 says a parameter definition "SHALL be
/// ignored" for, since a header of the request is set by the media type of the
/// body, by what the caller accepts and by the security scheme.
const IGNORED: [&str; 3] = ["Accept", "Content-Type", "Authorization"];

/// Whether OpenAPI says to ignore this parameter. The comparison is without
/// case, as the names of HTTP headers are, and only a header is ignored.
fn ignored(param: &Parameter) -> bool {
    matches!(param, Parameter::Header { parameter_data, .. }
        if IGNORED.iter().any(|name| name.eq_ignore_ascii_case(&parameter_data.name)))
}

/// Where the document puts a parameter. The only map from the four kinds of
/// `openapiv3::Parameter` to [`Place`].
fn place(param: &Parameter) -> Place {
    match param {
        Parameter::Query { .. } => Place::Query,
        Parameter::Header { .. } => Place::Header,
        Parameter::Path { .. } => Place::Path,
        Parameter::Cookie { .. } => Place::Cookie,
    }
}

/// A slot of the merged list of parameters, and whose entry it holds.
enum Slot<'d> {
    /// The path item's. An entry of the operation with the same name and place
    /// takes the slot.
    Inherited(&'d Parameter),
    /// The operation's own, which nothing takes the slot from: a second entry of
    /// the operation with one name and place is another parameter.
    Own(&'d Parameter),
}

impl<'d> Slot<'d> {
    fn parameter(&self) -> &'d Parameter {
        match self {
            Self::Inherited(param) | Self::Own(param) => param,
        }
    }
}

/// The parameters one operation takes: the path item's, in the order it lists
/// them, and then the operation's own.
///
/// A parameter is identified by its name and its place, and the operation's
/// entry takes the slot of the path item's entry of the same pair, as OpenAPI
/// says, so the order the path item lists them in stays, and the operation's
/// other entries follow in the order it lists them. This is the one order the
/// document fixes: `openapiv3` keeps none between a path item's `parameters` and
/// its methods. Nothing else merges: the same name in another place is another
/// parameter, which [`namesake`] deals with. The ignored headers are not
/// parameters of the operation, so they are left out here and neither refuse a
/// document nor take a name from another parameter.
///
/// The generator reads the same list, so a wrapper's argument is typed by the
/// entry that won.
pub(crate) fn declared<'d>(
    item: &'d PathItem,
    op: &'d openapiv3::Operation,
    components: &'d Components,
) -> Result<Vec<&'d Parameter>, RefError> {
    let followed = |list: &'d [ReferenceOr<Parameter>]| {
        list.iter()
            .map(|param| resolve(param, |key| components.parameters.get(key), "parameters"))
            .collect::<Result<Vec<&'d Parameter>, _>>()
    };
    let same = |a: &Parameter, b: &Parameter| {
        a.parameter_data_ref().name == b.parameter_data_ref().name && place(a) == place(b)
    };
    let mut merged: Vec<Slot<'d>> = followed(&item.parameters)?
        .into_iter()
        .map(Slot::Inherited)
        .collect();
    for over in followed(&op.parameters)? {
        match merged
            .iter_mut()
            .find(|slot| matches!(slot, Slot::Inherited(param) if same(param, over)))
        {
            Some(slot) => *slot = Slot::Own(over),
            None => merged.push(Slot::Own(over)),
        }
    }
    Ok(merged
        .iter()
        .map(Slot::parameter)
        .filter(|param| !ignored(param))
        .collect())
}

/// The entry of `declared` that the flag of this name was read from: the first
/// entry with the name.
///
/// The first entry keeps the name even when it has no flag itself, and every
/// later entry with the name has none (`namesake`), so a flag is always the
/// first entry's and the place of the flag needs no asking.
#[cfg(feature = "generate")]
pub(crate) fn entry_of_flag<'d>(declared: &[&'d Parameter], name: &str) -> Option<&'d Parameter> {
    declared
        .iter()
        .copied()
        .find(|param| param.parameter_data_ref().name == name)
}

/// The variables of a path template, in the order the path writes them. A `{`
/// that nothing closes is text, not a variable.
fn variables(path: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = path;
    while let Some((_, after)) = rest.split_once('{') {
        let Some((name, tail)) = after.split_once('}') else {
            break;
        };
        found.push(name);
        rest = tail;
    }
    found
}

/// Hold the path template and the path parameters to each other: every variable
/// has a path parameter, every path parameter has a variable, and every path
/// parameter is required. Each break of that is a request that goes out wrong,
/// with `{id}` in it or with a value thrown away.
fn reconcile(id: &str, path: &str, params: &[&Parameter]) -> Result<(), LoadError> {
    let variables = variables(path);
    let in_path: Vec<&ParameterData> = params
        .iter()
        .filter(|param| matches!(param, Parameter::Path { .. }))
        .map(|param| param.parameter_data_ref())
        .collect();
    if let Some(name) = variables
        .iter()
        .find(|name| !in_path.iter().any(|data| data.name == **name))
    {
        return Err(LoadError::UnboundVariable {
            op: id.to_owned(),
            path: path.to_owned(),
            name: (*name).to_owned(),
        });
    }
    for data in in_path {
        if !variables.contains(&data.name.as_str()) {
            return Err(LoadError::UnusedPathParameter {
                op: id.to_owned(),
                path: path.to_owned(),
                name: data.name.clone(),
            });
        }
        if !data.required {
            return Err(LoadError::OptionalPathParameter {
                op: id.to_owned(),
                name: data.name.clone(),
            });
        }
    }
    Ok(())
}

/// The place of the first of the `earlier` parameters that has this one's name.
///
/// A value reaches a parameter by its name alone, so the second parameter with
/// a name could never be given one: [`Operation::param`] finds the first.
fn namesake(earlier: &[&Parameter], param: &Parameter) -> Option<Place> {
    earlier
        .iter()
        .copied()
        .find(|other| other.parameter_data_ref().name == param.parameter_data_ref().name)
        .map(place)
}

/// What the whole document supplies while one of its operations is read: the
/// schemas every `$ref` resolves against, and the rule that places operations
/// in the command tree. Both are facts about the document rather than about
/// the operation, so both are read once and handed down.
#[derive(Debug, Clone, Copy)]
struct Reading<'d> {
    components: &'d Components,
    grouping: Grouping,
    /// The word this CLI spends on confirming a write. Read once here and then
    /// written onto the document and onto every operation: a subcommand is
    /// built from one operation with no document in reach, so the copy is what
    /// lets `tree` declare the flag the adopter chose.
    commit: &'d str,
}

/// Where an operation sits in the command tree: the grouping rule, with the
/// document's own overrides over it.
///
/// `x-cli-group` and `x-cli-command` are the adopter's say over a name a path
/// spells badly, and the only way out of a collision — so they are read here,
/// where the name is decided, and nowhere else.
fn placement(
    op: &openapiv3::Operation,
    id: &str,
    path: &str,
    method: &Method,
    grouping: Grouping,
) -> Result<(CommandName, CommandName), LoadError> {
    let unplaced = |source| LoadError::Name {
        op: id.to_owned(),
        source,
    };
    let group = match named(op, id, GROUP)? {
        Some(raw) => CommandName::new(GROUP, raw),
        None => grouping.group(path),
    }
    .map_err(unplaced)?;
    let command = match named(op, id, COMMAND)? {
        Some(raw) => CommandName::new(COMMAND, raw),
        None => grouping.leaf(path, method),
    }
    .map_err(unplaced)?;
    for (what, name) in [("group", &group), ("command", &command)] {
        if spends_command(name.as_str()) {
            return Err(LoadError::NameSpent {
                op: id.to_owned(),
                what,
                name: name.as_str().to_owned(),
            });
        }
    }
    Ok((group, command))
}

/// One `x-cli-` name the document offers, if it offers one.
///
/// A marker that is present and is not a string is the document saying
/// something this crate has no reading for, and is refused rather than passed
/// over — an adopter who writes a list where a name goes would otherwise get
/// the name they were overriding.
fn named<'o>(
    op: &'o openapiv3::Operation,
    id: &str,
    key: &'static str,
) -> Result<Option<&'o str>, LoadError> {
    match op.extensions.get(key) {
        None => Ok(None),
        Some(serde_json::Value::String(raw)) => Ok(Some(raw)),
        Some(_) => Err(LoadError::Override {
            op: id.to_owned(),
            key,
        }),
    }
}

/// The gates one operation stands behind, in the order the document names them.
///
/// Every rule about a gate is run here, while the document is reduced: a name
/// that is not a flag, a name the command line has already spent, a name given
/// twice, and a gate on an operation that is sent on sight. All four are an
/// adopter's mistake, and all four are refused at their expense rather than at
/// a user's — a gate that reaches a shipped binary is a gate somebody is about
/// to type.
fn gates_of(
    op: &openapiv3::Operation,
    id: &str,
    effect: Effect,
    commit: &str,
) -> Result<Vec<Gate>, LoadError> {
    let mut gates: Vec<Gate> = Vec::new();
    for raw in listed(op, id, GATES)? {
        let gate = Gate::new(GATES, raw).map_err(|source| LoadError::GateName {
            op: id.to_owned(),
            source,
        })?;
        if gate.as_str() == commit {
            return Err(LoadError::GateIsTheConfirmation {
                op: id.to_owned(),
                gate,
            });
        }
        if spends(gate.as_str()) {
            return Err(LoadError::ReservedGate {
                op: id.to_owned(),
                gate,
            });
        }
        if gates.contains(&gate) {
            return Err(LoadError::DuplicateGate {
                op: id.to_owned(),
                gate,
            });
        }
        gates.push(gate);
    }
    match gate_on_read(effect, &gates) {
        Some(gate) => Err(LoadError::GatedRead {
            op: id.to_owned(),
            gate: gate.clone(),
        }),
        None => Ok(gates),
    }
}

/// The names one list-valued `x-cli-` marker offers, if it offers any.
///
/// A marker that is present and is not a list of names is the document saying
/// something this crate has no reading for, and is refused rather than passed
/// over — the same reading [`named`] gives a marker that should have been one
/// name, for the same reason: an adopter who writes one word where a list goes
/// would otherwise get no gate at all.
fn listed<'o>(
    op: &'o openapiv3::Operation,
    id: &str,
    key: &'static str,
) -> Result<Vec<&'o str>, LoadError> {
    let reject = || LoadError::GateList {
        op: id.to_owned(),
        key,
    };
    match op.extensions.get(key) {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Array(names)) => names
            .iter()
            .map(|name| name.as_str().ok_or_else(reject))
            .collect(),
        Some(_) => Err(reject()),
    }
}

/// Two operations under one `<group> <command>` would silently shadow each
/// other, so the document is refused instead — never resolved by renaming one
/// of them, which would move a name nobody asked to move.
fn collision(ops: &[Operation]) -> Option<LoadError> {
    first_collision(ops).map(|(first, second)| LoadError::DuplicateCommand {
        group: first.group.clone(),
        command: first.command.clone(),
        first: first.id.clone(),
        second: second.id.clone(),
    })
}

/// `PathItem::iter` yields only the eight methods OpenAPI names, lowercase.
fn method_of(name: &str) -> Method {
    match name {
        "put" => Method::PUT,
        "post" => Method::POST,
        "delete" => Method::DELETE,
        "options" => Method::OPTIONS,
        "head" => Method::HEAD,
        "patch" => Method::PATCH,
        "trace" => Method::TRACE,
        _ => Method::GET,
    }
}

/// A GET the document marks as writing is a write; so is anything but a safe
/// method. Default-closed: the marker can only add writes, never remove them.
///
/// A marker that is present and is not `true` is refused, because the one
/// reading that is left for it is no marker at all, and that sends a writing
/// `GET` on sight.
fn effect_of(method: &Method, op: &openapiv3::Operation, id: &str) -> Result<Effect, LoadError> {
    match op.extensions.get(WRITES) {
        None => Ok(Effect::of_method(method)),
        Some(serde_json::Value::Bool(true)) => Ok(Effect::Write),
        Some(_) => Err(LoadError::Writes { op: id.to_owned() }),
    }
}

impl Param {
    fn build(
        op: &str,
        param: &Parameter,
        namesake: Option<Place>,
        components: &Components,
        flags: &mut Namespace,
    ) -> Result<Self, LoadError> {
        let data = param.parameter_data_ref();
        let shape = shape_of(op, param, namesake, components, flags)?;
        if let Shape::Unreachable(why) = &shape
            && data.required
        {
            return Err(LoadError::Parameter {
                op: op.to_owned(),
                name: data.name.clone(),
                place: place(param),
                why: why.clone(),
            });
        }
        Ok(Self {
            name: data.name.clone(),
            required: data.required,
            shape,
            description: data.description.clone(),
            requires: requires_of(op, data)?,
        })
    }
}

/// The names one parameter's `x-cli-requires` lists, if it lists any.
fn requires_of(op: &str, data: &ParameterData) -> Result<Vec<String>, LoadError> {
    let reject = || LoadError::RequiresList {
        op: op.to_owned(),
        name: data.name.clone(),
    };
    match data.extensions.get(REQUIRES) {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Array(names)) => names
            .iter()
            .map(|name| name.as_str().map(str::to_owned).ok_or_else(reject))
            .collect(),
        Some(_) => Err(reject()),
    }
}

/// Every requirement one operation's parameters state names another parameter of
/// the same operation that a request can carry.
///
/// Checked once every parameter is read, because a requirement may name one the
/// document lists after it. A parameter that lists its own name asks for
/// nothing, and a debug build of clap refuses an argument that requires itself.
/// A name that is not there, or one this crate cannot send, is a requirement no
/// run could meet. Each is refused here rather than mounted: the first would
/// stop a debug build of clap, and the others would make a flag that refuses
/// everyone who gives it.
fn companions(op: &str, params: &[Param]) -> Result<(), LoadError> {
    match unmet(params) {
        None => Ok(()),
        Some(Unmet::Itself { name }) => Err(LoadError::RequiresItself {
            op: op.to_owned(),
            name: name.to_owned(),
        }),
        Some(Unmet::Unknown { name, requires }) => Err(LoadError::RequiresUnknown {
            op: op.to_owned(),
            name: name.to_owned(),
            requires: requires.to_owned(),
        }),
        Some(Unmet::Unreachable {
            name,
            requires,
            why,
        }) => Err(LoadError::RequiresUnreachable {
            op: op.to_owned(),
            name: name.to_owned(),
            requires: requires.to_owned(),
            why: why.clone(),
        }),
    }
}

/// What one parameter is worth on a command line, and the flag it claims when it
/// is worth one.
///
/// Every way of not being worth a flag lands in the same place. The blast radius
/// of a shape this CLI cannot spell is the operation that declares it, never the
/// document that holds it.
fn shape_of(
    op: &str,
    param: &Parameter,
    namesake: Option<Place>,
    components: &Components,
    flags: &mut Namespace,
) -> Result<Shape, LoadError> {
    let data = param.parameter_data_ref();
    let Some((location, style)) = sent_in(param) else {
        return Ok(Shape::Unreachable(Unsupported::Cookie));
    };
    let ParameterSchemaOrContent::Schema(schema) = &data.format else {
        return Ok(Shape::Unreachable(Unsupported::Encoded));
    };
    // One value, or a list of them: an array is asked about its items, and
    // whichever schema describes the value is the one everything about that
    // value is read off — so the rules of each value and its declared kind are
    // the items', which is where a list of days says that a day is what it
    // holds, and the rules of the list are the array's own.
    let (value, rules) = match under(items_of(schema, components), op, &data.name)? {
        Some((items, rules)) => (Cow::Owned(items), Some(rules)),
        None => (Cow::Borrowed(schema), None),
    };
    let Some(scalar) = runnable_scalar(&value, components, op, &data.name)? else {
        return Ok(Shape::Unreachable(Unsupported::Structured));
    };
    // The style is read whatever the schema is, because it is not only about
    // delimiters: `matrix` puts a `;name=` in front of one path value as surely
    // as in front of a list. A style written out as `form` instead would be a
    // request in a shape the server does not read.
    let join = match style {
        Ok(join) => join,
        Err(style) => return Ok(Shape::Unreachable(Unsupported::Style(style.to_owned()))),
    };
    // The name is asked last, so that a parameter this CLI could not have
    // supplied anyway is reported for what the document said about its value
    // rather than for how the vendor spelled it.
    let Ok(spelling) = spelled("parameter", &data.name) else {
        return Ok(Shape::Unreachable(Unsupported::Unspellable));
    };
    // After the name is spelled, and before it claims a flag: a parameter that
    // a value can never reach takes nothing from the namespace.
    if let Some(place) = namesake {
        return Ok(Shape::Unreachable(Unsupported::Shared(place)));
    }
    // A header the request would carry has a name that a header can have. A
    // parameter with no flag sends nothing, so its name is no one's business.
    if location == Location::Header && HeaderName::from_bytes(data.name.as_bytes()).is_err() {
        return Err(LoadError::HeaderName {
            op: op.to_owned(),
            name: data.name.clone(),
        });
    }
    Ok(Shape::Flag {
        flag: flags
            .claim(&spelling, "param")
            .map_err(|clash| clashed(&clash, op, "parameter", &data.name))?,
        location,
        scalar,
        list: rules.map(|items| List { join, items }),
        format: under(format_of(&value, components), op, &data.name)?,
    })
}

/// Where a parameter goes, and how a list of its values would reach it there.
///
/// One answer, because the second is read off the first: `style` means different
/// things in a query and in a path, and `in: cookie` has no answer at all. The
/// `Err` carries the document's own spelling of a style this crate does not
/// write, so that whatever refuses it can name it — `form` in a query and
/// `simple` everywhere else are the two it writes, and they are also the two
/// OpenAPI defaults, so a document that says nothing lands on them.
fn sent_in(param: &Parameter) -> Option<(Location, Result<Join, &'static str>)> {
    Some(match param {
        Parameter::Query {
            parameter_data,
            style,
            ..
        } => (Location::Query, query_join(style, parameter_data.explode)),
        Parameter::Path { style, .. } => (
            Location::Path,
            match style {
                PathStyle::Simple => Ok(Join::Commas),
                PathStyle::Matrix => Err("matrix"),
                PathStyle::Label => Err("label"),
            },
        ),
        // `simple` is the only style a header has, and a list under it is
        // comma-separated whether or not it explodes.
        Parameter::Header { .. } => (Location::Header, Ok(Join::Commas)),
        Parameter::Cookie { .. } => return None,
    })
}

/// `form` is what a query parameter defaults to and `explode: true` is what
/// `form` defaults to, which is one field per value. The three other styles name
/// themselves rather than being written as `form`.
fn query_join(style: &QueryStyle, explode: Option<bool>) -> Result<Join, &'static str> {
    match style {
        QueryStyle::Form => Ok(match explode {
            Some(false) => Join::Commas,
            None | Some(true) => Join::Pairs,
        }),
        QueryStyle::SpaceDelimited => Err("spaceDelimited"),
        QueryStyle::PipeDelimited => Err("pipeDelimited"),
        QueryStyle::DeepObject => Err("deepObject"),
    }
}

/// The schema a `type: array` parameter's values are described by, when the
/// document says what its items are, and what the array states about the values
/// taken together.
///
/// Everything a list parameter's values are — the rules they are held to and
/// the kind the document calls them — is stated by the items rather than on the
/// array, which only says how many of them there are and whether they differ.
fn items_of(
    schema: &ReferenceOr<Schema>,
    components: &Components,
) -> Result<Option<(ReferenceOr<Schema>, Items)>, RefError> {
    let schema = resolve_schema(schema, components)?;
    let SchemaKind::Type(openapiv3::Type::Array(array)) = &schema.schema_kind else {
        return Ok(None);
    };
    let rules = Items {
        min: array.min_items,
        max: array.max_items,
        unique: array.unique_items,
    };
    Ok(array
        .items
        .clone()
        .map(|items| (ReferenceOr::unbox(items), rules)))
}

impl Body {
    fn build(
        id: &str,
        op: &openapiv3::Operation,
        components: &Components,
        flags: &mut Namespace,
    ) -> Result<Self, LoadError> {
        let Some(body) = &op.request_body else {
            return Ok(Self::None);
        };
        let body = under(
            resolve(
                body,
                |key| components.request_bodies.get(key),
                "requestBodies",
            ),
            id,
            BODY,
        )?;
        let required = body.required;
        // The first key that names JSON, wherever it stands, and else the first
        // key of all, whatever it names. `docs/validation.md` states the rule.
        let entry = body
            .content
            .iter()
            .find(|(name, _)| is_json(name))
            .or_else(|| body.content.iter().next());
        let Some((media_type, media)) = entry else {
            return Ok(Self::None);
        };
        // The key this operation would be sent under, and only that one: a key
        // beside it that nothing here ever reads is the vendor's business, and
        // refusing over it would refuse documents this crate serves correctly.
        if !is_media_type(media_type) {
            return Err(LoadError::MediaType {
                op: id.to_owned(),
                media_type: media_type.clone(),
            });
        }
        if is_media_range(media_type) {
            return Err(LoadError::MediaRange {
                op: id.to_owned(),
                media_type: media_type.clone(),
            });
        }
        if is_multipart(media_type) {
            return Ok(Self::Multipart {
                names: under(part_names(media, components), id, BODY)?,
                required,
            });
        }
        if !is_json(media_type) {
            return Ok(Self::Opaque {
                media_type: media_type.clone(),
                required,
            });
        }
        let json = JsonEntry {
            media_type,
            schema: media.schema.as_ref(),
            required,
        };
        json_body(id, json, components, flags)
    }
}

/// The `content` entry a JSON body is read from: the key it stands under, as the
/// document spells it, which is the `Content-Type` the body goes out under, the
/// schema beside the key, and whether the body is required.
#[derive(Clone, Copy)]
struct JsonEntry<'a> {
    media_type: &'a str,
    schema: Option<&'a ReferenceOr<Schema>>,
    required: bool,
}

/// What a JSON body is worth on a command line: one flag per property, or one
/// flag for the whole thing.
///
/// The same question [`shape_of`] asks of a parameter, and answered the same
/// way — everything a flag set needs lives on the variant that has it, so a
/// body cannot leave behind a flag the request builder would ignore.
///
/// A body that goes whole carries its skeleton, because going whole is exactly
/// what leaves the command line with nothing saying what the body wants. The
/// walk that decided it has already seen the shape.
fn json_body(
    id: &str,
    entry: JsonEntry<'_>,
    components: &Components,
    flags: &mut Namespace,
) -> Result<Body, LoadError> {
    let JsonEntry {
        media_type,
        schema,
        required,
    } = entry;
    let media_type = media_type.to_owned();
    let Some(schema) = schema else {
        return Ok(Body::JsonWhole {
            media_type,
            required,
            template: None,
        });
    };
    let whole = || {
        Ok(Body::JsonWhole {
            media_type: media_type.clone(),
            required,
            template: under(template(schema, components), id, BODY)?,
        })
    };
    let SchemaKind::Type(openapiv3::Type::Object(object)) =
        &under(resolve_schema(schema, components), id, BODY)?.schema_kind
    else {
        return whole();
    };

    let properties = properties_of(object, components, id)?;
    let mut fields = Vec::with_capacity(properties.len());
    for Property {
        name,
        schema: property,
        scalar,
    } in properties
    {
        let Some(scalar) = scalar else {
            // One nested property is enough: the whole body goes through
            // `--json-body`, and no sibling gets a flag the request builder
            // would then throw away.
            return whole();
        };
        // A property whose name has no kebab-case spelling has no flag either,
        // and the body's rule is about whether a property has one rather than
        // about why it has none — so it goes the way a nested property goes.
        // What that buys is the disclosure: the template `--json-body-template`
        // prints carries the key, where a flag called `""` names it nowhere.
        let Ok(spelling) = spelled("property", name) else {
            return whole();
        };
        fields.push(Field {
            flag: flags
                .claim(&spelling, "body")
                .map_err(|clash| clashed(&clash, id, "property", name))?,
            name: name.clone(),
            required: required && object.required.iter().any(|r| r == name),
            scalar,
            format: under(format_of(&property, components), id, name)?,
            description: under(description_of(&property, components), id, name)?,
        });
    }
    Ok(Body::JsonFields { media_type, fields })
}

/// A property of a body and the scalar its schema states, once every rule it
/// carries is known to run. `None` is a property that does not fit on a flag.
struct Property<'a> {
    name: &'a String,
    schema: ReferenceOr<Schema>,
    scalar: Option<Scalar>,
}

/// Every property of the object, read for the rules it states.
///
/// All of them are asked before [`json_body`] decides whether the body is flat,
/// because a rule that cannot run is a fault of the document and not of the
/// flag it would have been. A nested property sends the body whole, and a
/// property listed after it states the same rule all the same, so the verdict
/// cannot depend on which one comes first.
fn properties_of<'a>(
    object: &'a ObjectType,
    components: &Components,
    id: &str,
) -> Result<Vec<Property<'a>>, LoadError> {
    object
        .properties
        .iter()
        .map(|(name, property)| {
            let schema = property.clone().unbox();
            let scalar = runnable_scalar(&schema, components, id, name)?;
            Ok(Property {
                name,
                schema,
                scalar,
            })
        })
        .collect()
}

/// A clash, said in the vocabulary of whoever can resolve it.
///
/// The two words are guarded for one reason and moved for two different ones,
/// so the refusal is two sentences rather than one with a branch in it: the
/// gate's spelling is in the document's correction layer, and the
/// confirmation's is in the call that loads or generates from it.
fn clashed(clash: &Clash, op: &str, what: &'static str, name: &str) -> LoadError {
    match clash.kind {
        Protected::Gate => LoadError::GateTakenByName {
            op: op.to_owned(),
            what,
            name: name.to_owned(),
            gate: clash.word.clone(),
        },
        Protected::Commit => LoadError::CommitTakenByName {
            op: op.to_owned(),
            what,
            name: name.to_owned(),
            commit: clash.word.clone(),
        },
    }
}

/// Read a schema on one value's behalf, so that a `$ref` under it is refused by
/// name.
///
/// Every `$ref` this reduction follows is reached through a parameter or a body
/// property, and the reference on its own is not something an adopter can find:
/// one `#/components/schemas/…` stands under a dozen operations, and only one of
/// them is the one to correct. So the reads under a value go through
/// here, which is why [`runnable`] stands beside it — both turn a failure about
/// a schema into a refusal saying which value stated it.
fn under<T>(read: Result<T, RefError>, op: &str, name: &str) -> Result<T, LoadError> {
    read.map_err(|source| LoadError::Unresolvable {
        op: op.to_owned(),
        name: name.to_owned(),
        source,
    })
}

/// The scalar a value's schema states, once every rule it carries is known to
/// run, and `None` where the schema does not fit on a flag.
///
/// The one place a parameter and a body property are read for their rules, so
/// that the two cannot refuse different things.
fn runnable_scalar(
    schema: &ReferenceOr<Schema>,
    components: &Components,
    op: &str,
    name: &str,
) -> Result<Option<Scalar>, LoadError> {
    let Some(scalar) = under(scalar_of(schema, components), op, name)? else {
        return Ok(None);
    };
    if under(lists_only_null(schema, components), op, name)? {
        return Err(LoadError::Unrunnable {
            op: op.to_owned(),
            name: name.to_owned(),
            source: ScalarError::Rule {
                raw: "null".to_owned(),
                rule: "is all that the `enum` lists, and a flag cannot send it".to_owned(),
            },
        });
    }
    runnable(&scalar, op, name)?;
    Ok(Some(scalar))
}

/// Refuse a rule that cannot be run as the document means it, naming the
/// operation and the value it was stated about.
///
/// A `pattern` the engine cannot read refuses every value, so a document that
/// states one describes a flag nothing can satisfy. A `pattern` with `\p{`, `\P{`
/// or `\u{` is read another way by the flag and by a server that runs it with the
/// Unicode flag, and a `multipleOf` that is not more than 0 has no multiples to
/// admit. Saying so while the document is reduced is what keeps each a bless-time
/// failure rather than a user's.
fn runnable(scalar: &Scalar, op: &str, name: &str) -> Result<(), LoadError> {
    scalar.runnable().map_err(|source| LoadError::Unrunnable {
        op: op.to_owned(),
        name: name.to_owned(),
        source,
    })
}

/// The part names a multipart schema declares, in document order.
///
/// A schema that cannot be followed is the refusal any other body schema gets.
/// Reading it as a body with no parts would load a document whose upload names
/// nothing, and the loss would show only when a caller sent a file.
fn part_names(
    media: &openapiv3::MediaType,
    components: &Components,
) -> Result<Vec<String>, RefError> {
    let Some(schema) = &media.schema else {
        return Ok(Vec::new());
    };
    let schema = resolve_schema(schema, components)?;
    let SchemaKind::Type(openapiv3::Type::Object(object)) = &schema.schema_kind else {
        return Ok(Vec::new());
    };
    Ok(object.properties.keys().cloned().collect())
}
