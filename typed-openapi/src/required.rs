//! Refusing a document that requires a key nothing declares.
//!
//! *Requires the `document` feature.*
//!
//! A *phantom* key is a name in a schema's `required` list that neither that
//! schema nor anything it composes with declares. The document is saying two
//! things at once — this key must be sent, and this key does not exist — and
//! one of the two statements is a mistake that the document does not say which.
//! Refused rather than reduced, because what a generator makes of it is a
//! required field of no stated type: nothing checks a value for it, an
//! adopter's own check over the body accepts anything, and the request goes out
//! without the key its caller was told to send.
//!
//! [`check`] runs over the *overlaid* document and before it is read as an
//! OpenAPI object model, for two reasons. An Overlay is where the repair is
//! written, so the check has to see the document an adopter's corrections
//! produced. And the object model normalises away what a raw walk can still
//! see: the walk reaches an inline request body and an inline response as
//! readily as `components.schemas`, which is where these turn up.
//!
//! # What the walk reads
//!
//! A node is classified by where its names *are*, never by its `type` keyword —
//! a vendor who omits `type` writes the same object as everyone else. The keys
//! a node admits are its own `properties` plus the `properties` of everything
//! it composes with: `$ref`, `allOf`, `anyOf` and `oneOf`, followed
//! transitively.
//!
//! Composition is a graph rather than a chain, which is why what terminates the
//! walk is a set of the references already followed and not the hop count
//! [`crate::schema::resolve`] carries. A counter suits a chain, where each hop
//! replaces the last. On a graph it either stops short of a legal document that
//! composes deeply — and stopping short means missing names, which means
//! refusing a correct document — or, counted per branch, never meets the cycle
//! that returns through a different branch. A set of the references already
//! followed terminates on the cycle itself and truncates nothing. Following one
//! reference once is also all a union needs: the names behind it are already in
//! the answer by the time a second branch asks for them.
//!
//! A `required` *inside* a composition is judged against the whole composition.
//! `allOf: [{ required: [total] }]` beside a `properties` that declares `total`
//! is one schema saying two halves of one thing, and the halves constrain the
//! same value, so what a member is held to is every node it sits inside as well
//! as itself. Judging a member on its own would refuse an ordinary document.
//!
//! # What the walk steps over
//!
//! `required` is the name of two things in OpenAPI. The one this module is
//! about is a schema's list of keys. The other is the boolean on a parameter
//! object and on a request body, which says whether a caller must supply the
//! thing and nothing at all about keys. A `required` whose value is not a list
//! of strings is that other one, so it is stepped over — a check that fired on
//! it would refuse every document with a required path parameter, which is
//! every document.
//!
//! `example`, `examples`, `default`, `enum` and `const` hold instance data
//! rather than schemas, so the walk does not descend into them. A vendor whose
//! example spells out a form as `required: [name, email]` is writing a value,
//! not requiring a key, and reading it as a schema would refuse a document that
//! is correct. The cost is that a schema reached only through a key spelled one
//! of those five words goes unread, which is a phantom missed rather than a
//! document wrongly refused.
//!
//! # What makes a node open-ended
//!
//! Anything this walk cannot read makes the node open-ended, and an open-ended
//! node requires nothing it cannot admit. `additionalProperties` that is not
//! `false` says any name satisfies the node, so a name declared nowhere is
//! still satisfiable and the document is not contradicting itself. The same
//! reading is given to `patternProperties`, to `$dynamicRef`, to `not` and to
//! `if`/`then`/`else` — none of which this walk follows — and to a `$ref` that
//! leads nowhere this document holds.
//!
//! That lopsidedness is the point. A refusal an adopter cannot override must
//! have no false positives on a valid document, so every shape the walk has no
//! reading for is read as permission rather than as a defect: a phantom missed
//! costs one untyped generated field, where a document wrongly refused costs
//! every operation in it.
//!
//! # What the walk says nothing about
//!
//! A `required` list sitting on an array node while the names are declared
//! under its `items` is inert. It binds nothing, and what is generated from it
//! is correct. It is also not repairable without a measurement: moving the list
//! onto `items` invents a requirement nobody has tested, and deleting it claims
//! nothing is required, so either repair is a claim about an API nobody has
//! called. There is nothing for an adopter to do about it, and a warning an
//! adopter cannot act on is a warning they learn to ignore — so nothing is
//! emitted for it at all.

use std::collections::BTreeSet;
use std::fmt;

use serde_json::Value;
use thiserror::Error;

