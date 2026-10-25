//! Where the specification puts each object of an OpenAPI 3.0 document: which
//! key of which object leads to another object, and what the keys of a map are.
//!
//! Three walks read the overlaid document. [`required`](crate::required) and
//! [`markers`](crate::markers) read it before the object model does, and look
//! for a schema that requires a key nothing declares and for an `x-cli-` key that
//! nothing reads. [`patterns`](crate::patterns) reads it after the reduction, and
//! looks for a `pattern` that the Unicode flag reads another way. Each has to
//! tell an object the specification gives fixed fields from a name somebody chose
//! and from JSON that is the vendor's own, so they read one table.
//!
//! # The table is a list of what is entered
//!
//! [`Kind::route`] names the keys that lead to an object of the document, to a
//! list of them, or to a map of them. A walk enters those and reads nothing
//! under any other key:
//!
//! - a field of type Any: an `example`, a `default`, an `enum`, the `value` of
//!   an Example Object, and the `parameters` values and the `requestBody` of a
//!   Link Object;
//! - the value of an extension, which is the vendor's own JSON;
//! - a key that the object does not define: `examples` and `dependencies` on a
//!   3.0 schema.
//!
//! A list of what is entered fails toward a position unread. The schema walk
//! misses a phantom there, the marker walk a marker, and the pattern walk a
//! `pattern`. A list of what is not entered fails toward a document wrongly
//! refused at every field nobody put on it, and no walk may do that.
//!
//! An `x-…` key of an object that a walk enters is an extension, and the walk
//! asks about it by its name. In a map of names somebody chose it is a name like
//! any other.
//!
//! # What OpenAPI 3.0.3 defines, and what only the schema walk enters
//!
//! A route is one that OpenAPI 3.0.3 defines, which is what the object model
//! reads, or one that a 3.0 document may carry and the model drops. The second
//! kind is a field of a later version (`webhooks`, `pathItems`), a keyword of a
//! later draft (`$defs`, `if`), or a key that no version defines: a key of the
//! Paths Object that is no path, and a key of the Responses Object that is no
//! status code. A walk says how far it reaches with a [`Reach`].
//!
//! Every walk enters what 3.0.3 defines, so a map added to the table for a route
//! of that kind is added for all of them. Only the schema walk enters the rest, and
//! [`Reach::Whole`] is where the table says why:
//!
//! - The marker walk judges a key by where it stands. Nothing builds a request
//!   or a flag from what stands under a key the model drops, so a refusal there
//!   refuses a document for data that nobody reads, and a walk that reads
//!   `webhooks` as path items reads a document that says it is 3.0 as 3.1.
//! - The schema walk judges a schema by what it holds and not by where it
//!   stands. Wherever it can tell that a name is declared nowhere, it refuses
//!   the phantom, also where typify writes no type for the schema, and the
//!   refusal names an Overlay.

use openapiv3::StatusCode;
use serde::Deserialize as _;
use serde::de::value::{Error, StrDeserializer};
use serde_json::{Map, Value};

/// The key that makes an object a reference to another object of the document.
const REF: &str = "$ref";

/// A specification extension: where the specification lets an object carry one,
/// an `x-…` key holds the vendor's own arbitrary JSON rather than a part of the
/// document. Where it does not, the same spelling is an ordinary name, so the
/// callers of this are the positions the specification names and no others.
pub(crate) fn is_extension(key: &str) -> bool {
    key.starts_with("x-")
}

/// The keys a schema declares, and the one place a name is declared at all.
pub(crate) const PROPERTIES: &str = "properties";
/// The schema an array states its elements against.
pub(crate) const ITEMS: &str = "items";
/// What a node composes with, beside the `$ref` read alongside them. Their
/// members declare for the same value the node does, and are read as
/// declarations.
pub(crate) const COMPOSED: [&str; 3] = ["allOf", "anyOf", "oneOf"];
/// The keywords whose subschemas also constrain the value the node holding them
/// constrains, and out of which the schema walk of [`required`](crate::required)
/// reads no name: a `not` declares by ruling out, and a `then` or an `else`
/// declares only of the instances that took that branch. A declaration may stand
/// behind one, and typify writes no field out of one either — an uninhabited
/// enum for a `not`, no type at all for the other three — so a node carrying one
/// is exempt.
pub(crate) const CONDITIONAL: [&str; 4] = ["not", "if", "then", "else"];
/// The keyword whose subschemas are about the object a property stands in
/// rather than about that property's value. `dependentSchemas: { foo: {
/// required: [bar] } }` is the object requiring its own `bar` whenever it
/// carries `foo`, so the object's `properties` are where `bar` is declared.
pub(crate) const DEPENDENT: &str = "dependentSchemas";

