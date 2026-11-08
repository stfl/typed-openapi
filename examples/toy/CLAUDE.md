# examples/toy — the worked adoption

The invariants the example keeps. The rules of the library are in
`typed-openapi/src/CLAUDE.md`, and the rules that reach past this directory are
in the root `CLAUDE.md`.

**The typed half of an adoption links no argument parser.** `api` and
`api-generated` take the library with `default-features = false`.
`just clap-free` is the check.

**Generated files are never edited.** `examples/toy/api-generated/src/{types,ops}.rs`,
`src/model.postcard`, `src/summary.md` and `spec/toy.overlaid.yaml` are written
by `just bless`.
A correction belongs in `examples/toy/spec/corrections.yaml` or, where only a
command line cares, `examples/toy/spec/cli.yaml`. `just bless` must
leave `git diff` empty when nothing upstream has changed — a non-empty diff
means either the vendor moved or the generator did.

**An owned replacement is held to the document both ways, or guarded so it
is never reachable as a flag.** `api/tests/money.rs` holds `money::Money` to
its pattern in both directions. Its arbitrary-precision count accepts the
unbounded digits the document allows. `api/tests/cents.rs` uses
`Operation::carrying` to guard that the narrower `money::Cents` never reaches
a parameter flag or a flat-body field flag. `Contact` has a nested address,
so its body goes through `--json-body` and the owned type. A response can
contain a value the document allows and this narrower type refuses; decoding
then fails. The adopter accepts that cost when choosing the type, as
sevdesk-cli does with `Amount`.

The toy keeps three routes side by side: `Voucher.total` is owned and held
both ways, `Voucher.currency` is generated, and `Contact.creditLimit` is
owned and guarded. `api` re-exports `money` as a module so the generated
schema wrappers and their owned values keep distinct names.
