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
//! Those are the places the reduction reads, so a key anywhere else is read by
//! nothing. An operation under `callbacks` or `webhooks` is never reduced, and
//! the marker on it is refused with the others.
//!
//! # What the walk reads
//!
//! The walk is over the *overlaid* document before the object model reads it,
//! because the model drops a key beside a `$ref` without a word. It enters every
//! object the specification gives fixed fields, and it reads the keys of such an
//! object as the specification's own: an `x-…` key is an extension there.
//!
//! Position is what tells an extension from a name. A map keyed by names
//! somebody chose holds a property, a component, a media type or a response
//! header under any spelling, `x-cli-writes` among them, and the same spelling
//! is an extension on an Operation Object. The walk therefore knows which keys
//! lead to such a map, and it reads no key of the map itself. The Paths Object
//! and the Responses Object are the two maps that the specification also lets a
//! vendor extend, so an `x-…` key stands for an extension there and for a name
//! nowhere else. The value of an extension is the vendor's own JSON, and so is
//! an `example`, a `default`, an `enum`, a `const` and the `value` of an
//! Example Object: nothing under them is read.
//!
//! What the rule costs is a place the walk takes for a name or for data and
//! that the specification gives fixed fields: a marker standing there is a
//! marker missed, never a document wrongly refused.

use serde_json::{Map, Value};

use crate::model::LoadError;
use crate::required::{Step, spelled};

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

/// Every key this crate reads. A key under the prefix that is not one of these
/// is read by nothing.
const READ: [&str; 5] = [COMMAND, GATES, GROUP, WRITES, REQUIRES];

/// The keys under which the specification keeps instance data or the vendor's
/// own JSON, and nothing under them is a part of the document.
///
/// `security` is here for another reason: its entries are keyed by the names of
/// security schemes, and their values are lists of scopes.
const OPAQUE: [&str; 6] = ["example", "default", "enum", "const", "value", "security"];

/// The keys whose object is a map from names somebody chose to the objects that
/// carry them: properties, components, media types, headers.
///
/// A key of the same spelling over a list leads to the list's members instead,
/// as `parameters` does on an operation and under `components`.
const NAMED: [&str; 20] = [
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
    "schemas",
    "parameters",
    "examples",
    "requestBodies",
    "headers",
    "securitySchemes",
    "links",
    "callbacks",
    "pathItems",
    "webhooks",
    "content",
    "encoding",
    "variables",
    "mapping",
    "scopes",
];

/// The methods a path item names an operation under.
const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

/// What the reasons of a refusal say about where `key` is read.
///
/// The message of `LoadError::MisplacedMarker` ends with it.
pub(crate) fn reading(key: &str) -> &'static str {
    if key == REQUIRES {
        "it reads `x-cli-requires` on a parameter, and not beside a `$ref`"
    } else {
        "it reads that key on an operation"
    }
}

/// The keys this crate reads, spelled for a message.
pub(crate) fn vocabulary() -> &'static str {
    "`x-cli-command`, `x-cli-gates`, `x-cli-group`, `x-cli-writes` and `x-cli-requires`"
}

/// Refuse the first `x-cli-` key of the document that this crate reads nowhere
/// or does not read where it stands, in the order the document lists them.
pub(crate) fn check(document: &Value) -> Result<(), LoadError> {
    Walk { at: Vec::new() }.read(Level::Fixed, document)
}

/// What the keys of an object are.
#[derive(Debug, Clone, Copy)]
enum Level {
    /// An object the specification gives fixed fields. An `x-…` key is an
    /// extension, and any other key is a field.
    Fixed,
    /// A map from names somebody chose. Every key is a name, `x-cli-writes`
    /// among them.
    Names,
    /// A map of names that the specification also lets a vendor extend: the
    /// Paths Object and the Responses Object. An `x-…` key is an extension, and
    /// any other key is a name.
    NamesAndExtensions,
}

/// Where an `x-cli-` key stands, as far as the question of who reads it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// An operation of a path item under `paths`.
    Operation,
    /// A parameter a request is built from: a path item's, an operation's, or
    /// one that a `$ref` can name under `components.parameters`.
    Parameter,
    /// Anywhere else.
    Elsewhere,
}

