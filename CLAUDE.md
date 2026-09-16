# Working on typed-openapi

`typed-openapi/` is the published crate. `examples/toy/` is one worked adoption
of it, five crates, all `publish = false`, built and tested with the rest so
that the example cannot rot.

Run `just gate` before saying anything is done. It is `check` (fmt, compile,
clippy, rustdoc — each with warnings denied), `features` (every feature
combination a consumer can select, plus the clap-free assertion), `test`
(nextest and the doctests, which nextest cannot run), `blessed` (the generator
still reproduces every committed artefact) and `package` (the crate built from
its own tarball). CI runs the same recipes, not a copy of them.

## Invariants a change must keep true

**The gate is default-closed and derived from the document.** `Plan::decide` in
`src/plan.rs` is the only place that decides whether a request is sent. A write
without confirmation is a `DryRun` carrying the exact request a confirmed run
would send — the same value, so the two cannot drift. Whether an operation
writes comes from the HTTP method plus the document's `x-cli-writes` marker,
never from a name or a heuristic.

An operation may name further gates in `x-cli-gates`, and they are demanded
*in addition to* the confirmation: `Plan::Send` wants the commit and every gate
answered, so adding a gate can only hold a request back. Each is a `required`
flag, which is the point — the hazard is named before the request is built, dry
run included. Their flags are reserved in the subcommand's `Namespace` before
any parameter or field claims one, so a body field spelled like a gate moves
aside rather than shadowing it. A gate on a read is refused while the document
is reduced: a request sent on sight has nothing for a gate to hold.

`tree::gates` is the one place a gate becomes a flag, and `tree::command` goes
through it, so a hand-written verb and a generated subcommand cannot spell one
gate two ways. `tree::answers` reads a flag the command never declared as
unanswered rather than panicking, which is what makes the reading safe to point
at a command this crate did not build.

**Both command names are decided while the document is reduced.** `Grouping` in
`src/names.rs` reads the rule off every path once; an `Operation` carries the
group and the command it was placed under, and both travel in the postcard blob.
A shipped binary reads them and derives nothing. Two operations reducing to one
`<group> <command>` is a `LoadError` naming both `operationId`s — never a silent
rename, because a name that moves when a *second* operation arrives is a name
that moved without anyone asking. `x-cli-group` and `x-cli-command` are the way
out, and they are read in the same place the rule runs.

**Corrections are an ordered list of Overlays, and the library reads nothing
into it.** `Settings::overlay` and `Document::load` take layers and apply them
in order. What an adoption puts in which layer is a convention `docs/overlay.md`
recommends and the example keeps — there is no enum, no schema and no naming
rule in the library. `examples/toy/api/tests/corrections.rs` is what holds the
example to its own convention.

**Every rule the document states about a value is enforced, in one place.**
`Scalar` carries what the schema said — `pattern`, `minLength`/`maxLength`, the
bounds, `multipleOf` — and `Scalar::parse` is the only thing that checks any of
them. A rule that reaches `--help` and not the parser is the defect this
invariant exists to prevent, so `Scalar::note` and the refusal are one
rendering. `pattern` runs on `regress`, the engine typify puts inside a
generated newtype's `FromStr`, so a value the command line accepts is a value
the generated type accepts by construction. No `format` is special-cased: a
format names a rule and is not one, and a document that wants a rule states it
in JSON Schema everything can read.

`Scalar` reads the document, and that is deliberate rather than incidental.
progenitor decides the same question the other way — `cli.rs` asks typify
`prop_type.has_impl(TypeSpaceImpl::FromStr)`, letting the *generated type*
settle whether a field is a flag — and it is the better instinct in general,
the one this crate follows for a schema's Rust name. It does not transfer here.
A command line is decided while the document is reduced and travels in the
postcard blob, so a shipped binary derives nothing and the generated types are
not in the picture at all; `has_impl` is answerable only from a `TypeSpace` at
bless time. Taking it would tie the command line to the generated types, which
this crate keeps apart on purpose, and it would still not supply the `pattern`
and the bounds `Scalar::parse` enforces.

**A declared `format` travels beside the rules and is inert.** `Shape::Flag` and
`Field` carry the `format` the value's schema declares, in the document's own
spelling; `Operation::carrying` asks which values of an operation are of a kind,
and `Carrier` says which half of the request each is in. It is deliberately
*not* on `Scalar`: everything a `Scalar` carries is a
rule `Scalar::parse` enforces and `Scalar::note` renders, so a format placed
there would be a rule by the back door — the defect the invariant above exists
to prevent. Where there is no value there is no kind, which is why an
`Unreachable` parameter and a `JsonWhole` body report none, and why the format
is read off the same schema the `Scalar` was read off — the items' for a list.
`schema::format_of` reads it off `stated()`, which is what `Settings::replace`
keys on too, so the generated types and the reduced model name one vocabulary.
Unlike `description_of`, the named schema wins over the field: a sentence is
about the field, a kind is about the value.

