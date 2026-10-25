//! Refusing a `pattern` that the Unicode flag reads another way, wherever a
//! schema states one.
//!
//! The reduction refuses it on a `pattern` that a scalar carries
//! (`runnable_scalar`: a parameter, or a property of a JSON body), and those are
//! a few places out of many. typify builds a type from every named schema, from
//! an inline body and from an inline response, and writes
//! `Regex::new("^\\p{L}+$")` into each `pattern` it meets: a type that decides
//! `Müller` the other way from a vendor's server that runs the pattern with the
//! flag, and so refuses the vendor's own responses that carry a name. So the
//! walk here judges every schema the object model reads by the `pattern` it
//! holds, and not by whether a scalar or a type is made of it. It runs after the
//! reduction, so a `pattern` that a scalar carries is refused there, with the
//! value that holds it, and the walk refuses every other.
//!
//! It reaches what OpenAPI 3.0.3 defines, as the model does, and so as typify
//! does: a schema stated under a key the model drops is no schema of the
//! document. Neither is anything that stands beside a `$ref` that is text, in an
//! object that can be a reference, which the model reads as the reference alone.
//! In any other object a `$ref` is a key like any other. The walk reaches a few
//! schemas that no type and no flag is made of, such as the schema of an error
//! response. The pattern in one is the same disagreement, and the Overlay line
//! that repairs it is the line that repairs the others.

use serde_json::{Map, Value};

use crate::model::LoadError;
use crate::required::{Step, spelled};
use crate::scalar;
use crate::shape::{Entry, Keys, Kind, Reach, Route};

/// The walk reads what the model reads.
const REACH: Reach = Reach::Defined;

/// A schema's regular expression.
const PATTERN: &str = "pattern";

/// Refuse the first schema of the document, in the order the document lists
/// them, whose `pattern` holds `\p{`, `\P{` or `\u{`.
pub(crate) fn check(document: &Value) -> Result<(), LoadError> {
    Walk {
        document,
        at: Vec::new(),
    }
    .read(Kind::Root, document)
}

/// The head of a refusal: the operation, when the schema stands in one.
pub(crate) fn held_by(op: Option<&str>) -> String {
    op.map_or_else(String::new, |op| format!("{op}: "))
}

/// The descent, with the way down to the object being read.
struct Walk<'d> {
    document: &'d Value,
    at: Vec<Step<'d>>,
}

impl<'d> Walk<'d> {
    /// This node, then everything under it that the table leads to. A list
    /// stands for each of its members. A node that the model reads as a
    /// reference is read as one: it holds nothing to judge, and nothing beside
    /// the `$ref` is entered.
    fn read(&mut self, kind: Kind, node: &'d Value) -> Result<(), LoadError> {
        match node {
            Value::Object(fields) => {
                if kind.is_read_as_reference(fields) {
                    return Ok(());
                }
                if matches!(kind, Kind::Schema) {
                    self.judge(fields)?;
                }
                fields
                    .iter()
                    .try_for_each(|(key, value)| self.field(kind, key, value))
            }
            Value::Array(members) => members.iter().enumerate().try_for_each(|(index, member)| {
                self.at.push(Step::Index(index));
                let read = self.read(kind, member);
                self.at.pop();
                read
            }),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        }
    }

    /// One field of an object of this kind, entered where the table leads there
    /// and nowhere else: a `pattern` is read off the schema that holds it, and
    /// never off a `properties` map that has a property called `pattern`.
    fn field(&mut self, kind: Kind, key: &'d str, value: &'d Value) -> Result<(), LoadError> {
        let Some(route) = kind.route(key, REACH) else {
            return Ok(());
        };
        self.at.push(Step::Key(key));
        let read = match route {
            Route::Node(kind) => self.read(kind, value),
            Route::Map(kind, keys) => self.entries(kind, keys, value),
        };
        self.at.pop();
        read
    }

