# Writing corrections

How to say that the vendor's document is wrong, in a file the vendor's own
tooling can still read. The primer is [the README](../README.md); what happens
when a correction later stops fitting is [docs/drift.md](drift.md).

## Contents

- [Why an Overlay](#why-an-overlay)
- [Action kinds](#action-kinds)
- [The tripwire form](#the-tripwire-form)
- [Layers](#layers)
  - [1. Plain corrections](#1-plain-corrections)
  - [2. Type validations and newtypes](#2-type-validations-and-newtypes)
  - [3. Grouping and the command line](#3-grouping-and-the-command-line)
- [Owning the type yourself](#owning-the-type-yourself)
- [CORRECTIONS, and the test that holds it](#corrections-and-the-test-that-holds-it)

## Why an Overlay

The corrections live in an [OpenAPI Overlay][spec] rather than in a patch format
invented here, for one reason: the output is a standard OpenAPI document. The
bless step applies the Overlays and commits the result
([`examples/toy/api-generated/spec/toy.overlaid.yaml`](../examples/toy/api-generated/spec/toy.overlaid.yaml)),
and anything that reads OpenAPI can read it — a mock server, a Postman
collection, another generator, a reviewer. A correction expressed as Rust would
be true only for the Rust.

The vendor's document is never edited. `extends` records which document these
corrections are for; the crate is handed both files' contents and does not
resolve it, so the path is documentation for a human and for other Overlay
tooling.

```yaml
overlay: 1.1.0
info:
  title: Corrections to the Toy Accounting API
  version: "1.0"
extends: toy.yaml
actions:
  - target: $.components.schemas.Voucher.properties
    description: Add the undocumented `internal_ref` field.
    update:
      internal_ref:
        type: string
        description: Vendor's internal bookkeeping reference (undocumented)
```

The `overlay` field has to match `1.1.x`. A document declaring `1.0.0` is
refused before any action runs:

```
the Overlay document is not valid YAML or JSON: invalid value: string "1.0.0", expected `1.1.<patch>` semver (Overlay v1.1)
```

Overlay files are YAML or JSON, and so are the documents they extend.

## Action kinds

An action is a `target` — an RFC 9535 JSONPath — plus exactly one of three
verbs.

**`update`** merges a value into every node the target selects. All four actions
the example adoption commits are updates; the one above adds a property to a
schema.

**`remove: true`** deletes the selected nodes from their container. This is how
an operation leaves the API — drop it from `paths` and it has no wrapper, no
subcommand and no inventory row. The example adoption has no `remove` action,
because nothing is skipped; the form is the one the unit tests in
[`src/overlay.rs`](../typed-openapi/src/overlay.rs) exercise, and the row that
would accompany one in the summary list is `Correction::Skipped`:

```yaml
  - target: $.paths['/vouchers/{id}/legacy-export']
    description: The vendor ships it; we do not offer it.
    remove: true
```

**`copy`** merges a node the target document already has — named by a second
JSONPath — into every selected node. Nothing in this repository uses it.

## The tripwire form

This is the idea worth taking away. An Overlay is applied with
`ErrorOnZeroMatch` ([`src/overlay.rs`](../typed-openapi/src/overlay.rs)), so an
action whose target selects nothing is an error rather than a quiet no-op. That
turns a JSONPath into an assertion: **write the target as a filter over what the
vendor currently says, and the correction checks its own premise every time the
bless step runs.**

```yaml
  - target: "$.components.schemas.Voucher[?(@.total.format == 'money')].total"
    description: Point the vendor's amount at the rule.
    update:
      $ref: "#/components/schemas/Money"
```

The filter selects `Voucher`'s `properties` member only while `total` is still
declared `format: money`; the step after it names `total`. A plain
`$.components.schemas.Voucher.properties.total` would do the same edit — and
would keep doing it silently after the vendor retyped the field as a number,
pointing an amount's rule at a schema that is no longer a string. The filter
form fails instead, and names itself:

```
spec/corrections.yaml: the Overlay does not apply: actions[1] (target "$.components.schemas.Voucher[?(@.total.format == 'money')].total"): target matched zero nodes (error-on-zero-match)
```

The same trick works on a path item. Targeting `.get` rather than the path
itself asserts that the operation is still a `GET`:

```yaml
  - target: $.paths['/vouchers/{id}/render'].get
    description: Mark the rendering GET as a write, so `--commit` gates it.
    update:
      x-cli-writes: true
```

A vendor who moves that operation to `POST` makes this a zero match — which is
the right outcome, because a `POST` is already gated and the correction has
become redundant. [`typed-openapi/tests/drift.rs`](../typed-openapi/tests/drift.rs)
holds every tripwire to a mutated fixture; the example adoption has three, two
in the vendor layer and one in the CLI layer.

The cost is deliberate: every tripwire is a place a vendor revision stops the
build. That is the trade — you are buying a failure you can read in exchange for
a silent one you cannot.

## Layers

`Settings::overlay` may be called more than once, and the order of the calls is
the order the Overlays are applied — each one corrects the document the ones
before it produced:

```rust,ignore
Settings::new("spec/toy.yaml")
    .overlay("spec/corrections.yaml")
    .overlay("spec/cli.yaml")
    .write_to("api-generated")?;
```

**The library reads an ordered list of standard Overlay documents and nothing
else.** No layer names, no enum, no schema, and no check that a given action
belongs in a given file. Everything below is a recommendation for how to use
it, kept by the example's file names and by one test — not a rule the crate
enforces.

The split is by **audience**, and the question that sorts an action is: *who
else could use this?*

| layer | holds | audience |
|---|---|---|
| `corrections.yaml` | what is true of the API and the vendor got wrong or left out | anyone — the vendor, a TypeScript generator, a mock server, a request validator |
| `client.yaml` | what is true of *your* client but not of the API | you, in any language |
| `cli.yaml` | `x-cli-writes`, `x-cli-group`, `x-cli-command` | the command-line half of this crate |

Later layers may say things earlier ones must not, so the order is not a
preference. The practical argument is also small and immediate: a tripwire that
stops the bless names the file it is in, so a failure points at whoever owns
that layer.

The example adoption commits two of the three. Writing the third with nothing
in it is not possible — an Overlay with no actions is not a valid Overlay 1.1
document, and an empty file is a worse artefact than none — so what a
`client.yaml` action looks like is written out below instead.

### 1. Plain corrections

**What is true of the API.** The vendor's document is wrong or silent, and a
consumer who never heard of this crate would want the correction too.

A property the vendor returns and accepts and never documented:

```yaml
  - target: $.components.schemas.Voucher.properties
    description: Add the undocumented `internal_ref` field.
    update:
      internal_ref:
        type: string
        description: Vendor's internal bookkeeping reference (undocumented)
```

An operation the vendor ships and documents nowhere:

```yaml
  - target: $.paths
    description: Add the undocumented `archiveVoucher` operation.
    update:
      /vouchers/{id}/archive:
        post:
          operationId: archiveVoucher
          summary: Archive a voucher (undocumented; vendor ships it)
          responses:
            "200": { description: OK }
```

A content type that is not one. A media type is `type/subtype`, so a body
declared under the bare key `form-data` names none, and there is nothing to
send that body under. `typed-openapi` refuses the document while it reduces it
rather than putting the word on the wire as a `Content-Type`:

```
uploadAttachment: `form-data` is not a media type; an Overlay is where a document's content type is corrected
```

Which type the vendor meant is a judgement, which is why the crate leaves it to
you. Two actions state it: `remove` takes the key out, `update` puts the one
the vendor meant in its place.

```yaml
  - target: "$.paths['/attachments'].post.requestBody.content['form-data']"
    description: The vendor means `multipart/form-data`.
    remove: true

  - target: $.paths['/attachments'].post.requestBody.content
    description: Say it the way the wire spells it.
    update:
      multipart/form-data:
        schema:
          type: object
          properties:
            file: { type: string, format: binary }
```

This is the layer for it. The vendor's server reads a multipart body whatever
its document says, so the repair is true of the API: a TypeScript generator, a
mock server and a request validator all want it, and it is a correction worth
handing back — the document is the thing that is wrong.

A rule the vendor names and never states. There are two ways to name one
without stating it, and the toy document has both.

`format: money` says that somebody somewhere knows what an amount is; `pattern`
says it in plain JSON Schema, which every consumer of the document can run. So
this belongs here and not in a client layer: a TypeScript generator honours it,
a request validator honours it, and leaving it out hands the vendor back a
document that still does not say what an amount is.

Give the rule a **schema name** and it is stated once for every field that
carries an amount. Two actions, and the second is a tripwire:

```yaml
  - target: $.components.schemas
    description: Say what an amount is, once, under a name.
    update:
      Money:
        type: string
        format: money
        description: A decimal amount carried in a string.
        pattern: ^-?[0-9]+(\.[0-9]{1,2})?$

  - target: "$.components.schemas.Voucher[?(@.total.format == 'money')].total"
    description: Point the vendor's amount at that rule.
    update:
      $ref: "#/components/schemas/Money"
```

`update` merges, so the vendor's own `type`, `format` and `description` stay
where the vendor put them and `total` keeps its place in `properties` —
`--help` still reads in document order. What the action adds is the reference,
and OpenAPI 3.0 reads a `$ref` in preference to whatever sits beside it.

**Which is also what the reference costs: the field's own description.** What
`--help` shows after the correction is the named schema's sentence, and that is
the same sentence under every field sharing the rule. The 3.0 spelling that
keeps both puts the reference inside an `allOf` of one element and the sentence
outside it, where nothing overrides it:

```yaml
total:
  allOf:
    - $ref: "#/components/schemas/Money"
  description: The gross total of this voucher.
```

`typed-openapi` reads a single-element `allOf` as the schema it wraps, so the
rule still reaches `--total` and the field keeps its own words; a field that
says nothing of its own takes the named schema's. The wrapper has to be the
whole of the field's schema — a `type` or `format` left beside the `allOf`
makes the node a composition of a different kind, and not a flag at all — so an
Overlay writing this shape over a typed field removes the property and adds it
back rather than merging into it. An `allOf` of two schemas is a real
composition and is no flag either.

The other way to name a rule without stating it is prose. `Voucher.currency` is
`type: string` described as "ISO 4217 code" — a sentence a person can follow and
nothing can run. An ISO 4217 code is three uppercase letters, so the document
says that, in the same two actions:

```yaml
  - target: $.components.schemas
    description: Say what a currency code is, once, under a name.
    update:
      Currency:
        type: string
        description: An ISO 4217 currency code.
        pattern: ^[A-Z]{3}$

  - target: "$.components.schemas.Voucher[?(@.currency.type == 'string')].currency"
    description: Point the vendor's currency code at that rule.
    update:
      $ref: "#/components/schemas/Currency"
```

**What a named schema buys is both halves at once.** `typed-openapi` follows
the `$ref` while it reduces the document, so `--currency` refuses `eur` with
the document's own pattern; typify reads the schema *name*, so
`Voucher.currency` is a `Currency` newtype whose `FromStr` runs the same pattern
on the same engine. Neither half is written in Rust, and neither can drift from
the other.

`Money` carries the vendor's `format` onto the named schema as well, and that
is the second route to a type — [owning the type yourself](#owning-the-type-yourself),
below. It leaves this layer no less portable: the tag is the vendor's own and
says nothing a consumer has to act on, and what reads it is a `Settings::replace`
call that lives in Rust rather than here.

**Nothing in this layer may be specific to this crate.** That is what makes the
layer worth keeping separate: this file plus the vendor's document *is* the
document the vendor should have shipped, and any Overlay 1.1 implementation
will produce it — the file is not special to `typed-openapi`. With this crate:

```rust,ignore
let vendor = fs::read_to_string("spec/toy.yaml")?;
let corrections = fs::read_to_string("spec/corrections.yaml")?;
let corrected = overlay::apply(overlay::parse(&vendor)?, &corrections)?;
print!("{}", serde_yaml_ng::to_string(&corrected)?);
```

This repository does not commit that output. It is derived data with no reader
here, and a committed file nothing reads is a file that rots. (Contrast
`api-generated/spec/toy.overlaid.yaml`, which stays: it is embedded as
`DOCUMENT` and `api/tests/typed.rs` holds the reduced blob to it. An in-repo
consumer is what earns a generated file its place.)

`api/tests/corrections.rs` asserts this layer carries no `x-cli-` key anywhere.
The test is on the file a person edits, so it fails at the moment someone drops
a CLI concern into the layer that is meant to be consumable by anyone.

### 2. Type validations and newtypes

**What is true of your client but not of the API.** The vendor is not wrong;
you want something narrower, and only for yourself.

The worked case is a tightening. `listVouchers`'s `limit` in
[`examples/toy/spec/toy.yaml`](../examples/toy/spec/toy.yaml) is a bare
`int32`, which is a fair description of an API that will take any of them and
answer 400 for the silly ones. Your client never asks for more than a page, so
say so here:

```yaml
# spec/client.yaml
  - target: "$.paths['/vouchers'].get.parameters[?(@.name == 'limit')].schema"
    description: This client never asks for more than a page.
    update:
      maximum: 100
```

The other kind is a **narrowing** of an `enum`: the API accepts five statuses
and your client only ever deals in three, so you shrink the list. Either way it
is a statement about your client, and it must never reach `corrections.yaml` —
a narrowing handed back to the vendor is a bug report about an API that is
behaving correctly.

**What this layer buys.** Exactly what layer 1 buys, aimed at yourself: the rule
is enforced on the command line, and `--limit 500` is refused at the parser with
the document's own number in the message. Name a schema rather than writing the
rule inline and a generated newtype carries it too — the same trick `Currency`
uses one layer down. The difference is only who the statement is true of — see
[validation.md](validation.md) for what each keyword buys.

### 3. Grouping and the command line

**What only this crate reads** — the three `x-cli-` extensions, under the `x-`
prefix OpenAPI reserves for exactly this.
[`examples/toy/spec/cli.yaml`](../examples/toy/spec/cli.yaml) is this layer,
and it is last because nothing else has any use for what is in it.

| marker | on | says |
|---|---|---|
| `x-cli-writes: true` | an operation | hold it behind `--commit`, whatever its method is |
| `x-cli-group: <name>` | an operation | mount it under this group rather than the one its path names |
| `x-cli-command: <name>` | an operation | call it this rather than what its path and method name |

The last two are also the only way out of a name collision — two operations
reducing to one `<group> <command>` is a `LoadError` at bless time naming both
`operationId`s, never a silent rename. [docs/cli.md](cli.md#the-shape-of-the-tree)
has the rule they overrule.

```yaml
  - target: $.paths['/vouchers/{id}/enshrine'].post
    description: `vouchers finalize` is what the team calls it.
    update:
      x-cli-command: finalize
```

## Owning the type yourself

A named schema hands you a newtype the generator wrote, and the rule it
enforces is the document's. Sometimes the type you need is one the document
cannot describe at all.

**A fixed-point decimal is that case, and it is not an exotic one.** OpenAPI
has no such type. `type: number` is an IEEE 754 double in every JSON parser
that matters, so a column of amounts does not add up; `multipleOf: 0.01` cannot
rescue it, because `0.01` has no binary representation either. `type: string`
with a `pattern` states what an amount *looks like* and nothing about
arithmetic on it — which is the right thing for a document to say, and not
enough to book a ledger with. `format` is an open annotation with no registered
decimal entry, so it names the gap without closing it.

So the wire form is expressible and the type is not. Tag the shape with a
`format` — here the vendor's own — and hook a Rust path onto it with
[`Settings::replace`](generating.md#settingsreplaceformat-rust_type---settings):

```yaml
# spec/corrections.yaml
  - target: $.components.schemas
    description: Say what an amount is, once, under a name.
    update:
      Money:
        type: string
        format: money
        description: A decimal amount carried in a string.
        pattern: ^-?[0-9]+(\.[0-9]{1,2})?$
```

```rust,ignore
Settings::new("spec/toy.yaml")
    .overlay("spec/corrections.yaml")
    .overlay("spec/cli.yaml")
    .replace("money", "money::Money")   // the other half
    .write_to("api-generated")?;
```

Both halves are needed and neither means anything alone: the document says
which shape, the call says which type. `rust_type` is written into the
generated source verbatim, so the crate owning it has to be a dependency of the
generated crate — which usually means a small crate *below* it, since the
generated code names it.
[`examples/toy/money`](../examples/toy/money/src/lib.rs) is that crate: one
type, `serde`, `thiserror` and `num-bigint`, and no dependency on
`typed-openapi` at all. `Voucher.total` is a `money::Money`, and typify defines
nothing for it.

What the owned type is made of stays the adopter's business. `num-bigint` is
declared in that crate's manifest and in no other, and `just bigint-free`
asserts it never reaches `typed-openapi` in any feature set — the library has no
business knowing what a replaced type is built from, which is the premise
`replace` rests on.

**The two halves compose, and that is the point.** The `pattern` stays in the
document, so `--total 12,50` is still refused on the command line with the
document's own rule; `replace` supplies exact arithmetic over cents, which the
document had no way to ask for. Neither is a substitute for the other: a
`format` alone states no rule, and a `pattern` alone hands you a string.

The type is free to pick a representation the document never mentions, and the
choice pays for itself: an arbitrary-precision count of cents cannot overflow,
so `+`, `-`, `-x` and `sum()` are plain total operations with no `Option` in
sight — and the type accepts *every* value the pattern admits, which a
fixed-width integer could not.

What you buy with the second half is a type free to disagree with the wire.
`money::Money` prints `12,50` for a person and sends `12.50`, so `Display` and
`FromStr` are deliberately not inverses — a rendering choice no OpenAPI
document can state, and one a generated newtype could never have made.

The trade against the named-schema route above:

| | named schema | `replace` and a type of your own |
|---|---|---|
| the type's name | the schema's | yours |
| `Display`, arithmetic, conversions | what the generator emits | anything you write |
| enforced on the command line | yes | only if the document states the rule too |
| held to the document | by construction — typify compiles the same `pattern` | by a test you write |
| your dependency tree | nothing — the generator writes the type | whatever your type is built from |
| the regex engine | linked either way ([validation.md](validation.md#what-it-costs)) | linked either way |

Neither route buys or saves the regex engine: `typed-openapi` links `regress`
unconditionally so that a command line can enforce whatever pattern the
document states, and it is in the tree of every crate here — `api`'s included,
which links no argument parser. The 810 KB is the price of enforcing rules at
all, not of choosing one of these two routes.

**The dependency row is a real cost, and it is yours alone.** Exact money over
an integer that cannot overflow means `num-bigint` and its `num-integer` /
`num-traits` tail — three crates the adoption would not otherwise compile, and
what arbitrary precision costs. They land in `examples/toy/money`'s manifest and
travel up into the binary that uses it; `just bigint-free` asserts they never
reach `typed-openapi` in any feature set, because the crate that is published
has no business knowing what a replaced type is made of. Choosing a narrower
count, or a different crate, or no crate at all, is the adopter's call to make
and the library never learns it was made.

**The price of `replace` is the *held to the document* row.** A generated
newtype cannot drift from the document; a hand-written one can, and nothing
notices unless you make it. [`examples/toy/api/tests/money.rs`](../examples/toy/api/tests/money.rs)
is that: it reads the `pattern` out of the embedded document and holds
`Money::from_str` to it over every edge the pattern has, in both directions, so
a vendor who widens the rule fails there instead of quietly admitting values the
type refuses.

Reach for `replace` when you need behaviour on the type, or when no JSON Schema
can say what the value *is*. Reach for the named schema alone when you need the
rule and nothing more. The example adoption does both on adjacent fields of one
schema: `Voucher.currency` is a generated `Currency`, `Voucher.total` a
hand-owned `Money`, and a `format` tag *and* a `pattern` is what buys the
second without giving up the first.

## CORRECTIONS, and the test that holds it

YAML files of JSONPath targets are the mechanism, not the answer to "what have
we changed, and is it still needed?".
[`CORRECTIONS`](../examples/toy/api/src/corrections.rs) is that answer — one
row per decision, in Rust an adopter can read:

```rust,ignore
pub const CORRECTIONS: &[Correction] = &[
    Correction::Retyped { schema: "Voucher", property: "total", named: "Money" },
    Correction::Retyped { schema: "Voucher", property: "currency", named: "Currency" },
    Correction::Undeclared { schema: "Voucher", property: "internal_ref" },
    Correction::Undocumented("archiveVoucher"),
    Correction::Gated("renderVoucher"),
];
```

It is not a second copy of the Overlays, because
[`examples/toy/api/tests/corrections.rs`](../examples/toy/api/tests/corrections.rs)
checks every row against *both* documents — the vendor's, exactly as it ships,
and the corrected one this crate embeds — in both directions:

- **A row that no longer describes a difference fails.** A gate is a correction
  only while the vendor's own method says otherwise; a retype only while the
  vendor leaves the rule unsaid and the schema stating it is the Overlay's own;
  an undocumented operation only while the vendor still omits it. When the
  vendor catches up, the test says to drop the action and the row.
- **A difference with no row fails.** The test walks every schema property of
  both documents and every operation's effect. A property the corrected
  document has and the vendor's does not, without an `Undeclared` row, is a
  failure. So is editing an Overlay and not saying so here.

The `Correction` enum is matched exhaustively with no wildcard arm, so adding a
variant is a compile error in the test — where someone has to decide what
checking it means — rather than a row that quietly goes unchecked.

Where a correction goes wrong later, and what fails when:
[docs/drift.md](drift.md).

[spec]: https://spec.openapis.org/overlay/v1.1.0.html