**`Operation::carrying` is the whole question; `Body::fields` and
`Param::format` are its parts, and all three are public.** `Body::fields` is the
door to a flat body's properties for anything walking one — a guard, a renderer,
documentation — and answers empty for every other body, so a walk is complete
with no match over `Body` and covers a variant added later. Keeping it private
would force `carrying` to be the only route. `carrying` stays beside it
because chaining the two halves by hand is not the cost: *omitting* one is, and
a guard over the parameters alone passes on every body field it was written to
cover without reading short. `tests/document.rs` holds the whole to the parts so
that a `carrying` which stopped reading one half is named rather than merely
answering. `Carrier` is what makes one list out of two — both halves name
themselves with a `&str`, and `Values` keeps parameters and body apart, so the
name alone does not say where the value goes.

**A shape the reduction cannot read is refused by name, never approximated.** A
`pattern` no engine runs and a `content` key that is not a media type are both
the document saying something this crate has no reading for, so both are a
`LoadError` naming the operation and the thing that could not be read — guessing
what the vendor meant is a correction, and a correction is the adopter's to
write in an Overlay where a reviewer can see it.

**A shape this crate cannot spell stops at the operation, not at the document.**
A parameter that is `in: cookie`, described by `content`, neither a value nor a
list of values, declaring a `style` this crate does not serialise, or *named* in
a way `names::spelled` refuses, is carried as `Shape::Unreachable` — named on
the subcommand's long help and in the wrapper's documentation, and given no
flag, no argument and no place in the request — and only a *required* one is a
`LoadError`, because an operation that could never build a correct request is
worth refusing by name. It is the rule a body already follows, where one nested
property sends the whole body through `--json-body` rather than refusing the
document that holds it.

Every flag a user types passes `names::spelled`, which is why a body property
spelled `*` sends its body whole rather than claiming a flag called `""`. The
body's rule turns on whether a property has a flag and never on why it has none,
so an unspellable property and a nested one reach the same answer — and the
`Body::JsonWhole` template is then the one place the key is written down.

**A body template is read from the model and reaches no request.** The body with
no per-field flags is the body `--help` cannot describe, so `Body::JsonWhole`
carries the skeleton `schema::template` rendered while the document was reduced
— text, because postcard is not self-describing and a `serde_json::Value`
deserialises through `deserialize_any`, which postcard answers with
`WontImplement`. `tree::select` answers `--json-body-template` from that field
and returns `Asked::Template` before any value is read, so the template route
builds no request: `Plan::decide` stays the only place deciding whether one is
sent, because this way round there is nothing for it to decide about. `tree::apart`
adds the flag last and sets it against the ids the subcommand declares: it
conflicts with all of them, and each one the subcommand demands is
`required_unless_present` the template instead — asking for a shape is not the
hazard the confirmation or a gate stands in front of, and it is not a second
command either. Those ids are read back off the `Command`, never written out: a
conflict clap cannot match is a conflict clap says nothing about, so a list
naming an id that has since been renamed compiles, runs, and quietly stops
refusing the flag it was written for. `exclusive` is the other way to say it and
is not this crate's to use — it reaches every argument the parse saw, including
the globals an adopter mounts above the tree, and only those typed after the
subcommand, so one spelling of a line would work and the other would not.
`JSON_BODY_TEMPLATE` is in `RESERVED` for the same reason every other body flag
is. `Asked` and `Outcome::Template` are arms rather than a flag left
lying about: an adopter cannot declare the flag and fail to print it, which is
the defect progenitor's `cli.rs` ships.

**One request builder serves both consumers.** A CLI reaches it through `tree`,
a generated wrapper through `Values` directly. Anything that makes the command
line and the typed call disagree about an operation is a bug, not a feature.

**A feature adds and removes whole items; it never changes one.** No type may
gain a variant or a field with a feature. This is why `LoadError` is separate
from `DocumentError` and why `Plan` lives in `plan` rather than in the
clap-gated `tree`. A caller who matches an error exhaustively must write the
same match in every build.

**The crate depends on no HTTP client, in any feature combination.** The seam is
`SyncClient` / `AsyncClient` over `http::Request<Vec<u8>>`. Adapters live in
`examples/toy/cli/src/client.rs` to be copied, not depended on. An adapter must
not turn a status code into an error: the body that came with a 4xx is what a
caller needs.

**The typed half of an adoption links no argument parser.** `api` and
`api-generated` take the library with `default-features = false`.
`just clap-free` is the check.

