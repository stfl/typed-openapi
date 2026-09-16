//! A document that requires a key nothing declares is refused while it is
//! reduced, and a document that only looks like one is not.
//!
//! The refusal is the point of this file, but the tests that matter most are
//! the ones holding it *back*: the only way past it is an Overlay, so it has to
//! be wrong about nothing a vendor wrote correctly. Two near misses have a test
//! each. The first is that `required` is the name of two things in OpenAPI, and
//! a document that spells the other one is every document. The second is that a
//! `required` list is only a schema's where a schema stands — the same three
//! words inside a specification extension, or among the arguments a link passes
//! on, are the vendor's data. Against those stands one test that every place a
//! schema *does* stand is still read, so that the walk cannot be made right by
//! being switched off.

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

/// The refusal names the node whose `required` asks for a key nothing
/// describes and which names those are, and the location it prints is the
/// target an Overlay repairs it at.
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

/// The same mistake in a body written out where it is used. A check scoped to
/// `components.schemas` passes every other test in this file and fails this
/// one, which is why it is here: these stand in inline bodies as often as in
/// named schemas.
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

/// Permitting a name is not describing it. A node that admits any name still
/// declares none of them, and the type generated from it carries the same
/// required field of no stated type as the closed node beside it — so every one
/// of these is refused, and the Overlay line that repairs one is the line that
/// gives its field a type.
///
/// `$dynamicRef` is here because it is the shape that most looks like an
/// exemption and is not: it puts a declaration behind an anchor this walk does
/// not index, and typify writes the field anyway.
#[test]
fn a_node_that_permits_extra_names_still_describes_none_of_them() {
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
    Patterned:
      type: object
      required: [account]
      patternProperties:
        '^acc': { type: string }
    Unevaluated:
      type: object
      required: [account]
      unevaluatedProperties: { type: string }
    Dynamic:
      type: object
      required: [account]
      $dynamicRef: '#meta'
    Closed:
      type: object
      required: [account]
      additionalProperties: false
      properties:
        amount: { type: string }
";

    assert_eq!(
        named(OPENNESS),
        [
            "$.components.schemas.Any requires `account`",
            "$.components.schemas.Strings requires `account`",
            "$.components.schemas.Patterned requires `account`",
            "$.components.schemas.Unevaluated requires `account`",
            "$.components.schemas.Dynamic requires `account`",
            "$.components.schemas.Closed requires `account`",
        ]
    );
}

/// A `$ref` that leads out of this document hides whatever it declares, so the
/// node carrying it is exempt: the key may well be declared in the file the
/// reference names, and this walk has no way to find out. A reference the
/// document does hold is followed, and a name neither end declares is still
/// named.
#[test]
fn a_reference_this_walk_cannot_follow_leaves_the_node_exempt() {
    const FOREIGN: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Elsewhere:
      required: [account]
      allOf:
        - $ref: 'ledger.yaml#/components/schemas/Base'
    Nowhere:
      required: [account]
      allOf:
        - $ref: '#/components/schemas/Absent'
    Here:
      required: [account]
      allOf:
        - $ref: '#/components/schemas/Empty'
    Empty:
      type: object
      properties:
        amount: { type: string }
";

    assert_eq!(
        named(FOREIGN),
        ["$.components.schemas.Here requires `account`"]
    );
}