/// Where a node stands in the document.
///
/// The document's own shape is what says what a node is, so a walk carries that
/// answer down rather than asking each node what it looks like. Every kind but
/// [`Kind::Schema`] is a place on the way to one or an object that holds none,
/// and the schema walk judges nothing but a schema.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Kind {
    /// The document itself.
    Root,
    /// The Info Object.
    Info,
    /// The Contact Object of the Info Object.
    Contact,
    /// The License Object of the Info Object.
    License,
    /// A Server Object.
    Server,
    /// A Server Variable Object.
    ServerVariable,
    /// A Tag Object.
    Tag,
    /// An External Documentation Object.
    ExternalDocs,
    /// `components`, holding the halves of everything else under a name.
    Components,
    /// The operations on one path, and the parameters they share.
    PathItem,
    /// One operation.
    Operation,
    /// The callbacks one operation may make, keyed by the runtime expressions
    /// that address them.
    Callback,
    /// One parameter.
    Parameter,
    /// One header. It is a parameter that does not have to name itself, and it
    /// leads to a schema by the same two keys.
    Header,
    /// A request body.
    RequestBody,
    /// One response.
    Response,
    /// A Link Object. Its `parameters` and its `requestBody` are values of type
    /// Any, so only its `server` leads anywhere.
    Link,
    /// One entry of a `content` map.
    MediaType,
    /// How one property of a body is carried.
    Encoding,
    /// An Example Object. Its `value` is of type Any.
    Example,
    /// A Security Scheme Object.
    SecurityScheme,
    /// The OAuth Flows Object of a Security Scheme.
    OAuthFlows,
    /// One OAuth Flow Object.
    OAuthFlow,
    /// A schema, and the only kind the schema walk judges.
    Schema,
    /// The Discriminator Object of a schema.
    Discriminator,
    /// The XML Object of a schema.
    Xml,
}

/// What stands under a key that leads somewhere.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Route {
    /// One node of this kind — or, where the key holds a list of them, each
    /// member of it.
    Node(Kind),
    /// A map whose values are each one node of this kind, under keys of the
    /// sort [`Keys`] says.
    Map(Kind, Keys),
}

/// How much of the table a walk enters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reach {
    /// What OpenAPI 3.0.3 defines, and so what the object model reads. The walk
    /// of [`markers`](crate::markers) reaches this far, because it judges a key
    /// by where it stands, and a key under a position that the model drops
    /// stands nowhere. So does the walk of [`patterns`](crate::patterns), because
    /// a generated type is made of what the model reads and of nothing it drops.
    Defined,
    /// What 3.0.3 defines, and the routes and keys beside it that a 3.0
    /// document may carry and the model drops. The walk of
    /// [`required`](crate::required) reaches this far, because it judges a
    /// schema by what it holds, wherever the document puts it.
    Whole,
}

/// What the keys of a map are.
///
/// The specification names the objects a vendor may hang JSON of its own
/// inside, and two of them are maps a walk enters as maps: the Paths Object and
/// the Responses Object. A Callback Object is the third, and it is read a key at
/// a time instead, because nothing but the runtime expressions names its
/// entries.
///
/// Every other map is keyed by names somebody chose — a component, a response
/// header, a media type, a property — and there `x-total` is a name like any
/// other. Reading it as an aside would leave whatever stands under it unread.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Keys {
    /// Every key names one entry.
    Names,
    /// The Paths Object. A key that starts with `/` names a path, an `x-…` key
    /// is the vendor's own JSON, and any other key is no path: the model reads
    /// the keys that start with a `/` and no others.
    Paths,
    /// The Responses Object. `default` and an HTTP status code (`200`, `2XX`)
    /// name a response, an `x-…` key is the vendor's own JSON, and any other
    /// key is no response: the model keeps the keys that
    /// `openapiv3::StatusCode` reads and no others.
    Responses,
}

/// What stands under one key of a map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    /// One entry of the map: a node to enter.
    Member,
    /// The vendor's own JSON, standing beside the entries.
    Extension,
}

