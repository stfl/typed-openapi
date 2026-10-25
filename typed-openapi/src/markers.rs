//! The `x-cli-` markers: the five keys this crate reads, where each of them is
//! read, and the refusal of every other `x-cli-` key and of every one that
//! stands where nothing reads it.
//!
//! A marker nobody reads is a statement the document makes and the CLI does not
//! keep. An `x-cli-writes` on a path item leaves the `GET` below it a read, sent
//! on sight with no error, and an `x-cli-requires` written beside a `$ref` is
//! dropped by the object model. The adopter wrote the gate and believes in it.
//! This crate owns the `x-cli-` prefix, so a key under it that nothing reads is
//! the adopter's mistake, and it is refused by name and place while the document
//! is loaded.
//!
//! # Where a key is read
//!
//! | key | read on |
//! |---|---|
//! | `x-cli-command`, `x-cli-gates`, `x-cli-group`, `x-cli-writes` | an operation: a method of a path item under `paths` |
//! | `x-cli-requires` | a parameter: of a path item, of an operation, or under `components.parameters`, and not beside a `$ref` |
//!
//! Those are the places the reduction reads, so a key on any other object the
//! walk enters is read by nothing. An operation under `callbacks` is never
//! reduced, and the marker on it is refused with the others. A parameter under
//! `components.parameters` is a place the key is read whether or not the
//! `$ref` of an operation names it, because a component is where the adopter
//! writes it once.
//!
//! # What the walk reads
//!
//! The walk is over the *overlaid* document before the object model reads it,
//! because the model drops a key beside a `$ref` without a word. It enters a
//! value only where [`shape`](crate::shape) says that OpenAPI gives the field
//! the type of one of its objects, or of a list or a map of them, for the kind
//! of object that holds the field. An `x-…` key of an object it enters is an
//! extension, and the walk asks about it by its name.
//!
//! It reads nothing under any other key. That is a field of type Any (an
//! `example`, a `default`, an `enum`, the `value` of an Example Object, and the
//! `parameters` values and the `requestBody` of a Link Object), the value of an
//! extension, and a key that OpenAPI 3.0.3 does not define. The last is one
//! class whoever defines the key: a field of a later version (`webhooks`), a
//! keyword of a later draft (`$defs`, or `dependencies` on a schema), or a key
//! that no version defines (`vouchers` in `paths`, which is no path, and `foo`
//! among the responses, which is no status code). The object model drops each
//! of them and nothing builds a request or a flag from them. What stands there
//! is the vendor's own JSON, and a key in it that is spelled like a marker is
//! not one.
//!
//! Position is what tells an extension from a name. A map keyed by names
//! somebody chose holds a property, a component, a media type or a response
//! header under any spelling, `x-cli-writes` among them, and the same spelling
//! is an extension on an Operation Object. The walk reads no key of such a map
//! itself. The Paths Object and the Responses Object are the two maps that the
//! specification also lets a vendor extend, so an `x-…` key stands for an
//! extension there and for a name nowhere else.
//!
//! What the rule costs is an object that the table leaves out and that the
//! specification gives fixed fields: a marker standing there is a marker
//! missed, never a document wrongly refused.

use serde_json::{Map, Value};

use crate::model::LoadError;
use crate::required::{Step, spelled};
use crate::shape::{Entry, Keys, Kind, Reach, Route, is_extension};

/// The walk judges a key by where it stands, so it enters what OpenAPI 3.0.3
/// defines and nothing beside it.
const REACH: Reach = Reach::Defined;

/// The prefix this crate owns.
const PREFIX: &str = "x-cli-";