/// `not` and `if`/`then`/`else` are subschemas this walk reads no name out of,
/// so a node carrying one is exempt rather than refused over names those
/// keywords might supply. The limit is stated in the module's own words, and
/// this is what holds it.
#[test]
fn a_subschema_this_walk_reads_no_name_out_of_leaves_the_node_exempt() {
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

/// A `$ref` this walk cannot follow hides the names behind it, and a name it
/// cannot see is not a name it can call missing. So the node carrying one is
/// read as open-ended, the same permission every other unreadable shape gets.
///
/// Two references arrive that way. One into another file is the adopter's
/// bundler's to resolve and not this walk's, and a `#/` pointer this document
/// holds nothing at is a document broken somewhere else entirely — neither is
/// evidence that a required key is declared nowhere. Refusing over either is
/// the false positive the lopsidedness exists to prevent: a refusal an adopter
/// cannot override has to be wrong about nothing, and a phantom missed costs
/// one untyped field where a document wrongly refused costs every operation in
/// it.
///
/// `Reachable` is the control. Its reference leads somewhere, so the names
/// behind it are names the walk has, and the one it requires and nothing
/// declares is still named.
#[test]
fn a_reference_this_walk_cannot_follow_leaves_the_node_open_ended() {
    const UNFOLLOWABLE: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Foreign:
      type: object
      required: [account]
      allOf:
        - $ref: 'other.yaml#/components/schemas/Entry'
    Dangling:
      type: object
      required: [amount]
      allOf:
        - $ref: '#/components/schemas/Gone'
    Reachable:
      type: object
      required: [period]
      allOf:
        - $ref: '#/components/schemas/Declared'
    Declared:
      type: object
      properties:
        account: { type: string }
";

    assert_eq!(
        named(UNFOLLOWABLE),
        ["$.components.schemas.Reachable requires `period`"]
    );
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

/// A specification extension carries the vendor's own arbitrary JSON, and JSON
/// that holds a `required` list beside no `properties` is a form the vendor
/// described rather than a schema requiring a key it never describes. Refusing
/// over one leaves an adopter deleting the vendor's extension in an Overlay to
/// repair a defect that was never there, which is why this test exists at all.
#[test]
fn a_specification_extension_is_the_vendors_own_json_and_never_a_schema() {
    /// A callback object carries one too. It is checked rather than loaded
    /// because `openapiv3` reads a callback as a bare map of path items and so
    /// refuses this document a step later — by the shape it could not read,
    /// which is a refusal that says what it is about. This check is not
    /// allowed to get there first.
    const IN_A_CALLBACK: &str = r"paths:
  /ledger:
    post:
      operationId: postEntry
      responses: { '201': { description: Created } }
      callbacks:
        settled:
          x-template:
            post:
              requestBody:
                content:
                  application/json:
                    schema: { required: [template] }
";

    const EXTENSIONS: &str = r"x-forms:
  signup: { required: [name, email] }
tags:
  - name: ledger
    description: The ledger.
    x-fields: { required: [account] }
paths:
  x-mock-defaults:
    get:
      responses:
        '200':
          content:
            application/json:
              schema: { required: [stub] }
  /ledger:
    get:
      operationId: listEntries
      x-form:
        required: [name, email]
        fields: [name, email]
      responses:
        '200': { description: OK }
        x-fallback:
          content:
            application/json:
              schema: { required: [fallback] }
components:
  x-templates:
    entry: { required: [account] }
  schemas:
    Entry:
      type: object
      properties:
        amount: { type: string }
      x-ui: { required: [amount, side] }
";

    assert_eq!(scanned(EXTENSIONS), Ok(()), "no diagnostic at all");
    assert!(loaded(EXTENSIONS).is_ok());
    assert_eq!(scanned(IN_A_CALLBACK), Ok(()), "no diagnostic at all");
}

/// A link object says which of an operation's parameters the next call is given
/// and what body to send it. Its `parameters` are values passed on, so a
/// `required` among them names a parameter rather than a key — the same
/// mistake as reading an extension, arrived at through a key the specification
/// does name.
#[test]
fn a_link_passes_arguments_on_and_states_no_schema() {
    const LINKS: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses:
        '200':
          description: OK
          links:
            entry:
              operationId: getEntry
              parameters:
                required: [account, amount]
              requestBody: { required: [account] }
";

    assert_eq!(scanned(LINKS), Ok(()), "no diagnostic at all");
}

/// A `dependentSchemas` subschema states what the object carrying the property
/// must satisfy, not what the property's own value must — so *if `foo` is
/// present then `bar` is required* is the object requiring its own `bar`, and
/// the object's `properties` are where `bar` is declared. Judging the subschema
/// on its own refuses the one idiom the keyword exists for, which is the same
/// mistake as judging an `allOf` member on its own.
///
/// The other two schemas are what keeps the reading from being the blanket
/// "anything under a schema inherits it". A name declared on the object is no
/// declaration for a property's value, which is a different value; and a
/// subschema constraining the same value still has to be read, so a name
/// neither it nor the object declares is still named.
#[test]
fn a_dependent_subschema_is_held_to_the_object_that_carries_the_property() {
    const DEPENDENT: &str = r"paths:
  /ledger:
    get:
      operationId: listEntries
      responses: { '200': { description: OK } }
components:
  schemas:
    Entry:
      type: object
      properties:
        foo: { type: string }
        bar: { type: string }
      dependentSchemas:
        foo:
          required: [bar]
    Unknown:
      type: object
      properties:
        foo: { type: string }
      dependentSchemas:
        foo:
          required: [side]
    Nested:
      type: object
      properties:
        bar: { type: string }
        inner:
          type: object
          required: [bar]
";

    assert_eq!(
        named(DEPENDENT),
        [
            "$.components.schemas.Unknown.dependentSchemas.foo requires `side`",
            "$.components.schemas.Nested.properties.inner requires `bar`",
        ]
    );
}

/// One phantom in every position the specification puts a schema in: one per
/// key the walk's table spells, down to each of the eight methods and each of
/// the two spellings of a definitions map.
///
/// Four of the keywords carry their phantom one level further down, under a
/// `properties` of their own. `not`, `if`, `then` and `else` exempt everything
/// beneath them, so a phantom sitting directly under one is silent
/// whether or not the walk goes there — and a route that cannot be observed is
/// a route this test cannot hold. A `properties` inside describes a different
/// value, which puts the phantom back where it can be seen.
const EVERYWHERE: &str = r"paths:
  /ledger:
    parameters:
      - name: tenant
        in: query
        schema: { type: object, required: [shared] }
    post:
      operationId: postEntry
      parameters:
        - name: filter
          in: query
          content:
            application/json:
              schema: { type: object, required: [onParameterContent] }
      requestBody:
        content:
          multipart/form-data:
            schema:
              type: object
              properties:
                file: { type: string }
            encoding:
              file:
                headers:
                  X-Checksum:
                    schema: { type: object, required: [onEncodingHeader] }
      responses:
        '200':
          description: OK
          headers:
            X-Page:
              schema: { type: object, required: [onResponseHeader] }
            x-pagination:
              schema: { type: object, required: [onHeaderNamedLikeAnExtension] }
          content:
            application/json:
              schema:
                type: object
                properties:
                  rows:
                    type: array
                    items: { type: object, required: [onItems] }
                  spare:
                    type: object
                    additionalProperties: { type: object, required: [onAdditional] }
      callbacks:
        settled:
          '{$request.body#/url}':
            post:
              operationId: onSettled
              requestBody:
                content:
                  application/json:
                    schema: { type: object, required: [onCallbackBody] }
              responses: { '204': { description: No Content } }
  /methods:
    get:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onGet] } }]
    put:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onPut] } }]
    post:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onPost] } }]
    delete:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onDelete] } }]
    options:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onOptions] } }]
    head:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onHead] } }]
    patch:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onPatch] } }]
    trace:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onTrace] } }]