impl Keys {
    /// What `key` is in a map of this sort, for a walk that reaches this far,
    /// and `None` for a key that the walk does not enter.
    pub(crate) fn entry(self, key: &str, reach: Reach) -> Option<Entry> {
        if !matches!(self, Self::Names) && is_extension(key) {
            Some(Entry::Extension)
        } else if self.names(key) || reach == Reach::Whole {
            Some(Entry::Member)
        } else {
            None
        }
    }

    /// Whether `key` names an entry that 3.0.3 defines.
    fn names(self, key: &str) -> bool {
        match self {
            Self::Names => true,
            Self::Paths => key.starts_with('/'),
            Self::Responses => key == "default" || is_status_code(key),
        }
    }
}

/// Whether the object model keeps a key of the Responses Object as a response:
/// it asks `StatusCode` itself, so the two cannot disagree about a spelling
/// such as `2xx`.
fn is_status_code(key: &str) -> bool {
    StatusCode::deserialize(StrDeserializer::<Error>::new(key)).is_ok()
}

impl Kind {
    /// What one of this node's keys leads to for a walk that reaches this far,
    /// and `None` for a key that leads to no object, or to one the walk does
    /// not enter.
    ///
    /// This is the whole of what a walk is allowed to enter. A position left out
    /// is a node unread, and a position wrongly put in is a document wrongly
    /// refused — so the table names what the specification names, and a shape
    /// the specification leaves to the vendor is not in it.
    pub(crate) fn route(self, key: &str, reach: Reach) -> Option<Route> {
        self.defined(key).or_else(|| match reach {
            Reach::Defined => None,
            Reach::Whole => self.undefined(key),
        })
    }

    /// Whether the object model reads this object as the reference it holds and
    /// as nothing else: an object of a kind that can be a reference, with a
    /// `$ref` that is text.
    ///
    /// `openapiv3` reads such a node as `ReferenceOr::Reference` and drops every
    /// other key, as OpenAPI 3.0.3 says of the Reference Object: "any properties
    /// added SHALL be ignored". So a walk that reaches what the model reads stops
    /// at one, and what stands beside the `$ref` is no part of the document. A
    /// `$ref` that is not text is no reference, and the node is the object it is.
    ///
    /// A path item is not in the list. The model reads one as a reference under
    /// `paths` only, and the load refuses a path item that is a `$ref`
    /// (`LoadError::PathItem`) before a walk that reads the model asks.
    pub(crate) fn is_read_as_reference(self, fields: &Map<String, Value>) -> bool {
        let referable = match self {
            Self::Schema
            | Self::Parameter
            | Self::Header
            | Self::RequestBody
            | Self::Response
            | Self::Link
            | Self::Example
            | Self::Callback
            | Self::SecurityScheme => true,
            Self::Root
            | Self::Info
            | Self::Contact
            | Self::License
            | Self::Server
            | Self::ServerVariable
            | Self::Tag
            | Self::ExternalDocs
            | Self::Components
            | Self::PathItem
            | Self::Operation
            | Self::MediaType
            | Self::Encoding
            | Self::OAuthFlows
            | Self::OAuthFlow
            | Self::Discriminator
            | Self::Xml => false,
        };
        referable && fields.get(REF).is_some_and(Value::is_string)
    }

