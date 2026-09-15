# Drift

Every way a vendor revision is caught, where it is caught — and the ones that
are not.

The document moves without asking. What matters is not that a change is
*detected* but that it is detected somewhere a human will look, in a message
that names the thing that moved. There are three such places, in order of how
early they bite:

- **bless time** — `just bless` lays every Overlay over the vendor's document
  in order and rewrites the generated crate. An Overlay action that no longer
  matches fails here, naming the layer it is in.
- **compile time** — the generated types and inventory are ordinary Rust. Code
  that names a field, a variant or an operation stops compiling.
- **test time** — `just test` holds the committed artefacts to each other and
  the corrections to both documents.

## The table

| the vendor… | what fails | when | and says |
|---|---|---|---|
| renames or retypes a field an Overlay action corrects | that layer, under `ErrorOnZeroMatch` | bless | the layer's file name, then target matched zero nodes, quoting the JSONPath |
| moves a corrected operation to another method | the same | bless | the same, quoting the other JSONPath |
| adds an operation that reduces to a command another one already has | `Document::load` | bless | both `operationId`s, and that one needs an `x-cli-command` |
| withdraws an operation an Overlay action names | the same | bless | the same, quoting the action's JSONPath |
| withdraws an operation nothing in an Overlay names, that something depends on | `const _: () = assert!(documented(..))` | compile | E0080, quoting the whole assertion |
| adds a field to a schema the adopter destructures | the exhaustive `let Voucher { … }` | compile | E0027, naming the added field |
| renames or removes such a field | the same | compile | E0026, naming the missing field and suggesting the new one |
| adds an enum variant | a `_`-less `match` | compile | E0004, naming the uncovered variant |
| ships half a bless — inventory and model from different runs | `Document::matches`, in `Api::new` | startup, so every test | which position disagrees, and both names |
| leaves the committed blob and the committed document disagreeing | `api/tests/typed.rs` | test | which operation is not what the document says it is |
| catches up with a correction, or drops something a correction names | `api/tests/corrections.rs` | test | which row no longer describes a difference |
| widens the rule a hand-owned type stands for | `api/tests/money.rs` | test | which value the document now admits and `Money` refuses |
| **adds an operation** | — | — | **nothing** |
| **adds a field to a schema nothing destructures or constructs** | — | — | **nothing** |
| **changes a summary or a description** | — | — | **nothing** |
| **loosens a rule nothing hand-written stands for** | — | — | **nothing**, until a value the old rule refused arrives |

The verbatim messages are in the sections below.

## Bless time: the Overlay as an assertion

`ErrorOnZeroMatch` is what makes a correction a check as well as an edit. Five
of the example adoption's nine actions — two in
[`spec/corrections.yaml`](../examples/toy/spec/corrections.yaml), three in
[`spec/cli.yaml`](../examples/toy/spec/cli.yaml) — are written as targets that
state what the vendor currently says, so a revision that changes the thing being
corrected fails the bless rather than being silently overwritten. The message opens with the layer, because with corrections split
by purpose that is the first thing to know:

```
spec/corrections.yaml: the Overlay does not apply: actions[1] (target "$.components.schemas.Voucher[?(@.total.format == 'money')].total"): target matched zero nodes (error-on-zero-match)
```