/// A schema's list of the keys a value must carry.
const REQUIRED: &str = "required";
/// The keys a schema declares, and the one place a name is declared at all.
const PROPERTIES: &str = "properties";
/// The schema an array states its elements against.
const ITEMS: &str = "items";
/// What a node composes with, beside the `$ref` read alongside them. Their
/// members declare for the same value the node does, and are read as
/// declarations.
const COMPOSED: [&str; 3] = ["allOf", "anyOf", "oneOf"];
/// The keywords whose subschemas also constrain the value the node holding them
/// constrains, but which this walk does not read as declarations. A node
/// carrying one is open-ended, and so is everything under one.
const CONDITIONAL: [&str; 4] = ["not", "if", "then", "else"];
/// Everything else that puts a name beyond this walk's reach, and so makes the
/// node carrying it open-ended.
const OPEN: [&str; 2] = ["patternProperties", "$dynamicRef"];
/// Keys whose values are instance data rather than schemas. The walk does not
/// descend into them, so a `required` a vendor wrote inside an example stays a
/// value.
const DATA: [&str; 5] = ["example", "examples", "default", "enum", "const"];

/// One node whose `required` names keys nothing it declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phantom {
    /// Where the node is, spelled the way an Overlay names a target — so the
    /// location a refusal prints is also the line an adopter writes to repair
    /// it, rather than a coordinate they have to translate first.
    pub at: String,
    /// The names it requires and nothing declares, in the order the document
    /// lists them.
    pub names: Vec<String>,
}

/// A document that requires keys it declares nowhere.
///
/// Every such node is named at once. An adopter repairs these in an Overlay and
/// finds out whether they are done by running the bless step again, so a
/// refusal that named only the first would make that a queue: one run per
/// phantom, and no way to see how long the queue is. They are all the same
/// mistake and all corrected in the same file.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error(
    "the document requires keys that nothing declares; \
     declare each key in an Overlay, or drop its name from the `required` that \
     asks for it:\n{}",
    listed(.nodes)
)]
pub struct PhantomKeys {
    /// Every node that does it, in document order.
    pub nodes: Vec<Phantom>,
}

/// Read one overlaid document for `required` lists naming keys nothing
/// declares.
///
/// The whole document is walked — `paths`, inline request bodies, inline
/// responses and `components` alike — because a scan restricted to named
/// schemas reports the class closed while instances of it stand in inline
/// bodies.
pub fn check(document: &Value) -> Result<(), PhantomKeys> {
    let mut walk = Walk {
        doc: document,
        at: Vec::new(),
        enclosing: Vec::new(),
        found: Vec::new(),
    };
    walk.read(document);
    if walk.found.is_empty() {
        Ok(())
    } else {
        Err(PhantomKeys { nodes: walk.found })
    }
}

impl fmt::Display for Phantom {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{} requires ", self.at)?;
        for (index, name) in self.names.iter().enumerate() {
            if index > 0 {
                out.write_str(", ")?;
            }
            write!(out, "`{name}`")?;
        }
        Ok(())
    }
}