    /// The routes that OpenAPI 3.0.3 defines.
    fn defined(self, key: &str) -> Option<Route> {
        use Keys::{Names, Paths, Responses};
        use Kind::{
            Callback, Components, Contact, Encoding, Example, ExternalDocs, Header, Info, License,
            Link, MediaType, OAuthFlow, OAuthFlows, Operation, Parameter, PathItem, RequestBody,
            Response, Root, Schema, SecurityScheme, Server, ServerVariable, Tag,
        };
        use Route::{Map, Node};
        // Grouped by where a key leads and by what the keys under it are,
        // rather than by the node it sits on, so that no two arms say the same
        // thing.
        Some(match (self, key) {
            (Root, "components") => Node(Components),
            // The Paths Object is one of the two maps here that the
            // specification also lets a vendor hang JSON of its own inside.
            (Root, "paths") => Map(PathItem, Paths),
            (Root, "info") => Node(Info),
            (Info, "contact") => Node(Contact),
            (Info, "license") => Node(License),
            (Root | PathItem | Operation, "servers") | (Link, "server") => Node(Server),
            (Server, "variables") => Map(ServerVariable, Names),
            (Root, "tags") => Node(Tag),
            (Root | Tag | Operation, "externalDocs") => Node(ExternalDocs),
            // Nothing names a callback's keys but the expressions themselves,
            // so each of them addresses a path item — bar an extension, which
            // is the vendor's JSON standing beside them rather than one more.
            (Callback, key) if !is_extension(key) => Node(PathItem),
            (Components | Operation, "callbacks") => Map(Callback, Names),
            (
                PathItem,
                "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace",
            ) => Node(Operation),
            (PathItem | Operation, "parameters") => Node(Parameter),
            (Components, "parameters") => Map(Parameter, Names),
            (Operation, "requestBody") => Node(RequestBody),
            (Components, "requestBodies") => Map(RequestBody, Names),
            // The Responses Object is the other one. `components.responses` is
            // a plain map, where a response called `x-problem` is a response.
            (Operation, "responses") => Map(Response, Responses),
            (Components, "responses") => Map(Response, Names),
            (Components | Response | Encoding, "headers") => Map(Header, Names),
            (Components | Response, "links") => Map(Link, Names),
            (Parameter | Header | RequestBody | Response, "content") => Map(MediaType, Names),
            (MediaType, "encoding") => Map(Encoding, Names),
            (Components | Parameter | Header | MediaType, "examples") => Map(Example, Names),
            (Parameter | Header | MediaType, "schema") => Node(Schema),
            (Components, "schemas") => Map(Schema, Names),
            (Components, "securitySchemes") => Map(SecurityScheme, Names),
            (SecurityScheme, "flows") => Node(OAuthFlows),
            (OAuthFlows, "implicit" | "password" | "clientCredentials" | "authorizationCode") => {
                Node(OAuthFlow)
            }
            (Schema, key) => return keyword(key),
            _ => return None,
        })
    }

    /// The routes that a 3.0 document may carry and OpenAPI 3.0.3 does not
    /// define, which the object model drops and only [`Reach::Whole`] enters.
    ///
    /// The phantom check judges a schema by what it holds, not by where it
    /// stands, so it reads a schema here as it reads one anywhere and refuses a
    /// `required` that names a key nothing declares, although typify writes no
    /// type for some of these. The refusal names an Overlay, which is the
    /// repair wherever the schema stands.
    fn undefined(self, key: &str) -> Option<Route> {
        use Keys::Names;
        use Kind::{Components, PathItem, Root, Schema};
        use Route::Map;
        match (self, key) {
            // Fields of 3.1: plain maps of path items under names somebody
            // chose.
            (Root, "webhooks") | (Components, "pathItems") => Some(Map(PathItem, Names)),
            (Schema, key) => later_keyword(key),
            _ => None,
        }
    }
}

/// What one of a schema's keywords leads to, among those that OpenAPI 3.0.3
/// defines.
///
/// `properties`, `items`, `additionalProperties`, the three compositions and
/// `not` lead to a schema, or to a list or a map of them, and `discriminator`,
/// `xml` and `externalDocs` lead to a small object of their own. A keyword left
/// out states something other than an object — a `type`, a bound, a `pattern` —
/// or is no keyword of a 3.0 schema, as `examples` and `dependencies` are not,
/// and nothing under such a keyword is entered.
fn keyword(key: &str) -> Option<Route> {
    Some(match key {
        PROPERTIES => Route::Map(Kind::Schema, Keys::Names),
        ITEMS | "additionalProperties" | "not" => Route::Node(Kind::Schema),
        key if COMPOSED.contains(&key) => Route::Node(Kind::Schema),
        "discriminator" => Route::Node(Kind::Discriminator),
        "xml" => Route::Node(Kind::Xml),
        "externalDocs" => Route::Node(Kind::ExternalDocs),
        _ => return None,
    })
}

/// The keywords of a later draft of JSON Schema that lead to a schema, which a
/// 3.0 schema does not have and which typify reads.
///
/// They are the other half of what the schema walk follows: a 3.0 document may
/// carry any of them, and the model drops them without a word.
fn later_keyword(key: &str) -> Option<Route> {
    Some(match key {
        "patternProperties" | "$defs" | "definitions" | DEPENDENT => {
            Route::Map(Kind::Schema, Keys::Names)
        }
        "prefixItems"
        | "unevaluatedProperties"
        | "unevaluatedItems"
        | "propertyNames"
        | "contains"
        | "contentSchema"
        | "if"
        | "then"
        | "else" => Route::Node(Kind::Schema),
        _ => return None,
    })
}
