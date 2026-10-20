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

**A type the adopter owns is held to the document by a test, because nothing
else holds it.** A newtype the generator writes the whole of compiles the
document's `pattern` into its own `FromStr`, so it cannot drift. A wrapper
around `examples/toy/money` reads with *that crate's* `FromStr` and borrows
nothing — `examples/toy/api/tests/money.rs` reads the rule off the embedded
document and holds the amount to it in both directions, which is also why
`Money` counts cents in a `num-bigint` integer: the pattern admits an unbounded
run of digits, and a narrower count would refuse values the document allows.
Any future `replace` owes the same test. `Voucher.total` and `Voucher.currency`
are the two routes kept side by side so that neither loses its demonstration:
both are named schemas and both are newtypes, and what differs is where the
reading lives — inside the generated `FromStr` for `Currency`, inside the
adopter's crate for `Money`. `api` re-exports `money` as a module rather than
by item so that `api::Money` and `api::money::Money` keep their own names.