webhooks:
  settled:
    post:
      operationId: onWebhook
      requestBody:
        content:
          application/json:
            schema: { type: object, required: [onWebhook] }
      responses: { '204': { description: No Content } }
components:
  parameters:
    Period:
      name: period
      in: query
      schema: { type: object, required: [onComponentParameter] }
    x-tenant:
      name: tenant
      in: query
      schema: { type: object, required: [onComponentNamedLikeAnExtension] }
  headers:
    X-Total:
      schema: { type: object, required: [onComponentHeader] }
    X-Filter:
      content:
        application/json:
          schema: { type: object, required: [onHeaderContent] }
  requestBodies:
    Entry:
      content:
        application/json:
          schema: { type: object, required: [onComponentRequestBody] }
  responses:
    Problem:
      description: Problem
      content:
        application/json:
          schema: { type: object, required: [onComponentResponse] }
  pathItems:
    Settled:
      parameters: [{ name: n, in: query, schema: { type: object, required: [onPathItem] } }]
  callbacks:
    Settled:
      '{$request.body#/url}':
        parameters: [{ name: n, in: query, schema: { type: object, required: [onCallback] } }]
  schemas:
    Named:
      type: object
      required: [onNamedSchema]
      properties:
        nested: { type: object, required: [onNestedProperty] }
        x-audit: { type: object, required: [onPropertyNamedLikeAnExtension] }
    Composed:
      allOf: [{ type: object, required: [onAllOf] }]
      anyOf: [{ type: object, required: [onAnyOf] }]
      oneOf: [{ type: object, required: [onOneOf] }]
    Conditional:
      not:
        properties: { denied: { type: object, required: [onNot] } }
      if:
        properties: { tested: { type: object, required: [onIf] } }
      then:
        properties: { held: { type: object, required: [onThen] } }
      else:
        properties: { otherwise: { type: object, required: [onElse] } }
    Dependent:
      dependentSchemas:
        flag: { type: object, required: [onDependentSchemas] }
    Patterned:
      patternProperties:
        '^on': { type: object, required: [onPatternProperties] }
    Defined:
      $defs:
        Inner: { type: object, required: [onDefs] }
      definitions:
        Legacy: { type: object, required: [onDefinitions] }
    Listed:
      type: array
      prefixItems: [{ type: object, required: [onPrefixItems] }]
      contains: { type: object, required: [onContains] }
      unevaluatedItems: { type: object, required: [onUnevaluatedItems] }
    Keyed:
      type: object
      propertyNames: { type: object, required: [onPropertyNames] }
      unevaluatedProperties: { type: object, required: [onUnevaluatedProperties] }
    Encoded:
      type: string
      contentSchema: { type: object, required: [onContentSchema] }
    x-Legacy:
      type: object
      required: [onSchemaNamedLikeAnExtension]