/// The four extensions this crate reads on an operation, all of them an
/// adopter's say over something the document alone cannot settle. An Overlay is
/// where they are written.
///
/// HTTP cannot say "this GET writes", so the document has to.
pub(crate) const WRITES: &str = "x-cli-writes";
/// The command name to mount an operation under, where the path spells one
/// badly — or where two operations reduce to the same name.
pub(crate) const COMMAND: &str = "x-cli-command";
/// The group to mount an operation under, where the path's own segment is not
/// the resource the operation belongs to.
pub(crate) const GROUP: &str = "x-cli-group";
/// The hazards an operation stands behind by name, beside the write gate
/// itself. One list rather than one marker per name, so a gate an adoption
/// invents costs an Overlay line and not a release of this crate.
pub(crate) const GATES: &str = "x-cli-gates";
/// The other parameters a parameter needs beside it, by wire name. OpenAPI
/// cannot say that two parameters only mean something together, so the marker
/// sits on the parameter that needs the other.
pub(crate) const REQUIRES: &str = "x-cli-requires";

/// The thing a request is built from that a marker is read off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subject {
    /// An operation of a path item under `paths`.
    Operation,
    /// A parameter that is not a `$ref`: a path item's, an operation's, or one
    /// under `components.parameters`.
    Parameter,
}

/// Every key this crate reads, and what it reads the key off. A key under the
/// prefix that is not one of these is read by nothing, and one of these
/// standing on anything but its subject is read by nothing either.
///
/// A new key joins this table, and the wording of a refusal follows from it.
const READ: [(&str, Subject); 5] = [
    (COMMAND, Subject::Operation),
    (GATES, Subject::Operation),
    (GROUP, Subject::Operation),
    (WRITES, Subject::Operation),
    (REQUIRES, Subject::Parameter),
];

impl Subject {
    /// What the object `holder` is as far as the reduction goes, read off its
    /// kind and the way down to it: the subject of a marker, or `None` for an
    /// object that nothing is built from.
    ///
    /// The position tells a path item that the reduction reads from one that it
    /// never does: an operation under `callbacks` is of the kind of an
    /// operation and is not read.
    fn of(kind: Kind, at: &[Step<'_>], holder: &Map<String, Value>) -> Option<Self> {
        use Step::{Index, Key};
        match (kind, at) {
            (Kind::Operation, [Key("paths"), Key(_), Key(_)]) => Some(Self::Operation),
            // A `$ref` stands for the parameter it names, and the model keeps
            // nothing beside it.
            (Kind::Parameter, _) if holder.contains_key("$ref") => None,
            (
                Kind::Parameter,
                [Key("paths"), Key(_), Key("parameters"), Index(_)]
                | [Key("paths"), Key(_), Key(_), Key("parameters"), Index(_)],
            ) => Some(Self::Parameter),
            (Kind::Parameter, [Key("components"), Key("parameters"), Key(_)]) => {
                Some(Self::Parameter)
            }
            _ => None,
        }
    }
}

/// What the reasons of a refusal say about where `key` is read, as the table of
/// the module doc does.
///
/// The message of `LoadError::MisplacedMarker` ends with it. The walk refuses a
/// key that nothing reads as an `UnknownMarker`, so the last arm is for a
/// `MisplacedMarker` built outside it.
pub(crate) fn reading(key: &str) -> String {
    match READ.iter().find(|(read, _)| *read == key) {
        Some((_, Subject::Operation)) => {
            "it reads that key on a method of a path item under `paths`".to_owned()
        }
        Some((read, Subject::Parameter)) => format!(
            "it reads `{read}` on a parameter under `paths` or `components.parameters`, \
             and not beside a `$ref`"
        ),
        None => "it reads no such key".to_owned(),
    }
}

/// The keys this crate reads, spelled for a message.
pub(crate) fn vocabulary() -> String {
    let spelled = READ.map(|(key, _)| format!("`{key}`"));
    let [first @ .., last] = &spelled;
    format!("{} and {last}", first.join(", "))
}

/// Refuse the first `x-cli-` key of the document that this crate reads nowhere
/// or does not read where it stands, in the order the document lists them.
pub(crate) fn check(document: &Value) -> Result<(), LoadError> {
    Walk { at: Vec::new() }.read(Kind::Root, document)
}

/// The descent, with the way down to the object being read.
struct Walk<'d> {
    at: Vec<Step<'d>>,
}