The form and its cost are in [docs/overlay.md](overlay.md#the-tripwire-form);
[`typed-openapi/tests/drift.rs`](../typed-openapi/tests/drift.rs) is where both
tripwires are held to a mutated fixture.

The same file records what *passes* this stage, and deliberately so: withdrawing
an operation no action names, and adding or removing a field the Overlay never
mentions, both bless cleanly. Those are the compiler's.

## Compile time: the operation inventory

The bless step emits `OPERATIONS`, a `(operationId, method, path)` row per
operation, and a `const fn documented` over it. Put an assertion beside anything
that depends on an operation and its disappearance becomes an E0080 that names
it — [`examples/toy/cli/src/finalize.rs`](../examples/toy/cli/src/finalize.rs)
does exactly that for the two operations its chain calls:

```rust,ignore
const _: () = assert!(api::documented(
    "enshrineVoucher",
    "POST",
    "/vouchers/{id}/enshrine"
));
```

```
error[E0080]: evaluation panicked: assertion failed: api::documented("enshrineVoucher", "POST", "/vouchers/{id}/enshrine")
  --> examples/toy/cli/src/finalize.rs:19:15
```

The assertion is a whole row, so an operation that keeps its `operationId` and
moves to a different path or method fails it too.

Both operations that chain calls are named by the CLI layer as well — one is
marked `x-cli-writes`, the other stands behind a gate — so in this adoption a
withdrawal stops the bless before this ever compiles. The assertion is what
catches the case no Overlay happens to cover.

This is opt-in per dependency. An operation nobody asserts on simply stops
having a subcommand.

## Compile time: fields and variants

Generated types are ordinary structs and enums, so the ordinary exhaustiveness
rules apply — as long as nothing opts out of them.
[`examples/toy/api/src/posting.rs`](../examples/toy/api/src/posting.rs)
destructures `Voucher` with every field named:

```rust,ignore
let Voucher { currency, id, internal_ref, status, total } = voucher;
```

A field added upstream is E0027, and it names the field:

```
error[E0027]: pattern does not mention field `note`
  --> examples/toy/api/src/posting.rs:44:13
```

A field renamed is E0026, and rustc suggests the new name:

```
error[E0026]: struct `api_generated::types::Voucher` does not have a field named `total`
  --> examples/toy/api/src/posting.rs:49:13
   |
   |             total,
   |             ^^^^^ help: `api_generated::types::Voucher` has a field named `amount`
```

A rename reports both: E0026 for the name that is gone and E0027 for the one
that arrived.

The two escapes from that are `..` and `field: _`, and
[`examples/toy/api/src/lib.rs`](../examples/toy/api/src/lib.rs) turns on a lint
against each, scoped to the crate:

```rust,ignore
#![warn(clippy::rest_pattern_accessible_field, clippy::unneeded_field_pattern)]
```

`just check` runs clippy with `-D warnings`, which promotes both to errors, so
neither survives the gate. A hand-written conversion out of a generated type
cannot skip a field, and the tripwire cannot be disarmed by accident.

Enums work the same way through matches with no `_` arm — one in `Posting::of`,
one in `finalize::plan`. A variant the vendor adds is:

```
error[E0004]: non-exhaustive patterns: `&api_generated::types::VoucherStatus::Void` not covered
  --> examples/toy/api/src/posting.rs:57:27
```

`api` fails before `cli` is reached, so one such change reports once, in the
crate that owns the decision.

## Test time: the committed artefacts

A bless step writes four files and the CLI only ever reads one of them — the
binary blob. Four checks hold the set together.

[`api/tests/typed.rs`](../examples/toy/api/tests/typed.rs) reduces the
*committed document* again and compares it with the *committed blob*, operation
by operation. A bless run on a stale document, a blob edited by hand, a
generated file committed without its blob: all of them are this assertion, and
it names what moved rather than printing both copies of everything.

```
assertion `left == right` failed: `listVouchers` is not what the document says it is
  left: Operation { … summary: Some("List vouchers") … }      # the blob
 right: Operation { … summary: Some("List all vouchers") … }  # the document
```

(Both sides print in full; the ellipses are this page's.)

`Api::new` makes the other pairing before it hands out a handle: the embedded
model and the generated inventory, row for row. It is a startup check rather
than a test, so a half-finished bless is a named error before a command tree is
built:

```
the document and the generated inventory disagree at operation 10: the inventory
says `voidVoucher` and the document says there is no such operation
```

[`api/tests/corrections.rs`](../examples/toy/api/tests/corrections.rs) is the
third: every row of `CORRECTIONS` is checked against both documents in both
directions, so a correction the vendor has caught up with turns red instead of
going on working in silence. One test per kind of row, each saying which row and
why:

```
`renderVoucher` is listed as gated but the vendor's own method already writes
`Voucher.internal_ref` is listed as undeclared but the vendor now declares it
`archiveVoucher` is listed as undocumented but the vendor now describes it: drop the Overlay action and the row
```

And in the other direction — `` `Voucher.note` is in the corrected document and
not the vendor's, and no row says so `` — so an Overlay edit nobody wrote down
fails too. [docs/overlay.md](overlay.md) has the detail.

[`api/tests/money.rs`](../examples/toy/api/tests/money.rs) is the fourth, and it
exists because the reading behind one type in this adoption is hand-written. A
newtype the generator writes the whole of compiles the document's `pattern` into
its own `FromStr`, so it cannot drift; `Voucher.total` is a newtype whose
`FromStr` hands the whole reading to `money::Money`, the adopter's own, reached
through [`Settings::replace`](overlay.md#owning-the-type-yourself) and borrowing
nothing. The test reads the rule off the embedded document — the very `Scalar`
that refuses a `--total` — and holds the type to it over every edge the pattern
has, in both directions:

```
`12,50`: the document accepts it and `Money` refuses it
the document's rule has moved: it now accepts `12,50`
```

There is no value they disagree about, and the representation is why: the
pattern admits an unbounded run of digits, so `Money` counts cents in an
arbitrary-precision integer. A narrower one would refuse amounts the document
allows, and this page would have to list the gap — a second test says so, at
nineteen, twenty, forty and a hundred digits.

## What is not caught

A page that claims everything is caught is worth less than one that says where
the holes are.

**An operation the vendor adds.** The bless step picks it up, emits a wrapper
and mounts a subcommand, and `OPERATION_COUNT` moves. Nothing asserts on that
number, and `corrections.rs` is satisfied — the operation is in the vendor's
document *and* mounted, which is exactly the state it checks for. The new
operation shows up as a `git diff` after `just bless` and nowhere else.

**A field added to a schema nothing destructures.** `Posting::of` covers
`Voucher`. Add a property to `Contact` and the library compiles clean; only a
struct literal elsewhere — in this repository, one in `api/tests/typed.rs` —
turns it into `error[E0063]: missing field`. A schema nobody constructs or
destructures by hand grows a field in silence.

**An operation the vendor withdraws, where nothing names it.** The wrapper
method goes with it, so Rust that calls `api.create_contact(..)` fails to
compile — but code that only *depends* on an operation without calling it needs
the `documented` assertion, and a subcommand nobody has written Rust against
simply stops existing.

**Prose, and a rule the vendor loosens.** A changed `summary` or `description`
reaches `--help` and nothing objects. A *tightened* rule — a narrower `pattern`,
a lower `maximum` — is enforced from the next bless step onwards, so a value
that stops being allowed is refused at the flag. A *loosened* one is the quiet
case: nothing was relying on the old rule, so nothing notices until a value the
old rule refused turns up and is accepted.

The exception is a rule a hand-owned type stands for. `Voucher.total`'s pattern
has `api/tests/money.rs` reading it, so widening that one is loud. Every other
rule in the document is on its own — which is an argument for writing such a
test wherever a rule matters, not for believing the loosening is caught.

**A vendor revision nobody fetches.** Every bless-time check above runs against
the vendor document that is committed here. `just blessed` re-runs the bless
step and fails on a non-empty `git diff`, so the committed artefacts cannot
drift from the committed document — but nothing fetches a *newer* document.
Until someone updates `spec/toy.yaml`, the committed document *is* the API as
far as this workspace is concerned.

The honest summary: drift in something you **use** is loud, drift in something
you **corrected** is loud, and drift in everything else is a diff someone has to
read.
