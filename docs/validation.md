# What is checked, and what checking costs

Every rule the document states about a scalar value is enforced. The primer is
[the README](../README.md); this page is the reference for someone deciding how
much of their API's contract they get for free, and what the engine that
provides it costs their binary.

## The rules

A scalar is a value that fits on one flag and in one struct field.
[`Scalar`](../typed-openapi/src/scalar.rs) carries what the schema says about
one, and `Scalar::parse` checks all of it.

| the document says | value name | a refusal reads |
|---|---|---|
| `type: string` | `<STRING>` | — |
| `minLength: 3` | | `` `EU` is shorter than 3 characters `` |
| `maxLength: 3` | | `` `EURO` is longer than 3 characters `` |
| `pattern: ^[A-Z]+$` | | `` `x1` does not match ^[A-Z]+$ `` |
| `enum: [draft, paid]` | `<STRING>` | `` `void` is not one of draft, paid `` — and these complete |
| `type: integer` | `<INT>` | `` `5.0` is not an integer `` |
| `type: number` | `<NUMBER>` | `` `x` is not a number `` |
| `minimum: 1` | | `` `0` is not at least 1 `` |
| `minimum: 0`, `exclusiveMinimum: true` | | `` `0` is not more than 0 `` |
| `maximum: 100` | | `` `101` is not at most 100 `` |
| `maximum: 10`, `exclusiveMaximum: true` | | `` `10` is not less than 10 `` |
| `multipleOf: 5` | | `` `7` is not a multiple of 5 `` |
| `type: boolean` | `<BOOL>` | `` `yes` is not `true` or `false` `` |

Lengths count characters rather than bytes, so a three-character `€€€` passes a
`maxLength: 3`. A number JSON cannot carry — an infinity, a NaN — is *not a
number* rather than a `null` on the wire.

The same rules are the flag's help line, rendered by the same code:

```console
$ toy raw vouchers update --help
      --total <STRING>
          A decimal amount carried in a string. (matches ^-?[0-9]+(\.[0-9]{1,2})?$)
```

A rule that reaches `--help` and not the parser would be a promise the server
has to keep instead, so `Scalar::note` and the refusal are one rendering and
cannot come apart.

