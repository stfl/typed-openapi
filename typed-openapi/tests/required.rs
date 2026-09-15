//! A document that requires a key nothing declares is refused while it is
//! reduced, and a document that only looks like one is not.
//!
//! The refusal is the point of this file, but the tests that matter most are
//! the ones holding it *back*: a check with no override has to be wrong about
//! nothing. The two `required` keywords OpenAPI spells with one word are the
//! nearest miss, and there is one test for each of them.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::sync::mpsc;
use std::time::Duration;

use typed_openapi::model::Body;
use typed_openapi::required::{self, Phantom, PhantomKeys};
use typed_openapi::{Document, overlay};

/// A document of this test's own: one server, and whatever the case puts under
/// it.
fn document(rest: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         {rest}"
    )
}

/// What the check alone makes of one document — the reading with nothing else
/// in front of it, so that "no diagnostic at all" is a thing a test can say.
fn scanned(rest: &str) -> Result<(), PhantomKeys> {
    required::check(&overlay::parse(&document(rest)).expect("the fixture parses"))
}

/// Every node the check names, each as the line a refusal prints for it.
fn named(rest: &str) -> Vec<String> {
    match scanned(rest) {
        Ok(()) => Vec::new(),
        Err(refused) => refused.nodes.iter().map(Phantom::to_string).collect(),
    }
}

/// The same document through the door an adopter uses.
fn loaded(rest: &str) -> Result<Document, typed_openapi::LoadError> {
    Document::load(&document(rest), &[])
}

/// One named schema requiring a key it declares nowhere, and the body reaching
/// it through a `$ref`.
const LEDGER: &str = r"paths:
  /ledger:
    post:
      operationId: postEntry
      requestBody:
        required: true
        content:
          application/json:
            schema: { $ref: '#/components/schemas/Entry' }
      responses: { '201': { description: Created } }
components:
  schemas:
    Entry:
      type: object
      required: [account, amount, period]
      properties:
        account: { type: string }
        amount: { type: string }
";

/// The refusal names where the contradiction is and which names are
/// unreachable, and the location it prints is the target an Overlay repairs it
/// at.
#[test]
fn a_required_key_nothing_declares_is_refused_and_named_where_it_stands() {
    // The way out the message names is the way out, and the check reads the
    // document the Overlay produced rather than the one the vendor shipped.
    const REPAIR: &str = "overlay: 1.1.0\n\
         info: { title: t, version: \"1\" }\n\
         actions:\n\
         \x20 - target: $.components.schemas.Entry.properties\n\
         \x20   description: The vendor requires `period` and declares it nowhere.\n\
         \x20   update:\n\
         \x20     period: { type: string, description: The period it is booked into. }\n";

    let refused = loaded(LEDGER).expect_err("`period` is declared nowhere");
    assert_eq!(
        refused.to_string(),
        "the document requires keys that nothing declares; declare each key in an \
         Overlay, or drop its name from the `required` that asks for it:\n\
         \x20 $.components.schemas.Entry requires `period`"
    );

    let repaired = Document::load(&document(LEDGER), &[REPAIR]).expect("the Overlay repairs it");
    let Body::JsonFields(fields) = repaired.get("postEntry").expect("postEntry").body() else {
        panic!("every property of `Entry` is a scalar");
    };
    let period = fields
        .iter()
        .find(|field| field.name() == "period")
        .expect("the Overlay declared it");
    assert!(period.required(), "the document still requires it");
}

/// The same contradiction in a body written out where it is used. A check
/// scoped to `components.schemas` passes every other test in this file and
/// fails this one, which is why it is here: these stand in inline bodies as
/// often as in named schemas.
#[test]
fn a_required_key_nothing_declares_in_an_inline_body_is_refused_too() {
    const INLINE: &str = r"paths:
  /vouchers:
    post:
      operationId: createVoucher
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [total, positions]
              properties:
                total: { type: string }
      responses: { '201': { description: Created } }
";

    assert_eq!(
        named(INLINE),
        [
            "$.paths['/vouchers'].post.requestBody.content['application/json'].schema \
          requires `positions`"
        ]
    );
    assert!(loaded(INLINE).is_err(), "and the document is refused");
}

