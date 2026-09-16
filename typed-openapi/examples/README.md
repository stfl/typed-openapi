# Examples

Run one with `cargo run --features document --example <name>`.

| Example | Question it answers |
|---|---|
| [`clashing-words`](clashing-words.rs) | A document declares a property called `enshrine` or `commit`. Which flag moves? |

## `clashing-words`

A command line spends most of its flags carrying a request's data and two kinds
on a person saying yes: the **confirmation**, and each **gate** an operation
names. The document owns the first set. This crate and its adopter own the
second.

A vendor schema with a property called `enshrine` or `commit` puts the two sets
on the same word. `typed-openapi` refuses the document rather than renaming the
property, because the property name is what goes on the wire, the vendor chose
it, and a confirmation that can be answered by a flag somebody typed for an
unrelated reason is not a confirmation.

Each word moves on the side that owns it: the gate in the adopter's
`x-cli-gates`, the confirmation in the call that loads the document or
generates from it.

```console
$ cargo run --features document --example clashing-words
=== 1. The gate and a property both want `--enshrine` ===
enshrineVoucher: the property `enshrine` and the gate `enshrine` both want `--enshrine`; rename the gate in `x-cli-gates`, to `gate-enshrine` or another word

=== 2. The gate has moved; the confirmation still wants `--commit` ===
enshrineVoucher: the property `commit` and the confirmation both want `--commit`; confirm with another word — `Loading::commit`, or `Settings::commit_word` where a bless step generates

=== 3. Both words moved, each on the side that owns it ===
the confirmation is `--yes`
the gate is `--gate-enshrine`

and the document's own names kept their flags:
  --id
  --enshrine
  --commit
  --note
  --json-body
  --yes
  --gate-enshrine
```

Both refusals name the file or the call to change, because the person reading
one is the person who can fix it.

### Moving the gate

The gate's spelling is the adopter's, stated in the correction layer:

```yaml
overlay: 1.1.0
info: { title: cli, version: "1" }
actions:
  - target: $.paths['/vouchers/{id}/enshrine'].post
    update:
      x-cli-gates: [gate-enshrine]
```

### Moving the confirmation

The confirmation is never in the document, so it moves where the document is
loaded — or, in a bless step, where it is generated:

```rust
use typed_openapi::{Document, Loading};

let doc = Document::load_with(document, &overlays, &Loading::new().commit("yes")?)?;
```

```rust
use typed_openapi::generate::Settings;

let settings = Settings::new("vendor.yaml").commit_word("yes")?;
```

The chosen word travels in the reduced model, so a shipped binary reads it
rather than deriving one: the flag a subcommand declares and the flag its own
generated help names are the same string.

### What moves aside quietly

The flags carrying a body — `--json-body`, `--json-body-template`, `--raw-body`,
`--file`, `--field` — are not consent, so a document name that wants one of them
takes `--body-<name>` instead and says so in its help line. A `--body-json-body`
is ugly and harmless; a confirmation nobody typed is neither.