    /// Every entry of a map of nodes of one kind. The keys of the map are names,
    /// and an `x-…` key of the Paths Object or the Responses Object is the
    /// vendor's own JSON.
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
                Some(Entry::Extension) | None => Ok(()),
            })
    }

    /// Whether the `pattern` of this schema, if it has one, means the same with
    /// the Unicode flag as without it.
    fn judge(&self, schema: &Map<String, Value>) -> Result<(), LoadError> {
        let Some(pattern) = schema.get(PATTERN).and_then(Value::as_str) else {
            return Ok(());
        };
        scalar::unflagged(pattern).map_err(|source| LoadError::UnrunnablePattern {
            op: self.operation(),
            at: spelled(&self.at),
            source,
        })
    }

    /// The `operationId` of the operation the schema stands in, when it stands in
    /// one and the document gives it an id: under a method of a path item, a
    /// callback of that operation included. The load refuses an operation that
    /// has no id before this walk runs.
    fn operation(&self) -> Option<String> {
        let [Step::Key("paths"), Step::Key(path), Step::Key(method), ..] = self.at.as_slice()
        else {
            return None;
        };
        self.document
            .get("paths")?
            .get(*path)?
            .get(*method)?
            .get("operationId")?
            .as_str()
            .map(str::to_owned)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    /// Where the walk finds the pattern, as the refusal prints it, and the
    /// operation that holds it.
    fn found(document: &str) -> Option<(Option<String>, String)> {
        let value = crate::overlay::parse(document).expect("the fixture parses");
        match check(&value) {
            Ok(()) => None,
            Err(LoadError::UnrunnablePattern { op, at, .. }) => Some((op, at)),
            Err(other) => panic!("only a pattern is refused here: {other:?}"),
        }
    }

    const HEAD: &str = "paths:\n  /a:\n    get:\n      operationId: getA\n";

    /// The walk finds the pattern of `document` where and under the operation
    /// that the case says.
    fn assert_found(document: &str, op: Option<&str>, at: &str) {
        assert_eq!(
            found(document),
            Some((op.map(str::to_owned), at.to_owned())),
            "{document}"
        );
    }

    #[test]
    fn a_pattern_is_read_in_the_schema_that_holds_it() {
        let schema = "{ type: string, pattern: '^\\p{L}+$' }";
        for (document, op, at) in [
            (
                format!(
                    "{HEAD}      responses:\n        '200':\n          content:\n            \
                     application/json:\n              schema: {schema}\n"
                ),
                Some("getA"),
                "$.paths['/a'].get.responses['200'].content['application/json'].schema",
            ),
            (
                format!("components:\n  schemas:\n    Name: {schema}\n"),
                None,
                "$.components.schemas.Name",
            ),
            (
                format!(
                    "components:\n  schemas:\n    Person:\n      type: object\n      \
                     properties:\n        surname: {schema}\n"
                ),
                None,
                "$.components.schemas.Person.properties.surname",
            ),
            (
                format!(
                    "{HEAD}      parameters:\n        - name: n\n          in: query\n          schema: {{ type: array, items: {schema} }}\n"
                ),
                Some("getA"),
                "$.paths['/a'].get.parameters[0].schema.items",
            ),
            (
                format!(
                    "paths:\n  /a:\n    parameters:\n      - name: n\n        in: query\n        \
                     schema: {schema}\n"
                ),
                None,
                "$.paths['/a'].parameters[0].schema",
            ),
            (
                format!("components:\n  schemas:\n    Any:\n      allOf:\n        - {schema}\n"),
                None,
                "$.components.schemas.Any.allOf[0]",
            ),
        ] {
            assert_found(&document, op, at);
        }
    }

    /// Documents in which a `$ref` is a key like any other, and the schema beside it
    /// is a schema of the document: a `$ref` that is not text, and a `$ref` in an
    /// object that cannot be a reference (the Components Object, a media type, an
    /// operation, the path item of a callback and an encoding). Each is the
    /// document, the operation that holds the schema and where the schema is.
    fn refs_that_are_no_reference() -> Vec<(String, Option<&'static str>, &'static str)> {
        let schema = "{ type: string, pattern: '^\\p{L}+$' }";
        let answer = format!(
            "{{ description: OK, content: {{ application/json: {{ schema: {schema} }} }} }}"
        );
        vec![
            (
                format!("components:\n  $ref: '#/nowhere'\n  schemas:\n    P: {schema}\n"),
                None,
                "$.components.schemas.P",
            ),
            (
                format!(
                    "{HEAD}      responses:\n        '200':\n          description: ok\n          content:\n            \
                     application/json:\n              $ref: '#/nowhere'\n              schema: {schema}\n"
                ),
                Some("getA"),
                "$.paths['/a'].get.responses['200'].content['application/json'].schema",
            ),
            (
                format!(
                    "{HEAD}      $ref: '#/nowhere'\n      responses:\n        '200': {answer}\n"
                ),
                Some("getA"),
                "$.paths['/a'].get.responses['200'].content['application/json'].schema",
            ),
            (
                format!(
                    "{HEAD}      callbacks:\n        cb:\n          '{{$request.body#/url}}':\n            \
                     $ref: '#/nowhere'\n            post:\n              responses:\n                '200': {answer}\n"
                ),
                Some("getA"),
                "$.paths['/a'].get.callbacks.cb['{$request.body#/url}'].post.responses['200']\
                 .content['application/json'].schema",
            ),
            (
                format!(
                    "{HEAD}      requestBody:\n        content:\n          multipart/form-data:\n            \
                     encoding:\n              f:\n                $ref: '#/nowhere'\n                \
                     headers: {{ X-Name: {{ schema: {schema} }} }}\n"
                ),
                Some("getA"),
                "$.paths['/a'].get.requestBody.content['multipart/form-data'].encoding.f\
                 .headers['X-Name'].schema",
            ),
            // A `$ref` that is not text is no reference: the model reads the
            // node as the schema it is.
            (
                "components:\n  schemas:\n    Odd: { $ref: 7, type: string, pattern: '^\\p{L}+$' }\n"
                    .to_owned(),
                None,
                "$.components.schemas.Odd",
            ),
        ]
    }

    #[test]
    fn a_ref_that_is_no_reference_leaves_the_schema_beside_it_read() {
        for (document, op, at) in refs_that_are_no_reference() {
            assert_found(&document, op, at);
        }
    }

    #[test]
    fn a_pattern_is_not_read_where_no_schema_states_it() {
        for document in [
            // A property called `pattern` is a name, and its schema holds none.
            "components:\n  schemas:\n    P:\n      type: object\n      properties:\n        pattern: { type: string }\n",
            // The value of an extension, an `example` and an `enum` are data.
            "components:\n  schemas:\n    P:\n      type: string\n      x-note: { pattern: '\\p{L}' }\n      example: { pattern: '\\p{L}' }\n      enum: ['\\p{L}']\n",
            // A key that 3.0.3 does not define: the model drops it.
            "components:\n  schemas:\n    P:\n      type: object\n      $defs:\n        N: { type: string, pattern: '\\p{L}' }\n",
            "webhooks:\n  w:\n    post:\n      parameters:\n        - { name: n, in: query, schema: { type: string, pattern: '\\p{L}' } }\n",
            // The Paths Object and the Responses Object hold the vendor's own
            // JSON under an `x-…` key.
            "paths:\n  x-vendor:\n    get:\n      parameters:\n        - { name: n, in: query, schema: { type: string, pattern: '\\p{L}' } }\n",
            "paths:\n  /a:\n    get:\n      responses:\n        x-vendor:\n          content: { application/json: { schema: { type: string, pattern: '\\p{L}' } } }\n",
            // An object that is no schema may carry a key spelled `pattern`.
            "components:\n  parameters:\n    N: { name: n, in: query, pattern: '\\p{L}' }\n",
            // A pattern that is not text states nothing here.
            "components:\n  schemas:\n    P: { type: string, pattern: 7 }\n",
            // The model reads a node that holds a string `$ref` as the
            // reference alone, so nothing beside it is a schema of the document:
            // not a `pattern`, and not a `properties` map with one inside.
            "components:\n  schemas:\n    P:\n      type: object\n      properties:\n        s: { $ref: '#/components/schemas/Plain', pattern: '\\p{L}' }\n",
            "paths:\n  /a:\n    get:\n      responses:\n        '200':\n          description: ok\n          content: { application/json: { schema: { $ref: '#/components/schemas/Plain', pattern: '\\p{L}' } } }\n",
            "paths:\n  /a:\n    get:\n      parameters:\n        - { name: n, in: query, schema: { $ref: '#/components/schemas/Plain', pattern: '\\p{L}' } }\n",
            "components:\n  schemas:\n    P:\n      $ref: '#/components/schemas/Plain'\n      properties:\n        s: { type: string, pattern: '\\p{L}' }\n",
            "paths:\n  /a:\n    get:\n      parameters:\n        - $ref: '#/components/parameters/N'\n          schema: { type: string, pattern: '\\p{L}' }\n",
            "paths:\n  /a:\n    get:\n      responses:\n        '200':\n          $ref: '#/components/responses/R'\n          content: { application/json: { schema: { type: string, pattern: '\\p{L}' } } }\n",
            "paths:\n  /a:\n    post:\n      requestBody:\n        $ref: '#/components/requestBodies/B'\n        content: { application/json: { schema: { type: string, pattern: '\\p{L}' } } }\n",
            "paths:\n  /a:\n    get:\n      responses:\n        '200':\n          description: ok\n          headers:\n            X-Name:\n              $ref: '#/components/headers/H'\n              schema: { type: string, pattern: '\\p{L}' }\n",
            // An operation holds its callbacks as callbacks and not as references,
            // so a callback is a reference under the components only.
            "components:\n  callbacks:\n    C:\n      $ref: '#/components/callbacks/D'\n      '{$request.body#/url}': { post: { parameters: [{ name: n, in: query, schema: { type: string, pattern: '\\p{L}' } }] } }\n",
        ] {
            assert_eq!(found(document), None, "{document}");
        }
    }

    #[test]
    fn an_operation_without_an_id_is_named_by_its_place() {
        let document = "paths:\n  /a:\n    get:\n      parameters:\n        - { name: n, in: query, schema: { type: string, pattern: '\\p{L}' } }\n";
        assert_eq!(
            found(document),
            Some((None, "$.paths['/a'].get.parameters[0].schema".to_owned()))
        );
    }
}