/// A name is declared wherever the composition reaches, however many hops away
/// that is and whichever keyword the hop is spelled with.
#[test]
fn a_name_reached_through_composition_is_a_name_that_is_declared() {
    const COMPOSED: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Booked:
      type: object
      properties:
        period: { type: string }
    Identified:
      allOf:
        - $ref: '#/components/schemas/Booked'
      properties:
        id: { type: integer }
    Entry:
      required: [id, period, amount]
      allOf:
        - $ref: '#/components/schemas/Identified'
        - type: object
          properties:
            amount: { type: string }
    Position:
      required: [quantity, account]
      anyOf:
        - type: object
          properties:
            quantity: { type: integer }
      oneOf:
        - $ref: '#/components/schemas/Booked'
        - type: object
          properties:
            account: { type: string }
";

    // `period` is two `$ref` hops and two `allOf`s away from the node that
    // requires it.
    assert!(named(COMPOSED).is_empty(), "{:?}", named(COMPOSED));
    assert!(loaded(COMPOSED).is_ok());
}

/// The list on the array rather than on the object it describes. It binds
/// nothing and generates correctly, and neither repair an adopter could make is
/// a thing anyone has measured — so the check says nothing at all about it,
/// rather than saying something they would learn to ignore.
#[test]
fn a_required_list_on_the_array_rather_than_its_items_says_nothing() {
    const ON_THE_ARRAY: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Positions:
      type: array
      required: [account, amount]
      items:
        type: object
        properties:
          account: { type: string }
          amount: { type: string }
    Untyped:
      required: [account]
      items:
        properties:
          account: { type: string }
";

    assert_eq!(scanned(ON_THE_ARRAY), Ok(()), "no diagnostic at all");
    assert!(loaded(ON_THE_ARRAY).is_ok());

    // What makes this class silent is that `items` accounts for the whole
    // list. A list only half of which is under `items` is not a list written
    // one level too high — it is a list naming a key nothing declares, beside
    // a name that happens also to be a property of the element — so the node
    // is refused, and every name it cannot itself admit is named.
    assert_eq!(
        named(&ON_THE_ARRAY.replace("required: [account, amount]", "required: [account, period]")),
        ["$.components.schemas.Positions requires `account`, `period`"]
    );
}

/// OpenAPI spells a second thing `required`: the boolean on a parameter object.
/// A check that did not step over it would fire on every required path
/// parameter in every document, which is the fastest way to make a refusal
/// nobody can override useless.
#[test]
fn a_required_parameter_is_not_a_required_key() {
    const PARAMETERS: &str = r"paths:
  /ledger/{period}/entries/{id}:
    parameters:
      - { name: period, in: path, required: true, schema: { type: string } }
    get:
      operationId: getEntry
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
        - { name: account, in: query, required: true, schema: { type: string } }
        - { name: settled, in: query, required: false, schema: { type: boolean } }
      responses: { '200': { description: OK } }
";

    assert_eq!(scanned(PARAMETERS), Ok(()));
    let doc = loaded(PARAMETERS).expect("four parameters, three of them required");
    assert_eq!(doc.get("getEntry").expect("getEntry").params().len(), 4);
}

/// The same boolean under a different parent. `requestBody.required` says a
/// caller must send a body and says nothing about which keys are in it.
#[test]
fn a_required_request_body_is_not_a_required_key() {
    const BODY: &str = r"paths:
  /ledger:
    post:
      operationId: postEntry
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              properties:
                amount: { type: string }
      responses: { '201': { description: Created } }
";

    assert_eq!(scanned(BODY), Ok(()));
    assert!(matches!(
        loaded(BODY)
            .expect("a body a caller must send")
            .get("postEntry")
            .expect("postEntry")
            .body(),
        Body::JsonFields(_)
    ));
}

/// Composition is a graph, so a document can point back at where it came from.
/// The walk meets the cycle, finishes, and finds the name that is only
/// reachable by going round it.
///
/// The check runs on a thread this test waits on with a deadline, because a
/// walk that did not terminate would otherwise hang the suite instead of
/// failing it.
#[test]
fn a_reference_cycle_terminates_rather_than_running_forever() {
    const CYCLIC: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Entry:
      required: [account, opposite]
      properties:
        account: { type: string }
      allOf:
        - $ref: '#/components/schemas/Opposite'
    Opposite:
      properties:
        opposite: { $ref: '#/components/schemas/Entry' }
      allOf:
        - $ref: '#/components/schemas/Entry'
";

    let (done, waiting) = mpsc::channel();
    std::thread::Builder::new()
        .name("cyclic-document".to_owned())
        .spawn(move || drop(done.send(named(CYCLIC))))
        .expect("a thread to run the walk on");
    let found = waiting
        .recv_timeout(Duration::from_secs(20))
        .expect("the walk terminates on a cycle rather than running forever");

    // `opposite` is declared on the schema that points back at the one
    // requiring it, so finding nothing is also the proof that the walk went
    // round rather than stopping short.
    assert!(found.is_empty(), "{found:?}");
}