**No `format` is enforced.** A format names a rule; it is not one. `format: money`
tells a consumer that somebody somewhere knows what an amount is, and a
document that wants the rule enforced states it as `pattern`, which every
consumer of the document can run — see
[overlay.md](overlay.md#1-plain-corrections). No value is admitted or refused
for a format, and no format appears in a help line beside the rules above.

The tag itself travels: the reduction carries the `format` a value's schema
declares, beside the rules rather than among them, so that an adoption can ask
which of an operation's values are of a kind the document names. That is
[naming a kind](#naming-a-kind) at the foot of this page, and it changes
nothing here.

## `pattern`

`pattern` is [ECMA-262][ecma] regular-expression syntax, run on
[`regress`][regress] — the same engine [typify] puts inside a generated
newtype's `FromStr`. One engine on one set of bytes, so a value the command line
accepts is a value the generated type accepts by construction, rather than by a
hand-written rule somebody keeps in step.

**It is a search, not a whole-string match.** JSON Schema defines it that way:
`[0-9]+` matches `ab12cd`, and a document that wants the whole value to match
writes `^[0-9]+$`. Nothing is anchored for you — anchoring would refuse values
the document allows.

A `pattern` the engine cannot read refuses the document while it is reduced,
rather than refusing every value at a user's flag:

```
listVouchers: `since`: `[unterminated` is not a regular expression: Unbalanced bracket
```

## Where each rule runs

A parameter and a body field take different roads, and the difference is worth
knowing before you decide how much to put in the document.

| the value | checked by | when |
|---|---|---|
| a path, query or header parameter | `Scalar::parse`, in the flag's value parser and again in `Invocation::new` | the command line, and every typed call |
| a body field, from a per-field flag | `Scalar::parse`, in the flag's value parser | the command line |
| a body field, in a generated type | the type's own `FromStr` / `Deserialize` | every typed call, and every response |
| a whole body from `--json-body FILE` | nothing in this crate | — |

A parameter never passes through a generated type, so `Scalar` is the only thing
that can ever check one. A body field has both roads, and they run the same rule
for as long as the rule is one the *schema* states: the flag runs every rule
`Scalar` carries, and a `pattern` among them is the same expression on the same
engine the generated newtype's `FromStr` runs.

The roads part where a field's reading lives in a Rust type rather than in the
schema. A property whose `format` an adopter claims with `Settings::replace`, on
a schema that states no rule of its own, is read by that type on the typed road
and by nothing at all on the flag. `createLedgerEntry`'s `amount` is
`format: money` with no `pattern`: a body carrying `"12,50"` is refused by
`money::Money`, and `--amount 12,50` is accepted.
[`examples/toy/api/tests/inline.rs`](../examples/toy/api/tests/inline.rs) holds
that gap open in both directions so it cannot close unnoticed.

So a flag is worth relying on for what the document states about a value, and
for nothing a `format` only implies. To have the command line refuse a value,
put the rule in the schema: an Overlay pointing the property at a schema that
carries a `pattern` is the repair, and
[`examples/toy/spec/corrections.yaml`](../examples/toy/spec/corrections.yaml)
does exactly that for `Voucher.total`.

The other gap is the last row. `check_body` in
[`src/request.rs`](../typed-openapi/src/request.rs) asks whether a JSON body was
supplied where JSON is wanted and stops there — holding the *content* of a file
to a schema needs the generated `struct`, which only the adopter's crate can
name. [`tree::select`](cli.md#mounting-the-tree) is the seam for it, and
[`examples/toy/cli/src/raw.rs`](../examples/toy/cli/src/raw.rs) is that check in
nine lines.

Two things are not read at all: an `enum` on a numeric schema, and every
keyword that describes a shape no flag carries — `minItems`, `minProperties`
and the rest belong to bodies, which go through a file.

## What it costs

`regress` is a dependency in every feature set, because a shipped binary
enforces `pattern` and so does a generated newtype. Measured on this
repository's `examples/toy`, sequentially on an idle machine, three repetitions
per cell, median reported; the second column is the same example built with no
regex engine linked at all.

| | enforced | not enforced | the engine |
|---|---|---|---|
| the stripped `toy` binary | 4 604 624 B | 3 794 048 B | **+810 576 B (+21%)** |
| clean `cargo build -p cli --release` | 10.23 s | 9.93 s | +0.30 s (+3%) |
| crates in `cli`'s normal dependency graph | 65 | 64 | +`regress` |

**The cost is the binary.** A fifth of the `toy` binary is a regular-expression
engine, and there is no feature that removes it: a rule that is optional is a
rule a caller cannot rely on, and the point of reading the rule off the document
is that both consumers get the same one. `memchr`, `regress`'s only dependency,
is already in the graph through `serde_json`.

It also sets the floor: `regress` uses let-chains, so this crate wants **Rust
1.88** in every feature set.

Reproduce the first row with:

```sh
cargo build -p cli --release && strip -o toy.stripped target/release/toy && stat -c%s toy.stripped
```

## Owning the rule yourself

If the document cannot state your rule — it is not expressible as JSON Schema,
or it is your client's and not the API's — the route is
[a `format` tag and `Settings::replace`](overlay.md#owning-the-type-yourself),
which hands the value to a Rust type you write.

It saves you nothing on this page. `regress` is linked either way, and a type
that enforces its own rule is a type nothing holds to the document unless you
write the test that does. Keep the `pattern` beside the `format` and you get
both: the command line runs the document's rule, and Rust gets the type the
document could not describe.

## Naming a kind

Some rules no JSON Schema states. *A booking day must not fall in a closed
accounting period* is one: a `pattern` can say what a day looks like and cannot
say which days are closed, because the answer is not in the document and
changes every month.

What the document *can* do is say which values are days. Tag the schema with a
`format` beside the `pattern`, and the reduction carries the tag:

```yaml
components:
  schemas:
    LedgerDay:
      type: string
      format: ledger-day
      pattern: '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
```

```rust,ignore
use typed_openapi::Carrier;

for carrier in operation.carrying("ledger-day") {
    let (name, sent_as) = match carrier {
        Carrier::Param(param) => (param.name(), "a path, query or header value"),
        Carrier::Field(field) => (field.name(), "a property of the JSON body"),
    };
    println!("`{name}` is a day, sent as {sent_as}");
}
```

`Operation::carrying` reads the reduced model, so a shipped binary answers with
no document, no reader and no second pass; `Param::format` and `Field::format`
are the same fact one value at a time. A guard written over the answer is
general — it covers the field the vendor adds next revision, because the
document is what names it — where a compiled-in list of field names protects
whatever it was written against and nothing else.

The kind is read off the schema that *states* it, so a dozen properties
pointing a `$ref` at `LedgerDay` are a dozen days, and a wrapper around that
reference is one too. That is the same node
[`Settings::replace`](overlay.md#owning-the-type-yourself) keys on, so the Rust
type a kind stands for and the values a command line calls that kind come out
of one vocabulary rather than two.

**The limits are worth knowing before you write the guard.**

- Nothing is enforced. This crate carries the name and never acts on it; what a
  kind *means* is yours, and a rule this crate could run would have been a
  `pattern`.
- A parameter with no command-line spelling names no kind. It carries no value
  at all — no flag, no argument, no place in the request — so there is nothing
  for a kind to be about, and the
  [subcommand's long help says why](cli.md#parameters-with-no-flag) it has no
  flag.
- **A body that goes out whole has no fields, so its kinds are unreachable.**
  One nested property sends the body through `--json-body`
  ([above](#where-each-rule-runs)), and then there is no field anywhere to carry
  what its properties declare. A guard over such a body is one you write over
  the JSON yourself.
- A list parameter's kind is its *items'*, which is where a list of days says
  what it holds.

[ecma]: https://tc39.es/ecma262/multipage/text-processing.html#sec-regexp-regular-expression-objects
[regress]: https://docs.rs/regress
[typify]: https://docs.rs/typify