/// One line per node, so that a bless run finding several shows all of them.
fn listed(nodes: &[Phantom]) -> String {
    nodes
        .iter()
        .map(|node| format!("  {node}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One step of the way down to a node, kept as it was taken rather than as a
/// rendered string, so that a path is spelled once — where a finding lands.
#[derive(Debug, Clone, Copy)]
enum Step<'d> {
    Key(&'d str),
    Index(usize),
}

/// The descent.
///
/// `doc` is the whole document, because a `$ref` resolves against the root
/// wherever it is met. `enclosing` is the schemas that constrain the same value
/// as the node being read — the ones it sits inside through a composition — and
/// it is emptied on the way into anything that describes a different value.
struct Walk<'d> {
    doc: &'d Value,
    at: Vec<Step<'d>>,
    enclosing: Vec<&'d Value>,
    found: Vec<Phantom>,
}

impl<'d> Walk<'d> {
    /// This node, then everything under it.
    fn read(&mut self, node: &'d Value) {
        match node {
            Value::Object(fields) => {
                self.judge(node);
                for (key, value) in fields {
                    self.field(node, key, value);
                }
            }
            Value::Array(members) => {
                for (index, value) in members.iter().enumerate() {
                    self.at.push(Step::Index(index));
                    self.read(value);
                    self.at.pop();
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    /// One field of `node`, read with the right notion of which value it is
    /// about.
    ///
    /// A composition keyword keeps talking about the value `node` talks about,
    /// so `node` joins what its members are held to. Every other key — a
    /// property, an item, a response, a path — is a different value, and the
    /// schemas out here have nothing to say about it. A key holding instance
    /// data is not read at all.
    fn field(&mut self, node: &'d Value, key: &'d str, value: &'d Value) {
        if DATA.contains(&key) {
            return;
        }
        self.at.push(Step::Key(key));
        self.descend(node, key, value);
        self.at.pop();
    }

    /// The step itself, with whatever the key means for the schemas the value
    /// under it is held to.
    fn descend(&mut self, node: &'d Value, key: &'d str, value: &'d Value) {
        if COMPOSED.contains(&key) || CONDITIONAL.contains(&key) {
            self.enclosing.push(node);
            self.read(value);
            self.enclosing.pop();
        } else {
            let outer = std::mem::take(&mut self.enclosing);
            self.read(value);
            self.enclosing = outer;
        }
    }

    /// Whether this node's `required` — if it has the one this module is about
    /// — names anything the value it describes cannot carry.
    fn judge(&mut self, node: &'d Value) {
        let Some(names) = required_names(node) else {
            return;
        };
        let here = Admits::of_each(self.enclosing.iter().copied().chain([node]), self.doc);
        let missing: Vec<&str> = names
            .into_iter()
            .filter(|name| !here.covers(name))
            .collect();
        if missing.is_empty() {
            return;
        }
        // The list on the array rather than on the object it describes: inert,
        // unrepairable without a measurement, and therefore silent.
        let under = Admits::of_each(items_of(node), self.doc);
        if missing.iter().all(|name| under.covers(name)) {
            return;
        }
        self.found.push(Phantom {
            at: spelled(&self.at),
            names: missing.into_iter().map(str::to_owned).collect(),
        });
    }
}

/// The `required` this module is about: a list of the names a schema asks for.
///
/// `None` for the boolean on a parameter object or a request body, and for
/// anything else this walk has no reading for. Both are the document spelling a
/// different thing with the same word.
fn required_names(node: &Value) -> Option<Vec<&str>> {
    node.get(REQUIRED)?
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect()
}

/// The schema an array node states its elements against — one schema, or the
/// list of them a tuple spells.
fn items_of(node: &Value) -> Vec<&Value> {
    match node.get(ITEMS) {
        Some(Value::Array(members)) => members.iter().collect(),
        Some(items) => vec![items],
        None => Vec::new(),
    }
}

/// What a set of schemas admits: the names they declare, and whether they admit
/// names they do not declare.
#[derive(Debug, Default)]
struct Admits<'d> {
    named: BTreeSet<&'d str>,
    any: bool,
}

impl<'d> Admits<'d> {
    /// Everything these nodes and the schemas they compose with declare — the
    /// union, because a name any one of them declares is a name the value can
    /// carry.
    fn of_each(nodes: impl IntoIterator<Item = &'d Value>, doc: &'d Value) -> Self {
        let mut admits = Self::default();
        let mut followed = BTreeSet::new();
        for node in nodes {
            admits.gather(node, doc, &mut followed);
        }
        admits
    }

    /// One node's own names, then the names of everything it composes with.
    ///
    /// `followed` carries the references already taken. It is shared across
    /// every branch rather than copied down each one: what is being built is a
    /// union, so a reference a second branch asks for has already put its names
    /// into the answer.
    fn gather(&mut self, node: &'d Value, doc: &'d Value, followed: &mut BTreeSet<&'d str>) {
        let Some(node) = node.as_object() else {
            return;
        };
        if let Some(declared) = node.get(PROPERTIES).and_then(Value::as_object) {
            self.named.extend(declared.keys().map(String::as_str));
        }
        if node
            .get("additionalProperties")
            .is_some_and(|open| open != &Value::Bool(false))
            || OPEN
                .iter()
                .chain(&CONDITIONAL)
                .any(|key| node.contains_key(*key))
        {
            self.any = true;
        }
        if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
            self.follow(reference, doc, followed);
        }
        for key in COMPOSED {
            for member in node
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.gather(member, doc, followed);
            }
        }
    }

    /// One `$ref`, taken once.
    ///
    /// A reference this walk cannot follow hides names it cannot then see are
    /// missing, so it leaves the node open-ended rather than short of names.
    fn follow(&mut self, reference: &'d str, doc: &'d Value, followed: &mut BTreeSet<&'d str>) {
        let Some(target) = pointed_at(doc, reference) else {
            self.any = true;
            return;
        };
        if followed.insert(reference) {
            self.gather(target, doc, followed);
        }
    }

    /// Whether a value under this name is one these schemas admit.
    fn covers(&self, name: &str) -> bool {
        self.any || self.named.contains(name)
    }
}

/// The node a `$ref` names, if it is a pointer into this document and the
/// document holds one there.
///
/// Only `#/...` is read. A reference into another file is one this walk cannot
/// follow, and the caller reads that as open-ended rather than as a defect.
fn pointed_at<'d>(doc: &'d Value, reference: &str) -> Option<&'d Value> {
    let mut node = doc;
    for step in reference.strip_prefix("#/")?.split('/') {
        // RFC 6901, and in this order: `~01` is an escaped `~1` and must not
        // come back out as a `/`.
        let step = step.replace("~1", "/").replace("~0", "~");
        node = node.get(step.as_str())?;
    }
    Some(node)
}

/// Where the walk is, spelled the way an Overlay names a target.
///
/// A bare word is written after a dot and everything else is quoted in
/// brackets, which is how a key with a `/` in it — every path, and every media
/// type — comes back as something an adopter can paste.
fn spelled(at: &[Step<'_>]) -> String {
    let mut out = String::from("$");
    for step in at {
        match *step {
            Step::Index(index) => {
                out.push('[');
                out.push_str(&index.to_string());
                out.push(']');
            }
            Step::Key(key) if is_bare(key) => {
                out.push('.');
                out.push_str(key);
            }
            Step::Key(key) => {
                out.push_str("['");
                out.push_str(&key.replace('\\', r"\\").replace('\'', r"\'"));
                out.push_str("']");
            }
        }
    }
    out
}

/// A name that needs no quoting after a dot.
fn is_bare(key: &str) -> bool {
    key.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    fn read(document: &str) -> Result<(), PhantomKeys> {
        check(&serde_yaml_ng::from_str(document).expect("the fixture parses"))
    }

    /// The location is spelled as an Overlay spells a target, so that what a
    /// refusal prints is what a repair is written against.
    #[test]
    fn a_location_is_spelled_the_way_an_overlay_names_a_target() {
        assert_eq!(
            spelled(&[
                Step::Key("paths"),
                Step::Key("/ledger/{period}"),
                Step::Key("post"),
                Step::Key("requestBody"),
                Step::Key("content"),
                Step::Key("application/json"),
                Step::Key("schema"),
            ]),
            "$.paths['/ledger/{period}'].post.requestBody.content['application/json'].schema"
        );
        assert_eq!(spelled(&[Step::Key("allOf"), Step::Index(2)]), "$.allOf[2]");
        // A vendor extension is not a bare word in a JSONPath, so it is quoted
        // rather than written after a dot.
        assert_eq!(spelled(&[Step::Key("x-cli-writes")]), "$['x-cli-writes']");
        assert_eq!(spelled(&[]), "$");
    }

    /// A pointer is unescaped before it is followed, and only ever into this
    /// document.
    #[test]
    fn a_pointer_is_unescaped_in_the_order_the_escape_rule_states() {
        let doc: Value = serde_json::json!({ "components": { "schemas": { "a/b": 1, "~1": 2 } } });
        assert_eq!(
            pointed_at(&doc, "#/components/schemas/a~1b"),
            Some(&Value::from(1))
        );
        assert_eq!(
            pointed_at(&doc, "#/components/schemas/~01"),
            Some(&Value::from(2))
        );
        assert_eq!(pointed_at(&doc, "#/components/schemas/nope"), None);
        assert_eq!(pointed_at(&doc, "ledger.yaml#/components/schemas/a"), None);
    }

    /// The narrowest statement of the whole module, with no OpenAPI around it:
    /// a name declared nowhere reachable is named, and a name declared
    /// somewhere reachable is not.
    #[test]
    fn a_name_is_missing_only_when_nothing_reachable_declares_it() {
        let refused = read(
            "components:\n  schemas:\n    Entry:\n      required: [amount, account]\n\
             \x20     properties:\n        amount: { type: string }\n",
        )
        .expect_err("`account` is declared nowhere");
        assert_eq!(
            refused.nodes,
            [Phantom {
                at: "$.components.schemas.Entry".to_owned(),
                names: vec!["account".to_owned()],
            }]
        );

        read(
            "components:\n  schemas:\n    Entry:\n      required: [amount, account]\n\
             \x20     properties:\n        amount: { type: string }\n        account: { type: string }\n",
        )
        .expect("both are declared");
    }

    /// A composition is one schema written in halves, and the halves describe
    /// one value: a member stating only a `required` takes its names from the
    /// node it sits in. Judging the member alone would refuse this document,
    /// which is an ordinary one.
    #[test]
    fn a_required_inside_a_composition_is_held_to_the_whole_composition() {
        read(
            "components:\n  schemas:\n    Entry:\n\
             \x20     properties:\n        amount: { type: string }\n\
             \x20     allOf:\n        - required: [amount]\n",
        )
        .expect("the composition declares what its member requires");

        let refused = read(
            "components:\n  schemas:\n    Entry:\n\
             \x20     properties:\n        amount: { type: string }\n\
             \x20     allOf:\n        - required: [account]\n",
        )
        .expect_err("nothing in the composition declares `account`");
        assert_eq!(refused.nodes.len(), 1, "{refused}");
        assert_eq!(refused.nodes[0].at, "$.components.schemas.Entry.allOf[0]");
    }
}