/// A node that admits any name cannot be contradicting itself: a name declared
/// nowhere is still a name a value may carry. `additionalProperties: false` is
/// the node saying the opposite, and is the only one of the three that is
/// refused.
#[test]
fn an_open_ended_node_admits_every_name_and_a_closed_one_admits_none() {
    const OPENNESS: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Any:
      type: object
      required: [account]
      additionalProperties: true
    Strings:
      type: object
      required: [account]
      additionalProperties: { type: string }
    Closed:
      type: object
      required: [account]
      additionalProperties: false
      properties:
        amount: { type: string }
";

    assert_eq!(
        named(OPENNESS),
        ["$.components.schemas.Closed requires `account`"]
    );
}

/// `not` and `if`/`then`/`else` are subschemas this walk does not follow, so a
/// node carrying one is read as open-ended rather than refused over names those
/// keywords might supply. The limit is stated in the module's own words, and
/// this is what holds it.
#[test]
fn a_keyword_this_walk_does_not_follow_leaves_the_node_open_ended() {
    const CONDITIONAL: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Split:
      type: object
      required: [debit, credit]
      if:
        properties:
          side: { type: string }
      then:
        required: [debit]
      else:
        required: [credit]
    Neither:
      type: object
      required: [account]
      not:
        properties:
          account: { type: integer }
";

    assert_eq!(scanned(CONDITIONAL), Ok(()), "no diagnostic at all");
    assert!(loaded(CONDITIONAL).is_ok());
}

/// A `required` inside an example, a default, an enum or a const is a value
/// that happens to have that key — not a schema requiring one. So is a property
/// a vendor spelled `required`. Reading either as a schema would refuse a
/// document that is correct.
#[test]
fn a_required_a_vendor_wrote_inside_instance_data_is_a_value_not_a_key() {
    const INSTANCES: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses:
        '200':
          description: OK
          content:
            application/json:
              schema:
                type: object
                properties:
                  total: { type: string }
              examples:
                empty:
                  value: { required: [account] }
components:
  schemas:
    Form:
      type: object
      properties:
        fields: { type: array, items: { type: string } }
        required: { type: array, items: { type: string } }
      example:
        required: [account, amount]
      default:
        required: [period]
      enum:
        - { required: [side] }
      const:
        required: [amount]
";

    assert_eq!(scanned(INSTANCES), Ok(()), "no diagnostic at all");
    assert!(loaded(INSTANCES).is_ok());
}

/// An adopter corrects these in an Overlay and learns whether they are done by
/// running the bless step again. Naming only the first would make that a queue
/// with no visible length, so every node is named at once and in document
/// order.
#[test]
fn every_node_that_requires_a_key_nothing_declares_is_named_at_once() {
    const SEVERAL: &str = r"paths:
  /ledger:
    post:
      operationId: postEntry
      requestBody:
        content:
          application/json:
            schema:
              type: object
              required: [period]
              properties:
                amount: { type: string }
      responses: { '201': { description: Created } }
components:
  schemas:
    Entry:
      type: object
      required: [account, side]
      properties:
        amount: { type: string }
";

    assert_eq!(
        named(SEVERAL),
        [
            "$.paths['/ledger'].post.requestBody.content['application/json'].schema \
             requires `period`",
            "$.components.schemas.Entry requires `account`, `side`",
        ]
    );
}

/// The committed fixture is the regression net for a false positive on a real,
/// correct document — and, with one key taken out from under a `required` it
/// names, the proof that the net is not simply blind.
#[test]
fn the_committed_fixture_reduces_and_the_same_fixture_with_a_phantom_does_not() {
    const TOY: &str = include_str!("fixtures/toy.yaml");
    const OVERLAYS: &[&str] = &[
        include_str!("fixtures/corrections.yaml"),
        include_str!("fixtures/cli.yaml"),
    ];

    Document::load(TOY, OVERLAYS).expect("the vendor's document plus the adopter's Overlays");

    let stripped = TOY.replace("        city:\n          type: string\n", "");
    assert_ne!(stripped, TOY, "the fixture still declares `city`");
    let refused = Document::load(&stripped, OVERLAYS)
        .expect_err("`Address` still requires the key that was taken out");
    assert!(
        refused
            .to_string()
            .contains("$.components.schemas.Address requires `city`"),
        "{refused}"
    );
}