impl Place {
    /// Where the object holding a key stands, read off the way down to it.
    fn of(at: &[Step<'_>]) -> Self {
        use Step::{Index, Key};
        match at {
            [Key("paths"), Key(path), Key(method)]
                if path.starts_with('/') && is_method(method) =>
            {
                Self::Operation
            }
            [Key("paths"), Key(path), Key("parameters"), Index(_)] if path.starts_with('/') => {
                Self::Parameter
            }
            [
                Key("paths"),
                Key(path),
                Key(method),
                Key("parameters"),
                Index(_),
            ] if path.starts_with('/') && is_method(method) => Self::Parameter,
            [Key("components"), Key("parameters"), Key(_)] => Self::Parameter,
            _ => Self::Elsewhere,
        }
    }
}

fn is_method(name: &str) -> bool {
    METHODS.contains(&name)
}

/// The descent, with the way down to the object being read.
struct Walk<'d> {
    at: Vec<Step<'d>>,
}

impl<'d> Walk<'d> {
    /// This node, then everything under it that the document gives fixed
    /// fields. A list stands for each of its members.
    fn read(&mut self, level: Level, node: &'d Value) -> Result<(), LoadError> {
        match node {
            Value::Object(fields) => fields
                .iter()
                .try_for_each(|(key, value)| self.field(level, fields, key, value)),
            Value::Array(members) => members.iter().enumerate().try_for_each(|(index, member)| {
                self.at.push(Step::Index(index));
                let read = self.read(level, member);
                self.at.pop();
                read
            }),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        }
    }

    /// One field of `holder`: an extension, which is asked whether this crate
    /// reads it, or a key whose value is read with the right notion of what
    /// stands under it.
    fn field(
        &mut self,
        level: Level,
        holder: &'d Map<String, Value>,
        key: &'d str,
        value: &'d Value,
    ) -> Result<(), LoadError> {
        if !matches!(level, Level::Names) && key.starts_with("x-") {
            // The value of an extension is the vendor's own JSON.
            return if key.starts_with(PREFIX) {
                self.marker(key, holder)
            } else {
                Ok(())
            };
        }
        let below = match level {
            Level::Fixed => below(self.at.last().copied(), key, value),
            Level::Names | Level::NamesAndExtensions => Some(Level::Fixed),
        };
        let Some(below) = below else {
            return Ok(());
        };
        self.at.push(Step::Key(key));
        let read = self.read(below, value);
        self.at.pop();
        read
    }

    /// An `x-cli-` key of `holder`: one this crate reads, standing where it is
    /// read, or a refusal that names the key and the object it stands on.
    fn marker(&self, key: &str, holder: &Map<String, Value>) -> Result<(), LoadError> {
        let at = || spelled(&self.at);
        if !READ.contains(&key) {
            return Err(LoadError::UnknownMarker {
                key: key.to_owned(),
                at: at(),
            });
        }
        let place = Place::of(&self.at);
        let read = match key {
            REQUIRES => place == Place::Parameter && !holder.contains_key("$ref"),
            _ => place == Place::Operation,
        };
        if read {
            return Ok(());
        }
        Err(LoadError::MisplacedMarker {
            key: key.to_owned(),
            at: at(),
        })
    }
}

/// What stands under a field of a [`Level::Fixed`] object, and `None` where
/// nothing under it is a part of the document.
///
/// `parent` is the key the object itself stands under. It tells the Responses
/// Object of an operation from `components.responses`, which is a plain map of
/// names.
fn below(parent: Option<Step<'_>>, key: &str, value: &Value) -> Option<Level> {
    if OPAQUE.contains(&key) {
        return None;
    }
    if !value.is_object() {
        return Some(Level::Fixed);
    }
    Some(match (parent, key) {
        (Some(Step::Key("components")), "responses") => Level::Names,
        (_, "paths" | "responses") => Level::NamesAndExtensions,
        _ if NAMED.contains(&key) => Level::Names,
        _ => Level::Fixed,
    })
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
        ] {
            assert_eq!(verdict(document), misplaced(key, at), "{document}");
        }
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