";

/// What [`EVERYWHERE`] is refused for, a line per route.
///
/// The four entries named after an extension are where the extension rule
/// stops. Only the Paths Object, the Responses Object and a Callback Object are
/// maps the specification lets a vendor hang JSON of its own inside; every
/// other map here is keyed by names somebody chose. `x-audit` is an ordinary
/// thing for a JSON body to carry a field called, `x-pagination` is an ordinary
/// response header, and a component or a schema may be named either way — so
/// `x-` there is a name and not a vendor's aside.
const EVERY_PHANTOM: &[&str] = &[
    "$.paths['/ledger'].parameters[0].schema requires `shared`",
    "$.paths['/ledger'].post.parameters[0].content['application/json'].schema \
     requires `onParameterContent`",
    "$.paths['/ledger'].post.requestBody.content['multipart/form-data'].encoding.file\
     .headers['X-Checksum'].schema requires `onEncodingHeader`",
    "$.paths['/ledger'].post.responses['200'].headers['X-Page'].schema \
     requires `onResponseHeader`",
    "$.paths['/ledger'].post.responses['200'].headers['x-pagination'].schema \
     requires `onHeaderNamedLikeAnExtension`",
    "$.paths['/ledger'].post.responses['200'].content['application/json'].schema\
     .properties.rows.items requires `onItems`",
    "$.paths['/ledger'].post.responses['200'].content['application/json'].schema\
     .properties.spare.additionalProperties requires `onAdditional`",
    "$.paths['/ledger'].post.callbacks.settled['{$request.body#/url}'].post.requestBody\
     .content['application/json'].schema requires `onCallbackBody`",
    "$.paths['/methods'].get.parameters[0].schema requires `onGet`",
    "$.paths['/methods'].put.parameters[0].schema requires `onPut`",
    "$.paths['/methods'].post.parameters[0].schema requires `onPost`",
    "$.paths['/methods'].delete.parameters[0].schema requires `onDelete`",
    "$.paths['/methods'].options.parameters[0].schema requires `onOptions`",
    "$.paths['/methods'].head.parameters[0].schema requires `onHead`",
    "$.paths['/methods'].patch.parameters[0].schema requires `onPatch`",
    "$.paths['/methods'].trace.parameters[0].schema requires `onTrace`",
    "$.webhooks.settled.post.requestBody.content['application/json'].schema \
     requires `onWebhook`",
    "$.components.parameters.Period.schema requires `onComponentParameter`",
    "$.components.parameters['x-tenant'].schema \
     requires `onComponentNamedLikeAnExtension`",
    "$.components.headers['X-Total'].schema requires `onComponentHeader`",
    "$.components.headers['X-Filter'].content['application/json'].schema \
     requires `onHeaderContent`",
    "$.components.requestBodies.Entry.content['application/json'].schema \
     requires `onComponentRequestBody`",
    "$.components.responses.Problem.content['application/json'].schema \
     requires `onComponentResponse`",
    "$.components.pathItems.Settled.parameters[0].schema requires `onPathItem`",
    "$.components.callbacks.Settled['{$request.body#/url}'].parameters[0].schema \
     requires `onCallback`",
    "$.components.schemas.Named requires `onNamedSchema`",
    "$.components.schemas.Named.properties.nested requires `onNestedProperty`",
    "$.components.schemas.Named.properties['x-audit'] \
     requires `onPropertyNamedLikeAnExtension`",
    "$.components.schemas.Composed.allOf[0] requires `onAllOf`",
    "$.components.schemas.Composed.anyOf[0] requires `onAnyOf`",
    "$.components.schemas.Composed.oneOf[0] requires `onOneOf`",
    "$.components.schemas.Conditional.not.properties.denied requires `onNot`",
    "$.components.schemas.Conditional.if.properties.tested requires `onIf`",
    "$.components.schemas.Conditional.then.properties.held requires `onThen`",
    "$.components.schemas.Conditional.else.properties.otherwise requires `onElse`",
    "$.components.schemas.Dependent.dependentSchemas.flag \
     requires `onDependentSchemas`",
    "$.components.schemas.Patterned.patternProperties['^on'] \
     requires `onPatternProperties`",
    "$.components.schemas.Defined['$defs'].Inner requires `onDefs`",
    "$.components.schemas.Defined.definitions.Legacy requires `onDefinitions`",
    "$.components.schemas.Listed.prefixItems[0] requires `onPrefixItems`",
    "$.components.schemas.Listed.contains requires `onContains`",
    "$.components.schemas.Listed.unevaluatedItems requires `onUnevaluatedItems`",
    "$.components.schemas.Keyed.propertyNames requires `onPropertyNames`",
    "$.components.schemas.Keyed.unevaluatedProperties \
     requires `onUnevaluatedProperties`",
    "$.components.schemas.Encoded.contentSchema requires `onContentSchema`",
    "$.components.schemas['x-Legacy'] requires `onSchemaNamedLikeAnExtension`",
];

/// The other half of reading by position: every place the specification does
/// put a schema is still read, so that the walk was narrowed rather than
/// switched off.
///
/// An allowlist fails by dropping a route in silence, and this is the test that
/// would say so — so the fixture spells every key the table does rather than a
/// representative few, and deleting one of them takes a line off the reading.
#[test]
fn every_position_a_schema_stands_in_is_still_read() {
    assert_eq!(named(EVERYWHERE), EVERY_PHANTOM);
}