impl<'d> Walk<'d> {
    /// This node, then everything under it that the table leads to. A list
    /// stands for each of its members.
    fn read(&mut self, kind: Kind, node: &'d Value) -> Result<(), LoadError> {
        match node {
            Value::Object(fields) => fields
                .iter()
                .try_for_each(|(key, value)| self.field(kind, fields, key, value)),
            Value::Array(members) => members.iter().enumerate().try_for_each(|(index, member)| {
                self.at.push(Step::Index(index));
                let read = self.read(kind, member);
                self.at.pop();
                read
            }),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        }
    }

    /// One field of `holder`, an object of this kind: an extension, which is
    /// asked whether this crate reads it, or a key whose value is entered where
    /// the table leads there and nowhere else.
    fn field(
        &mut self,
        kind: Kind,
        holder: &'d Map<String, Value>,
        key: &'d str,
        value: &'d Value,
    ) -> Result<(), LoadError> {
        if is_extension(key) {
            return self.extension(Subject::of(kind, &self.at, holder), key);
        }
        let Some(route) = kind.route(key, REACH) else {
            return Ok(());
        };
        self.at.push(Step::Key(key));
        let read = self.follow(route, value);
        self.at.pop();
        read
    }

    /// What the route says stands under the key.
    fn follow(&mut self, route: Route, value: &'d Value) -> Result<(), LoadError> {
        match route {
            Route::Node(kind) => self.read(kind, value),
            Route::Map(kind, keys) => self.entries(kind, keys, value),
        }
    }

    /// Every entry of a map of nodes of one kind. The keys of the map are
    /// names, and are not read, unless the route says the specification lets a
    /// vendor extend the map: the Paths Object and the Responses Object, whose
    /// `x-…` keys are asked and stand on no subject.
    fn entries(&mut self, kind: Kind, keys: Keys, map: &'d Value) -> Result<(), LoadError> {
        let Some(entries) = map.as_object() else {
            return Ok(());
        };
        entries
            .iter()
            .try_for_each(|(key, entry)| match keys.entry(key, REACH) {
                Some(Entry::Member) => {
                    self.at.push(Step::Key(key));
                    let read = self.read(kind, entry);
                    self.at.pop();
                    read
                }
                Some(Entry::Extension) => self.extension(None, key),
                None => Ok(()),
            })
    }

