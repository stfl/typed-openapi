//! Where the specification puts each object of an OpenAPI 3.0 document: which
//! key of which object leads to another object, and what the keys of a map are.
//!
//! Two walks read the overlaid document before the object model does.
//! [`required`](crate::required) looks for a schema that requires a key nothing
//! declares, and [`markers`](crate::markers) looks for an `x-cli-` key that
//! nothing reads. Each has to tell an object the specification gives fixed
//! fields from a name somebody chose and from JSON that is the vendor's own, so
//! they read one table, and a map added to it is added for both.
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
//! misses a phantom there, and the marker walk misses a marker. A list of what is
//! not entered fails toward a document wrongly refused at every field nobody put
//! on it, and neither walk may do that.
//!
//! An `x-…` key of an object that a walk enters is an extension, and the walk
//! asks about it by its name. In a map of names somebody chose it is a name like
//! any other.

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
    /// A map whose values are each one node of this kind, under keys the
    /// specification either lets a vendor write beside or does not.
    Map(Kind, Keys),
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
    /// Every key names one entry but an `x-…`, which is the vendor's own JSON
    /// standing beside them.
    NamesAndExtensions,
}

impl Kind {
    /// What one of this node's keys leads to, and `None` for a key that leads to
    /// no object.
    ///
    /// This is the whole of what a walk is allowed to enter. A position left out
    /// is a node unread, and a position wrongly put in is a document wrongly
    /// refused — so it names what the specification names, and a shape the
    /// specification leaves to the vendor is not in it.
    pub(crate) fn route(self, key: &str) -> Option<Route> {
        use Keys::{Names, NamesAndExtensions};
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
            // `webhooks` and `components.pathItems` are plain maps of path
            // items under names somebody chose.
            (Root, "paths") => Map(PathItem, NamesAndExtensions),
            (Root, "webhooks") | (Components, "pathItems") => Map(PathItem, Names),
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
            (Operation, "responses") => Map(Response, NamesAndExtensions),
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
}

/// What one of a schema's keywords leads to.
///
/// Every keyword whose value is a schema, or a list or a map of them, and the
/// three whose value is a small object of its own. A keyword left out states
/// something other than an object — a `type`, a bound, a `pattern` — or is no
/// keyword of a 3.0 schema, as `examples` and `dependencies` are not, and
/// nothing under such a keyword is entered.
fn keyword(key: &str) -> Option<Route> {
    Some(match key {
        PROPERTIES | "patternProperties" | "$defs" | "definitions" | DEPENDENT => {
            Route::Map(Kind::Schema, Keys::Names)
        }
        ITEMS
        | "prefixItems"
        | "additionalProperties"
        | "unevaluatedProperties"
        | "unevaluatedItems"
        | "propertyNames"
        | "contains"
        | "contentSchema" => Route::Node(Kind::Schema),
        key if COMPOSED.contains(&key) || CONDITIONAL.contains(&key) => Route::Node(Kind::Schema),
        "discriminator" => Route::Node(Kind::Discriminator),
        "xml" => Route::Node(Kind::Xml),
        "externalDocs" => Route::Node(Kind::ExternalDocs),
        _ => return None,
    })
}
