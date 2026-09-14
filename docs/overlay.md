# Writing corrections

How to say that the vendor's document is wrong, in a file the vendor's own
tooling can still read. The primer is [the README](../README.md); what happens
when a correction later stops fitting is [docs/drift.md](drift.md).

## Contents

- [Why an Overlay](#why-an-overlay)
- [Action kinds](#action-kinds)
- [The tripwire form](#the-tripwire-form)
- [Layers](#layers)
  - [The vendor layer](#the-vendor-layer)
  - [The type layer](#the-type-layer)
  - [The CLI layer](#the-cli-layer)
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
in the example adoption are updates; the one above adds a property to a schema.

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
    description: Say what an amount looks like; the vendor declares the format and not the rule.
    update:
      pattern: ^-?[0-9]+(\.[0-9]{1,2})?$
```

The filter selects `Voucher`'s `properties` member only while `total` is still
declared `format: money`; the step after it names `total`. A plain
`$.components.schemas.Voucher.properties.total` would do the same edit — and
would keep doing it silently after the vendor retyped the field as a number,
stamping a decimal-string pattern onto a schema that is no longer a string. The
filter form fails instead, and names itself:

```
spec/corrections.yaml: the Overlay does not apply: actions[0] (target "$.components.schemas.Voucher[?(@.total.format == 'money')].total"): target matched zero nodes (error-on-zero-match)
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
holds both tripwires to their fixtures.

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
    .replace("money", "api_types::Money")
    .write_to("api-generated")?;
```

The library reads an ordered list of standard Overlay documents and nothing
more. It has no idea what any layer is *for*, and there is no enum, no schema
and no naming rule to satisfy. What follows is a convention worth keeping, and
the failure message is the practical argument for it: a tripwire that stops the
bless names the file it is in, so splitting by purpose means the message points
at the person whose problem it is.

Three kinds of thing end up in a layer. The example adoption commits two files,
because its one type rule is a single line and rides along with the vendor
layer; an adoption with a dozen of them wants the third file.

### The vendor layer

**What the vendor got wrong** — an undocumented field, an operation the vendor
ships and never documented, a `required` list that does not match what the
server actually demands.
[`examples/toy/spec/corrections.yaml`](../examples/toy/spec/corrections.yaml)
is this layer.

Applying it alone yields the document the vendor *should* have shipped. That is
worth having on its own: it can go back to the vendor, or into a generator for
another language, or to a mock server, and it says nothing about how you
happen to consume the API. An `x-cli-` marker in it would spoil exactly that,
which is why `api/tests/corrections.rs` asserts there is none.

### The type layer

**Formats and validation rules** — what the vendor declares a format for and
never says the rule for. The money `pattern` above is one; it lives in the
vendor layer in the example only because there is one of it.

This layer is what [`Settings::replace`](generating.md#settingsreplaceformat-rust_type---settings)
keys on. Give `Voucher.currency` a `format: currency` and pair it with
`.replace("currency", "api_types::Currency")`, and the generated `Voucher.currency`
is your own newtype rather than a `String` — parsed once, at the boundary,
with no mirror type and no conversion:

```yaml
  - target: "$.components.schemas.Voucher[?(@.currency.type == 'string')].currency"
    description: The vendor types an ISO 4217 code as a bare string.
    update:
      format: currency
      pattern: ^[A-Z]{3}$
```

**What this layer does not buy you is CLI-side enforcement of the `pattern`.**
The Rust type is enforced by Rust: `Currency::from_str` runs wherever a
`Voucher` is parsed or constructed. But the command line's own value check —
[`Scalar::parse`](../typed-openapi/src/scalar.rs) — enforces only `format:
money` and enumerations. An arbitrary ECMA-262 `pattern` reaches the help line
and nothing else, because enforcing one would cost a regex engine in every
shipped binary. So `--currency GBP` and `--currency gbp` both reach
`Invocation::new`; the second is refused later, by the generated type, on the
`toy raw` path that vets bodies — and not at all on a `dispatch`-only CLI. If a
value must be refused at the parser, the document's own `enum` is the tool that
does it.

### The CLI layer

**What only a command line needs** — the three `x-cli-` extensions, under the
`x-` prefix OpenAPI reserves for exactly this.
[`examples/toy/spec/cli.yaml`](../examples/toy/spec/cli.yaml) is this layer.

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

## CORRECTIONS, and the test that holds it

YAML files of JSONPath targets are the mechanism, not the answer to "what have
we changed, and is it still needed?".
[`CORRECTIONS`](../examples/toy/api/src/corrections.rs) is that answer — one
row per decision, in Rust an adopter can read:

```rust,ignore
pub const CORRECTIONS: &[Correction] = &[
    Correction::Retyped { format: "money", rust: "Money" },
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
  vendor still declares the format it keys on; an undocumented operation only
  while the vendor still omits it. When the vendor catches up, the test says to
  drop the action and the row.
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