**A count about the document is derived from it, never written down.**
`Document::summary` in `src/summary.rs` answers every count an adoption is
tempted to keep by hand — operations, groups, reads and writes, what stands
behind each named gate, bodiless operations, bodies with no per-field flags,
parameters carried without a flag — off the operations already in the reduced
model. Nothing is stored: a stored count and the model it counts can disagree,
which is the whole defect this exists to retire, and deriving also keeps the
postcard blob the size it is. The type is feature-free, because a shipped binary
that loaded a blob has to answer the question a bless step answered.

`Summary`'s `Display` is the page a bless step renders, and the counts a test
asserts against are the accessors on the same value — so the sentence a reader
is shown and the number a test pins cannot drift apart. The page carries its
provenance from `Settings::summary_header`, beside the headers the other
generated files get: the command that counts it again and the documents it was
counted from, because a count a reader cannot re-take is a claim rather than a
measurement, and the two look identical on the page.
`examples/toy/api/tests/summary.rs` is the demonstration that the shape is
right — it builds the toy README's own sentences out of the summary — and any
count this crate adds owes a row there.

**The value is derived for everyone; the file is asked for.** `Settings::write_to`
writes four artefacts and answers with what it wrote;
`Settings::summary_page(path)` adds the page, and the answer grows by exactly
that path. The page is Markdown an adopter commits, reviews and lists in their
own bless check, in prose they cannot word — a whole obligation, so only an
adopter takes it on, and one that quotes no count is handed no file. Its path is
theirs for the reason the other four's is not: those name each other by relative
path, and nothing the generator writes reads the page. `examples/toy` opts in
because it quotes counts, which is also what keeps the claim from rotting.

**Generated files are never edited.** `examples/toy/api-generated/src/{types,ops}.rs`,
`src/model.postcard`, `src/summary.md` and `spec/toy.overlaid.yaml` are written
by `just bless`.
A correction belongs in `examples/toy/spec/corrections.yaml` or, where only a
command line cares, `examples/toy/spec/cli.yaml`. `just bless` must
leave `git diff` empty when nothing upstream has changed — a non-empty diff
means either the vendor moved or the generator did.

**A named schema carrying a replaced format is a newtype over the adopter's
type, whatever it is called.** typify decides it the other way — it declines to
wrap where the definition's name is what the replacement path ends in — so
`generate/types.rs` does not ask it that question: every schema declaring the
format is rewritten into a reference to a placeholder definition, and the
placeholder is the adopter's type through `with_replacement`. A definition that
is a bare reference is a newtype to typify, so the rule holds for `Cents`
against `money::Cents` exactly as for `Amount`, and typify still names the
wrapper — which is what keeps `names.rs` the one place a name is decided.
Rewriting the emitted file instead would mean knowing which generated field came
from which property, and that is typify's naming rule copied. A schema that
reduces to the placeholder's own Rust name is a `GenerateError::Reserved`
naming it.

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

**What an owned type is made of never reaches the library.** `num-bigint` is
declared in `examples/toy/money` and in no other manifest, and `just
bigint-free` — inside `features`, beside `bon-free` and `clap-free` — asserts it
is absent from `cargo tree -p typed-openapi --all-features`. `Settings::replace`
exists precisely so that the crate does not have to know what a replaced type
is built from.

**The library never reads the example's files.** `typed-openapi/tests/fixtures/`
holds this crate's own copies of the toy document and both Overlay layers. They
have the same content as `examples/toy/spec/` and a different owner: there the
document is the vendor's and the example is free to evolve it. Pointing the
library's tests at that copy would let a change to the example break the
library, and the fixtures would stop shipping in the published tarball.

## Style

The lint block in the root `Cargo.toml` is from the first commit so that no
code is written against a looser setting and grandfathered in. `unsafe_code` is
**forbidden**, not denied. `clippy.toml` tunes the size lints; a hit is a
cleanup item, and `#[expect]` needs a reason a reviewer would accept.

Doc comments are prose that says *why*, in present tense, describing the system
as it is. Never "now", "no longer", "previously", "instead of" — a reader
arriving today never saw the earlier version. The diff belongs in the commit
message.

Route a document by who reads it: `typed-openapi/README.md` is the primer for
someone deciding whether to use the crate, `docs/*.md` is reference for someone
looking one thing up, this file is for an agent changing the code.

## Toolchain

`rust-toolchain.toml` pins nightly because four of `rustfmt.toml`'s options are
nightly-only and the coverage recipe wants branch instrumentation. **The library
itself is stable** — CI's `stable` job builds and tests it there, and nothing in
`typed-openapi/src/` may need a nightly feature.

## Publishing

A tag `v<version>` on a commit CI has passed triggers `.github/workflows/release.yml`,
which checks the tag against the manifest and publishes through crates.io
Trusted Publishing. The first version has to be published by hand before Trusted
Publishing can be configured for the crate.
