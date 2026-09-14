# Writing corrections

How to say that the vendor's document is wrong, in a file the vendor's own
tooling can still read. The primer is [the README](../README.md); what happens
when a correction later stops fitting is [docs/drift.md](drift.md).

## Why an Overlay

The corrections live in an [OpenAPI Overlay][spec] rather than in a patch format
invented here, for one reason: the output is a standard OpenAPI document. The
bless step applies the Overlay and commits the result
([`examples/toy/api-generated/spec/toy.overlaid.yaml`](../examples/toy/api-generated/spec/toy.overlaid.yaml)),
and anything that reads OpenAPI can read it — a mock server, a Postman
collection, another generator, a reviewer. A correction expressed as Rust would
be true only for the Rust.

The vendor's document is never edited. `extends` records which document these
corrections are for; `overlay::apply` is handed both files' contents and does
not resolve it, so the path is documentation for a human and for other Overlay
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
in [`examples/toy/spec/overlay.yaml`](../examples/toy/spec/overlay.yaml) are
updates; the one above adds a property to a schema.

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
the Overlay does not apply: actions[0] (target "$.components.schemas.Voucher[?(@.total.format == 'money')].total"): target matched zero nodes (error-on-zero-match)
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

## The four corrections, and the one non-correction

The toy vendor's document is wrong in five ways.
[`examples/toy/spec/overlay.yaml`](../examples/toy/spec/overlay.yaml) corrects
four of them.

| # | kind | what it buys |
|---|---|---|
| 1 | `update`, tripwire | `Voucher.total` carries `format: money` with no rule for what an amount is. The action adds the `pattern`. The bless step reads the format off the corrected shape and emits `Money`, so `Voucher.total` is the adopter's newtype with no mirror type and no conversion — and `--total` gets an `<AMOUNT>` value parser. |
| 2 | `update` | The vendor returns and accepts `internal_ref` and documents it nowhere. Adding it to `Voucher.properties` puts it on the generated struct, on the wrapper and on `--internal-ref` at once, for no hand-written Rust. |
| 3 | `update` | `archiveVoucher` is shipped and undocumented. Adding the path item to `$.paths` makes it a generated wrapper and a subcommand like any other. |
| 4 | `update`, tripwire | `renderVoucher` is a `GET` that stores a PDF. HTTP cannot say "this GET writes", so `x-cli-writes: true` does, and `--commit` gates it. |

The fifth is left alone, and that is the other half of the lesson: not every
vendor mistake needs an action. `uploadDocument` declares its request body under
the media type `form-data` instead of `multipart/form-data`. An unknown media
type is not a failure — it becomes `--raw-body FILE`, and the request goes out
under the vendor's own spelling, which
[`api/tests/typed.rs`](../examples/toy/api/tests/typed.rs) asserts
(`the_misspelled_upload_sends_the_vendors_own_media_type`).
`uploadDocumentMultipart`, spelled correctly, gets `--file` and `--field`
instead. Both operations work, so there is no operation this crate refuses to
offer and nothing is listed as skipped.

## CORRECTIONS, and the test that holds it

A YAML file of JSONPath targets is the mechanism, not the answer to "what have
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

It is not a second copy of the Overlay, because
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
  failure. So is editing `spec/overlay.yaml` and not saying so here.

The `Correction` enum is matched exhaustively with no wildcard arm, so adding a
variant is a compile error in the test — where someone has to decide what
checking it means — rather than a row that quietly goes unchecked.

Where a correction goes wrong later, and what fails when:
[docs/drift.md](drift.md).

[spec]: https://spec.openapis.org/overlay/v1.1.0.html