    /// An extension standing on `subject`. The value of one is the vendor's own
    /// JSON and is not read, and a key under this crate's prefix is one this
    /// crate reads off that subject, or a refusal that names the key and the
    /// object it stands on.
    fn extension(&self, subject: Option<Subject>, key: &str) -> Result<(), LoadError> {
        if !key.starts_with(PREFIX) {
            return Ok(());
        }
        let at = || spelled(&self.at);
        let Some((_, read)) = READ.iter().find(|(known, _)| *known == key) else {
            return Err(LoadError::UnknownMarker {
                key: key.to_owned(),
                at: at(),
            });
        };
        if subject == Some(*read) {
            return Ok(());
        }
        Err(LoadError::MisplacedMarker {
            key: key.to_owned(),
            at: at(),
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    /// What the scan makes of a document, in words: `read`, or the refusal as
    /// `misplaced` or `unknown`, its key and the node that holds the key.
    fn verdict(document: &str) -> String {
        let value = crate::overlay::parse(document).expect("the fixture parses");
        match check(&value) {
            Ok(()) => "read".to_owned(),
            Err(LoadError::MisplacedMarker { key, at }) => misplaced(&key, &at),
            Err(LoadError::UnknownMarker { key, at }) => format!("unknown {key} on {at}"),
            Err(other) => panic!("only a marker is refused here: {other:?}"),
        }
    }

    fn misplaced(key: &str, at: &str) -> String {
        format!("misplaced {key} on {at}")
    }

    #[test]
    fn each_key_is_read_where_a_request_takes_it_from() {
        for marker in [
            "x-cli-command: c",
            "x-cli-gates: [g]",
            "x-cli-group: g",
            "x-cli-writes: true",
        ] {
            let document = format!("paths:\n  /a:\n    get:\n      {marker}\n");
            assert_eq!(verdict(&document), "read", "{marker}");
        }
        for document in [
            "paths:\n  /a:\n    parameters: [{ name: n, x-cli-requires: [m] }]\n",
            "paths:\n  /a:\n    get:\n      parameters: [{ name: n, x-cli-requires: [m] }]\n",
            "components:\n  parameters:\n    N: { name: n, x-cli-requires: [m] }\n",
        ] {
            assert_eq!(verdict(document), "read", "{document}");
        }
    }

    #[test]
    fn a_key_is_refused_at_every_place_it_is_not_read() {
        let get = "paths:\n  /a:\n    get:\n";
        for (document, key, at) in [
            (
                "paths:\n  /a:\n    x-cli-writes: true\n",
                "x-cli-writes",
                "$.paths['/a']",
            ),
            (
                &format!("{get}      x-cli-requires: [m]\n"),
                "x-cli-requires",
                "$.paths['/a'].get",
            ),
            (
                &format!("{get}      parameters: [{{ name: n, x-cli-writes: true }}]\n"),
                "x-cli-writes",
                "$.paths['/a'].get.parameters[0]",
            ),
            (
                &format!("{get}      responses:\n        '200': {{ x-cli-writes: true }}\n"),
                "x-cli-writes",
                "$.paths['/a'].get.responses['200']",
            ),
            (
                // An operation under a callback is never reduced.
                &format!(
                    "{get}      callbacks:\n        done:\n          '{{$request.body#/url}}':\n            post: {{ x-cli-gates: [g] }}\n"
                ),
                "x-cli-gates",
                "$.paths['/a'].get.callbacks.done['{$request.body#/url}'].post",
            ),
            ("x-cli-writes: true\n", "x-cli-writes", "$"),
            ("info: { x-cli-writes: true }\n", "x-cli-writes", "$.info"),
            (
                "components:\n  schemas:\n    A: { x-cli-group: g }\n",
                "x-cli-group",
                "$.components.schemas.A",
            ),
            ("paths:\n  x-cli-writes: true\n", "x-cli-writes", "$.paths"),
            // A Link Object and an Example Object are objects the walk enters:
            // their fields are fixed, and what they hold of the vendor's own
            // is the value of a field that is of type Any.
            (
                &format!(
                    "{get}      responses:\n        '200': {{ description: d, links: {{ next: {{ x-cli-writes: true }} }} }}\n"
                ),
                "x-cli-writes",
                "$.paths['/a'].get.responses['200'].links.next",
            ),
            (
                "components:\n  examples:\n    E: { x-cli-writes: true }\n",
                "x-cli-writes",
                "$.components.examples.E",
            ),
        ] {
            assert_eq!(verdict(document), misplaced(key, at), "{document}");
        }
    }

    /// Each row is a document with an `x-cli-writes` on one object, and the
    /// node that holds the key. Every one is refused as misplaced.
    fn refused_on(rows: &[(&str, &str)]) {
        for (document, at) in rows {
            assert_eq!(
                verdict(document),
                misplaced("x-cli-writes", at),
                "{document}"
            );
        }
    }

    /// Every object the table leads to is entered, and a marker on it is
    /// refused. A route that the table loses takes its row with it, and the
    /// walk then reads nothing there.
    #[test]
    fn a_key_is_refused_on_the_objects_that_describe_the_api() {
        let item = "paths:\n  /a:\n";
        let get = "paths:\n  /a:\n    get:\n";
        refused_on(&[
            ("externalDocs: { x-cli-writes: true }\n", "$.externalDocs"),
            (
                "info: { contact: { x-cli-writes: true } }\n",
                "$.info.contact",
            ),
            (
                "info: { license: { x-cli-writes: true } }\n",
                "$.info.license",
            ),
            (
                "servers: [{ url: u, x-cli-writes: true }]\n",
                "$.servers[0]",
            ),
            (
                "servers: [{ url: u, variables: { v: { x-cli-writes: true } } }]\n",
                "$.servers[0].variables.v",
            ),
            ("tags: [{ name: t, x-cli-writes: true }]\n", "$.tags[0]"),
            (
                "tags: [{ name: t, externalDocs: { x-cli-writes: true } }]\n",
                "$.tags[0].externalDocs",
            ),
            (
                &format!("{item}    servers: [{{ x-cli-writes: true }}]\n"),
                "$.paths['/a'].servers[0]",
            ),
            (
                &format!("{get}      servers: [{{ x-cli-writes: true }}]\n"),
                "$.paths['/a'].get.servers[0]",
            ),
            (
                &format!("{get}      externalDocs: {{ x-cli-writes: true }}\n"),
                "$.paths['/a'].get.externalDocs",
            ),
        ]);
    }

    #[test]
    fn a_key_is_refused_on_a_link_and_on_an_example_wherever_one_stands() {
        let get = "paths:\n  /a:\n    get:\n";
        let ok = "      responses:\n        '200': { description: d, ";
        refused_on(&[
            (
                &format!("{get}{ok}links: {{ next: {{ server: {{ x-cli-writes: true }} }} }} }}\n"),
                "$.paths['/a'].get.responses['200'].links.next.server",
            ),
            (
                "components:\n  links:\n    L: { x-cli-writes: true }\n",
                "$.components.links.L",
            ),
            (
                &format!(
                    "{get}      parameters: [{{ name: n, examples: {{ E: {{ x-cli-writes: true }} }} }}]\n"
                ),
                "$.paths['/a'].get.parameters[0].examples.E",
            ),
            (
                &format!(
                    "{get}{ok}content: {{ application/json: {{ examples: {{ E: {{ x-cli-writes: true }} }} }} }} }}\n"
                ),
                "$.paths['/a'].get.responses['200'].content['application/json'].examples.E",
            ),
        ]);
    }

    #[test]
    fn a_key_is_refused_on_a_security_scheme_and_on_the_small_objects_of_a_schema() {
        let secured = "components:\n  securitySchemes:\n    S: ";
        let shaped = "components:\n  schemas:\n    A: ";
        refused_on(&[
            (
                &format!("{secured}{{ x-cli-writes: true }}\n"),
                "$.components.securitySchemes.S",
            ),
            (
                &format!("{secured}{{ flows: {{ x-cli-writes: true }} }}\n"),
                "$.components.securitySchemes.S.flows",
            ),
            (
                &format!("{secured}{{ flows: {{ implicit: {{ x-cli-writes: true }} }} }}\n"),
                "$.components.securitySchemes.S.flows.implicit",
            ),
            (
                &format!("{shaped}{{ discriminator: {{ x-cli-writes: true }} }}\n"),
                "$.components.schemas.A.discriminator",
            ),
            (
                &format!("{shaped}{{ xml: {{ x-cli-writes: true }} }}\n"),
                "$.components.schemas.A.xml",
            ),
            (
                &format!("{shaped}{{ externalDocs: {{ x-cli-writes: true }} }}\n"),
                "$.components.schemas.A.externalDocs",
            ),
        ]);
    }

    #[test]
    fn the_reason_of_a_refusal_says_where_the_key_is_read() {
        assert_eq!(
            reading("x-cli-gates"),
            "it reads that key on a method of a path item under `paths`"
        );
        assert_eq!(
            reading("x-cli-requires"),
            "it reads `x-cli-requires` on a parameter under `paths` or \
             `components.parameters`, and not beside a `$ref`"
        );
        assert_eq!(reading("x-cli-mode"), "it reads no such key");
    }

    #[test]
    fn of_two_keys_nothing_reads_the_one_the_document_lists_first_is_named() {
        let document = "paths:\n  /a:\n    x-cli-gates: [g]\n    x-cli-writes: true\n";
        assert_eq!(verdict(document), misplaced("x-cli-gates", "$.paths['/a']"));
    }

    #[test]
    fn a_key_beside_a_ref_is_not_read_but_the_same_key_on_the_component_is() {
        let beside = "paths:\n  /a:\n    get:\n      parameters:\n        - $ref: '#/components/parameters/N'\n          x-cli-requires: [m]\n";
        assert_eq!(
            verdict(beside),
            misplaced("x-cli-requires", "$.paths['/a'].get.parameters[0]")
        );
        let on = "components:\n  parameters:\n    N: { name: n, x-cli-requires: [m] }\n";
        assert_eq!(verdict(on), "read");
    }

    #[test]
    fn a_key_no_one_reads_is_unknown_wherever_it_stands() {
        for (document, key, at) in [
            (
                "paths:\n  /a:\n    get:\n      x-cli-write: true\n",
                "x-cli-write",
                "$.paths['/a'].get",
            ),
            ("x-cli-anything: 1\n", "x-cli-anything", "$"),
            (
                "paths:\n  /a:\n    get:\n      x-cli-: 1\n",
                "x-cli-",
                "$.paths['/a'].get",
            ),
        ] {
            assert_eq!(
                verdict(document),
                format!("unknown {key} on {at}"),
                "{document}"
            );
        }
    }

    #[test]
    fn a_name_somebody_chose_is_no_extension_and_the_vendors_own_json_is_not_read() {
        for document in [
            // A property, a component, a media type, a response header, a
            // mapping entry and a scope, each spelled like a marker.
            "components:\n  schemas:\n    x-cli-writes:\n      properties:\n        x-cli-writes: { type: string }\n        properties: { properties: { x-cli-gates: { type: string } } }\n      discriminator: { mapping: { x-cli-writes: '#/a' } }\n",
            "components:\n  responses:\n    x-cli-writes: { description: d, headers: { x-cli-group: {} }, content: { x-cli-command: {} } }\n",
            "components:\n  securitySchemes:\n    s: { flows: { implicit: { scopes: { x-cli-writes: d } } } }\n",
            "security:\n  - x-cli-writes: []\n",
            // Data and the vendor's own JSON.
            "components:\n  schemas:\n    A: { example: { x-cli-writes: yes }, default: { x-cli-gates: [] }, enum: [{ x-cli-group: g }], const: { x-cli-command: c } }\n",
            "components:\n  examples:\n    E: { value: { x-cli-writes: yes } }\n",
            "x-vendor: { x-cli-writes: yes }\npaths:\n  /a:\n    x-total: { x-cli-gates: [g] }\n",
            "paths:\n  /a:\n    get:\n      responses:\n        default: { description: d }\n",
        ] {
            assert_eq!(verdict(document), "read", "{document}");
        }
    }

    /// A value that OpenAPI 3.0.3 gives the type Any, and a key that an object
    /// does not define, are the vendor's data and no part of the document. A
    /// walk that entered them would refuse a document with no marker in it.
    /// The keys that 3.0.3 does not define anywhere are in the test below.
    #[test]
    fn a_field_of_type_any_and_a_key_the_object_does_not_define_are_not_entered() {
        let response = "paths:\n  /a:\n    get:\n      responses:\n        '200': ";
        for document in [
            // The `parameters` values and the `requestBody` of a Link Object.
            &format!(
                "{response}{{ description: d, links: {{ next: {{ operationId: o, parameters: {{ filter: {{ x-cli-writes: true }} }} }} }} }}\n"
            ),
            // An argument that a link passes on may be named like a marker.
            &format!(
                "{response}{{ description: d, links: {{ next: {{ operationId: o, parameters: {{ x-cli-writes: '$response.body#/id' }} }} }} }}\n"
            ),
            &format!(
                "{response}{{ description: d, links: {{ next: {{ operationId: o, requestBody: {{ x-cli-gates: [a] }} }} }} }}\n"
            ),
            // Keywords that a 3.0 schema does not have.
            &format!(
                "{response}{{ description: d, content: {{ application/json: {{ schema: {{ examples: [{{ x-cli-mode: fast }}] }} }} }} }}\n"
            ),
            &format!(
                "{response}{{ description: d, content: {{ application/json: {{ schema: {{ dependencies: {{ x-cli-mode: [b] }} }} }} }} }}\n"
            ),
            // The `value` of an Example Object, under each place one stands.
            &format!(
                "{response}{{ description: d, content: {{ application/json: {{ examples: {{ E: {{ value: {{ x-cli-writes: true }} }} }} }} }} }}\n"
            ),
            // The `default` and the `enum` of a Server Variable.
            "servers: [{ url: u, variables: { v: { default: d, enum: [{ x-cli-writes: true }] } } }]\n",
            // A field that is not a field of the object holding it.
            "info: { title: t, version: v, contact: { name: n }, license: { name: l }, summary: { x-cli-writes: true } }\n",
        ] {
            assert_eq!(verdict(document), "read", "{document}");
        }
    }

    /// A key that OpenAPI 3.0.3 does not define is no part of a 3.0 document,
    /// whoever defines it: a field of a later version (`webhooks`), a keyword
    /// of a later draft (`$defs`), or a key that no version defines
    /// (`paths.vouchers`). The object model drops each of them, and nothing
    /// else reads them, so a marker under one is read by nothing and is no
    /// mistake of the adopter's.
    #[test]
    fn a_key_that_openapi_3_0_3_does_not_define_is_not_entered() {
        // One marker that nothing reads and one that is read elsewhere, so a
        // document that loads has not escaped one refusal only.
        let markers = ["x-cli-writes: true", "x-cli-mode: fast"];
        let mut templates: Vec<String> = [
            // A field of a later version.
            "webhooks:\n  w:\n    post: { MARKER }\n",
            "components:\n  pathItems:\n    P:\n      get: { MARKER }\n",
            // A key that no version defines: no path, and no response.
            "paths:\n  vouchers:\n    get: { MARKER }\n",
            "paths:\n  /a:\n    get:\n      responses:\n        foo: { MARKER }\n",
        ]
        .map(str::to_owned)
        .into();
        // A keyword of a later draft: each one the schema walk of `required`
        // follows that a 3.0 schema does not have.
        for keyword in [
            "$defs",
            "definitions",
            "patternProperties",
            "dependentSchemas",
        ] {
            templates.push(format!(
                "components:\n  schemas:\n    A:\n      {keyword}: {{ B: {{ MARKER }} }}\n"
            ));
        }
        for keyword in [
            "unevaluatedProperties",
            "unevaluatedItems",
            "propertyNames",
            "contains",
            "contentSchema",
            "if",
            "then",
            "else",
        ] {
            templates.push(format!(
                "components:\n  schemas:\n    A:\n      {keyword}: {{ MARKER }}\n"
            ));
        }
        templates
            .push("components:\n  schemas:\n    A:\n      prefixItems: [{ MARKER }]\n".to_owned());
        for template in &templates {
            for marker in markers {
                let document = template.replace("MARKER", marker);
                assert_eq!(verdict(&document), "read", "{document}");
            }
        }
    }

    /// The keys of the Responses Object that the model reads beside a code are
    /// entered: `default`, and a range in either case, because
    /// `openapiv3::StatusCode` decides.
    #[test]
    fn a_response_under_a_key_the_model_reads_is_entered() {
        let responses = "paths:\n  /a:\n    get:\n      responses:\n        ";
        for (key, at) in [
            ("default", "$.paths['/a'].get.responses.default"),
            ("2XX", "$.paths['/a'].get.responses['2XX']"),
            ("2xx", "$.paths['/a'].get.responses['2xx']"),
        ] {
            let document = format!("{responses}{key}: {{ x-cli-writes: true }}\n");
            assert_eq!(
                verdict(&document),
                misplaced("x-cli-writes", at),
                "{document}"
            );
        }
    }

    /// The Responses Object and the Paths Object are the two maps that a vendor
    /// also extends, and `components.responses` is not one of them.
    #[test]
    fn a_key_among_the_status_codes_is_an_extension_and_a_component_called_that_is_not() {
        assert_eq!(
            verdict(
                "paths:\n  /a:\n    get:\n      responses:\n        '200': { description: d }\n        x-cli-writes: true\n"
            ),
            misplaced("x-cli-writes", "$.paths['/a'].get.responses")
        );
        assert_eq!(
            verdict("components:\n  responses:\n    x-cli-writes: { description: d }\n"),
            "read"
        );
    }
}
