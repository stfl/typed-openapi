# typed-openapi/src — the library's modules

The invariants the library's modules keep. A path that starts with `src/` or
`tests/` is relative to `typed-openapi/`, and one that starts with `examples/`
is relative to the repository root. The rules that reach past this directory are
in the root `CLAUDE.md`, and the rules of the example are in
`examples/toy/CLAUDE.md`.

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
run included.

**A word that carries consent is never renamed around.** A gate's flag and the
confirmation are guarded in the subcommand's `Namespace`, so a parameter or body
field spelled like one refuses the load — `GateTakenByName`, `CommitTakenByName`
— rather than moving to `--body-<name>`. The document's names are the vendor's
and go on the wire; a confirmation answerable by a flag somebody typed for
another reason is not a confirmation. Each word moves on the side that owns it:
a gate in the adopter's `x-cli-gates`, the confirmation through
`Loading::commit` or `Settings::commit_word`. The five body flags in `TRANSPORT`
are not consent and still yield, to `--body-<name>`.

The confirmation is guarded on writes only, because only a write declares it,
and a gate on a read is refused while the document is reduced: a request sent on
sight has nothing for a gate to hold. The chosen word travels in the blob on the
document and on every operation, so a help page, the summary and a generated doc
comment all name the flag the binary accepts.

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
out, and they are read in the same place the rule runs. Whether a path
addresses one resource, which makes a `GET` a `get` and not a `list`, is read
from the segments below the group only: a template above the group names a
value the caller supplies for every operation under it.

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

**A document is read as the version it says, or refused by name.**
`Document::load` asks the overlaid document for its `openapi` before
`required::check` and before the model reads anything, and a version that is not
`3.0` or `3.0.x` is `LoadError::Version`, which prints it. 3.1 spells some rules
another way, and the model refuses those with no place, and it means others that
3.0 does not have, and the model drops those in silence: a `$ref` keeps its
sibling `maximum`, and `const` is a rule. Support for 3.1 is out of scope, so
the refusal names no repair. The version is the overlaid document's, the one
that is read, so an Overlay can state what an adopter holds a document to, where
a reviewer reads it. A document with no `openapi`, or with one that is not text,
has no version to name, and the model refuses it at the key. The bless step
asks the same question, because `Settings::write_to` loads the document through
`Document::load_with` before it generates anything.

**A document that leaves out what a request needs is refused with the Overlay
that adds it.** `NoServer` is a document with no `servers`: OpenAPI reads that
as `/`, which is relative to wherever the document was served from, and a
command line has no such place. `NoOperationId` names the method and the path of
an operation with no `operationId`, because every later refusal and every
generated wrapper calls the operation by it. A server URL with variables takes
the `default` of each, which OpenAPI requires for every variable; a variable
that `variables` does not declare stays in the URL, which is then no URL
(`ServerUrl`).

**A refusal names its operation and the place it stopped, and its repair only
where the repair is true.** A `LoadError` from one operation carries the
`operationId`; one from a path item carries the path (`PathItem`), and one from
an Overlay carries the index of the layer in the list `Document::load` was given
(`Layer`). A name that cannot be spelled is `Name` for a group or a command,
whose repair is `x-cli-group` and `x-cli-command`, and `GateName` for a gate,
whose repair is `x-cli-gates`, because the first repair mends nothing in a gate.
A path with no segment to name a group after, the root `/`, is named whole.
A path item that is a `$ref` is `PathItem`: the path and the reference, and no
`RefError`, because this crate follows no path item's `$ref` and never tried
this one, so it cannot say the reference does not resolve.
`Shape` is the OpenAPI model refusing the document, and the model's message has
no place for a fault in `paths`: `openapiv3` reads `paths` through a
deserialiser that drops the path inside it, so serde stops at `paths` for a
fault in the deepest operation. `model_of` in `src/openapi.rs` reads the
overlaid value once, and where serde stopped at `paths` it reads the value again
one path item at a time, in document order, and names the first item that fails
and the node inside it (`paths[/b].get.deprecated`). The re-read refuses only
what the model refuses, with the model's cause: it tries the keys that start
with `/` and no others, as `openapiv3` does, and reads each item as an
`openapiv3::PathItem`, which reads the item's keys in the order it lists them,
as the model does. Anywhere else serde's own place is kept. The bless step's
`NotOpenApi` calls the same function. A read that returns a `RefError` passes it
on through `under`, a body's multipart schema included: a schema that cannot be
followed is never a body with no parts. `ParameterRef` and `PathItem` are the
two variants that do not go through `under`: `ParameterRef` names the operation
that lists the parameter itself, and `PathItem` has no `RefError` to pass on.
`tests/refusals.rs` holds a case for each, from the input of the probe or the
review finding that found it.

**A document that requires a key nothing describes is refused, and the
exemptions are measured against the generator.** `required::check` in
`src/required.rs` reads the *overlaid* document, before the OpenAPI object model
normalises an inline body away, and names every node whose `required` lists a
name neither it nor anything it composes with declares. The reduced document is
the vendor's plus the adopter's Overlays — the one that has to be right — so a
name nothing describes is a vendor mistake the adopter repairs in an Overlay,
where the correction is a line a reviewer reads. What a miss costs is measured
rather than argued: typify writes `pub b: ::serde_json::Value`, required and
with no `serde(default)`, so the generated type refuses the vendor's own
responses.

A node is exempt only where the walk cannot tell whether a name is declared
*and* typify writes no such field either — a `$ref` leading out of the document,
and `not`/`if`/`then`/`else`. Permission is not a declaration:
`additionalProperties`, `patternProperties` and `unevaluatedProperties` exempt
nothing, and neither do `$dynamicRef` and a `dependentSchemas` subschema, which
hide a declaration but leave typify writing the untyped field all the same. The
`emission` tests in `src/required.rs` hold the list to that, shape by shape, and
they live there because typify is reachable only from inside the crate. An
exemption added without a row there is a reading of the specification rather
than a measurement, and a reading of the specification is what lets a shape the
generator still spoils onto the list.

Both conditions are necessary. Where the walk can tell whether a name is
declared, a phantom is refused although typify writes no field for it, and the
rows measure what a node holds and not where the node stands. SEV-249 measured
nine positions where typify writes no field: `unevaluatedProperties`, a
`dependentSchemas` subschema, `propertyNames`, `prefixItems`, `contains`,
`then`, `contentSchema`, `$defs`, and anywhere below `not`. At each of them the
walk reads the node and can tell, so a phantom there is refused, and no row
exempts a position. Eight of the nine are keywords that a 3.0 schema does not
have, and a document that is not 3.0 is refused first. The ninth, below `not`,
is rare, and its refusal names an Overlay.

The walk takes its nodes from a work list that shares the `followed` set, and
makes no call for a hop. A chain of `allOf` hops is as long as the vendor wrote
it: 7,000 of them aborted the load with a stack overflow when each hop was a
call. `schema::follow` is a loop for the same reason, so a reference chain adds
no recursion to either walk.

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

`spelled` refuses a name that holds a letter or a digit outside ASCII, and
transliterates nothing. `kebab` would turn that letter into a separator, so
`straße` would be the flag `--stra-e`, which is no word of the vendor's and which
two different names can share. A path segment that holds one needs
`x-cli-group` or `x-cli-command`, and `Name` says so; a gate or the confirmation
word is refused; a parameter is `Unreachable`, and only a required one refuses
the document; a body property sends the body whole. A mark outside ASCII that is
no letter and no digit separates words, as an ASCII one does.

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

**A request body is held to the generated type in one place, and an
undeclared property has no way through.** `client::fits` is where a JSON body
meets the type of its operation. The library cannot name that type, so the
adopter's seam calls it (`tree::select`, then the generated
`OperationId::check_body`), and a CLI that only calls `tree::dispatch` has none.
Each refusal is its own `BodyError` variant: is the body an object where the
type takes one (`NotAnObject`), do the values have the right types (`Mismatch`),
and does the type write back everything the body holds (`Undeclared` for a
property, `NotAnObject` for a list that serde read by place). A type that cannot
be written back is `Unwritable`, because there is nothing to hold the body
beside, and the gate fails closed. A new refusal is a new variant, since
`BodyError` is `#[non_exhaustive]`. `Undeclared` and `NotAnObject` print no value
of the body, which can be a credential or a client's data. The cause of a
`Mismatch` is serde's message, which can quote it. A path is spelled as
`serde_path_to_error` spells one (`address.extra`, `positions[1].flag`), and
`client::spelled` is the one place that spells a place, whether the walk found it
or the read did.

D4: no flag, setting or cargo feature lets an undeclared property through on a
call. The repair is an Overlay that declares it, which an adopter blesses and a
reviewer reads, and a second road round the check (an `--allow-undeclared`) is
what the rule is there to prevent. The generated types get no
`#[serde(deny_unknown_fields)]`: the same types decode a vendor's responses, and
a response may carry more than the document says. The rule lives in `fits`
because a request is the only side it is about.

**The measure is the generated type, found by writing a body back.** The typed
call is held to the same type, and anything that makes the command line and the
typed call disagree about an operation is a bug, so a body is refused where the
type reads less than the document says. `fits` reads the body into the type,
writes the value with `serde_json::to_value` and walks the body beside what came
back (`beside`): a body list beside a written object was read by place, and a
body key that the written object lacks was not read. That is the way to see
inside an untagged union, an internally tagged union, an adjacently tagged union
whose content comes before its tag, and a `flatten`ed struct (which is also what
typify writes for an `anyOf` it cannot judge exclusive). Serde reads each of them
from a buffer, and nothing is skipped from a buffer. Writing the body back is why
`fits` takes `T: Serialize`: a type must write the shape it reads, an object as
an object and a list as a list, and that holds for the type from
`Settings::replace` as well.

**A property that holds nothing is found by the read, and the place is the
read's.** A type drops a `null`, an empty list and an empty map when it writes an
optional property (`skip_serializing_if`, in typify-impl's `structs.rs`), so the
walk lets a key with such a value pass. `src/body_reader.rs` names it where the
type skips it while it reads. Serde asks its deserializer for
`deserialize_ignored_any` where a type has no field for a key, and `Reader` is
`serde_json`'s reading of a `Value` that knows where it is in the body and notes
the place when it is asked. Only the first is kept.

The place comes from the read and not from a search of the body. The key of an
externally tagged variant is a step of the body, and only the type knows that an
object of one key is a variant: a body can spell a variant like a property of its
content, and then it has two readings that nothing in the body tells apart.
`Reader` takes the key from the `deserialize_enum` that the type asks.

`Reader` is `serde_json`'s own except for the walk down. A scalar, a refusal and
the count of what a read leaves behind are asked of the `&Value` that
`serde_json` implements `Deserializer` for. A list or an object is read by
`serde_json` through a visitor that hands the visitor of the type an access that
knows the step of each value, so the keys of a map (an integer key parses as it
does there) and every message are `serde_json`'s. The name of a newtype reaches
`serde_json` too, which reads a `RawValue` through it when the `raw_value`
feature is on, and any crate of an adopter's build can turn that on. A reader
that differs from `serde_json` in a value or a message is a defect:
`src/body_reader.rs` holds a test that gives both a set of types and bodies and
compares them, `RawValue` among the types.

Serde skips nothing from a buffer, so the property passes inside an untagged
union, an internally tagged union, a `flatten`ed struct and an adjacently tagged
union whose content comes before its tag, and at the top when the body itself is
one of the first three. A struct, an externally tagged union and an adjacently
tagged union read their own keys directly, so a property that holds nothing is
refused there, and so is it in the content of an adjacently tagged union whose
tag comes first. That is a limit, `docs/validation.md` states it, and
`tests/shapes.rs` pins it on the types typify writes, and a unit test in
`src/client.rs` on a type of its own.

`Probe` asks the type what it reads for the body itself, because the body cannot
tell: serde derives `visit_seq` beside `visit_map` for a struct, so a list of
five values reads into a struct of five fields. The type is read from a
deserializer that holds nothing, and a call to `deserialize_struct` or
`deserialize_map` is an object, which is a struct, a map and an adjacently
tagged union. A type that reads itself through `deserialize_any` (an untagged or
an internally tagged union, a `serde_json::Value`), `deserialize_enum` (an
externally tagged union) or `deserialize_option` asks for none. A list or a
scalar at the top is `NotAnObject` at `.` whatever its length when the type asks
for an object. For a union that asks for none, a scalar at the top that no
member reads is a `Mismatch` (a union with a scalar member reads one), and a list
is `NotAnObject` when a member reads it by place and a `Mismatch` when none does.
Below the top the walk finds a list that serde read by place, and a list that
the type cannot read is a `Mismatch`, like a scalar, which serde cannot read an
object from.

`tests/shapes.rs` is the measurement of typify: it runs `typify::import_types!`
over `tests/fixtures/shapes.json` and holds `fits` to the types typify 0.8 writes
for each shape (a struct, an untagged, an internally tagged, an adjacently tagged
and an externally tagged enum, an `anyOf` that typify writes as an enum and one
that it writes as a struct of `flatten`ed `Option`s, an `allOf`, a map of objects
and each way to leave the rest open), so a release of typify that writes a shape
another way fails under the case that stood on it. The unit tests in
`src/client.rs` and `src/body_reader.rs` use hand-written types and hold the
mechanism. Two shapes are narrower in the type than in the document, and a body
is refused like any other: an `anyOf` that typify writes as an `untagged` enum,
given a body that fills two members, and a `oneOf` of an object and a list given
a list that the object reads. `docs/validation.md` says both to an adopter. The
struct of `flatten`ed `Option`s takes the properties of every member, but serde
reads a member that fails as absent, so a value of the wrong type for a member is
`Undeclared` there and not a `Mismatch`.

**`additionalProperties: true` is said as `{}` before typify reads the schema.**
typify writes no field for `true` beside declared properties, and the
`flatten`ed `extra` map for `{}`, which admits the same values and which its own
comment names as the way to get the map. The check is against the generated type,
so the first would make a property that the document leaves open `Undeclared`.
`OpenWithAMap` in `src/generate/types.rs` is a `schemars` visitor that says every
`true` as `{}`, at every depth, in the named schemas and in the inline ones. It
walks the schema and not the JSON, so a `default` or an `example` that holds the
same words is not changed. A schema that states no `additionalProperties` stays
closed: reading it as open would refuse nothing, and the refusal of an
undeclared property is the point. `false` stays what typify closes with
`deny_unknown_fields`.

**Every YAML text is read by one function, and no foreign error is public.**
`read` in `src/overlay.rs` is the function every document and every Overlay goes
through, into a `serde_json::Value`, before anything else looks at it. That
order is what tells `Syntax` (not readable as YAML or JSON) from `Shape` (YAML
of the wrong shape). The reader's options are in `reader`, which `read_here`
calls, and `read` calls `read_here`. The options are not the reader's defaults:
a default is a different document. On sevdesk-cli's committed overlaid file the
defaults read the enum value `N` as `false`. They read a vendor's
`x-cli-writes: yes` as `true`. So `yes`, `no`, `on`, `off`, `y` and `n` stay
strings, and `<<` is an ordinary key. Comments are not read: a value holds none,
and the default refuses a run of more than 32 between a key and its nested
value. The budget limits the value that a read builds: 2 500 000 nodes and
64 MiB of scalar text, counted with every alias expanded, at a depth of 128.
Every other cap is set where a document inside those limits cannot reach it,
with one exception: anchors nested inside each other keep a copy of their events
and of their owned text for each anchor open around them, so they reach the caps
on what anchors keep (10 000 000 events, 64 MiB) with a value inside the limits,
and the reader refuses them. A response defined once and used by a hundred
operations, and a value that occurs a million times, are documents working as
intended. The node limit is what refuses an alias bomb, because the reader
counts each event that an alias replays in it. `NODE_LIMIT` in `src/overlay.rs`
is the one place in code where the node limit is written, and the caps that
follow from it are written from it. The docs state the numbers too (`2 500 000`,
`2 500 001`, `10 000 000`, `64 MiB`, and `400`, the bomb's nodes over the node
limit, in the doc of `REFUSAL_WITHIN`), so a change to `NODE_LIMIT`,
`EVENT_LIMIT`, `TEXT_LIMIT` or the bomb's `BOMB_LEAVES` changes them as well:
grep for the numbers in `src/`, `tests/`, `docs/` and `CHANGELOG.md`. A cap that
a later release of the reader adds is a new way to refuse a document the base
read, so it is set behind the same limits and gets a test in `tests/yaml.rs`.
The emitter keeps its defaults, which quote what a YAML 1.1 reader would
misread. A public error holds an `overlay::Cause` (a message, and an
`overlay::Place`: the line and column of the text, or the action's index) or a
boxed `dyn Error + Send + Sync`, so a release of the YAML crate, `roas-overlay`
or typify is never a break here.

The depth limit is a recursion limit, and the library holds it on any thread. The
reader descends 128 levels before it refuses the 129th, which takes up to 3 MiB
of stack in a debug build, and a test thread has 2 MiB while a Windows main
thread has 1 MiB. A refusal that aborts the process is no refusal, so `read`
runs the reader on a scoped thread whose stack the crate sizes (`READER_STACK`),
and builds the `Cause` there, so no type of the reader crosses the thread. A
panic comes back on the caller's thread. Where the system refuses the thread,
`read` reads on the caller's, because a read does not fail for want of one.
`tests/yaml.rs` calls the reader from a 256 KiB thread, which is why it passes
only if the descent is off the caller's stack.

**`pub` means an adopter calls it.** What no caller outside the crate used is
`pub(crate)`: `schema`, `multipart`, `kebab`, `Grouping`, `Namespace`,
`Protected` and `Clash` in `src/names.rs`, and `scalar::Numeric`. The private
`schema` is what keeps "nothing from `openapiv3` escapes" in
`src/model/reduce.rs` true: it takes `openapiv3` types, so a public `schema`
would make them public. Nothing in the lint block stops a new `pub` item from
naming an `openapiv3` type, so review keeps the claim true for it. `overlay`
stays public because a bless step calls `overlay::parse`, and `overlay::Cause`
with it because every `OverlayError` variant carries one, and `overlay::Place`
because `Cause::place` returns it. `required::check` stays public with the
`PhantomKeys` it returns, and `scalar::Text` stays public because an adopter's
test names it. A type that a public variant carries is public too, which is why
`RefError` is re-exported at the root while the rest of `schema` is not.
`unreachable_pub` is a warning the gate denies, so a `pub` item the crate does
not export fails it.

**Every public item is documented, and no `allow` goes on an error.**
`#![warn(missing_docs)]` is in `src/lib.rs`, and the gate's `-D warnings` makes
it a refusal. `missing_docs` takes its level from the item that holds it, so an
`allow` of that lint on an error enum also exempts every variant and field in
it. A variant says when the crate raises it. A field says what the value is or
where it comes from: the `operationId`, the flag as the operator typed it, the
path into the body. It never says the `#[error]` message again, because the
message prints it already. A doc that says what a foreign cause's message holds
says only what a probe of every failure shows. Reading a `serde_json::Value`
into a type names a value and a type for some failures, and for others only a
missing key or an unmatched `untagged` enum. That read, `postcard` and `syn`
over tokens carry no position. The adapter rule, that an adapter must not turn a
status code into an error, is on `SyncClient` and `AsyncClient` themselves,
because an adopter writes an adapter from that page.

**A feature shows as a label, and no item writes it.** `src/lib.rs` turns
`--cfg docsrs` into `feature(doc_cfg)`, and rustdoc then labels every item
behind a `#[cfg(feature = ...)]` with "Available on crate feature ... only".
docs.rs passes the cfg through `[package.metadata.docs.rs]`, and `just check`
builds the docs with it, so the path docs.rs takes is in the gate. An item that
a feature adds carries its `cfg` and no `#[doc(cfg(...))]`, and its doc does
not say again what the label says ("Requires the `document` feature"); the
module declaration carries the `cfg` for the items inside it.

**The model layout number moves with the layout.** A blob opens with a stamp:
a magic of 8 bytes, `Document::MODEL_LAYOUT` as a `u32`, and the version of the
crate that wrote it as a length and UTF-8. `Document::from_blob` reads the stamp
before it decodes a byte, so a blob of another build is `Unstamped` or `Stale`
and a blob of this layout that does not decode is `Blob`. Any change to what
`postcard` writes for `Document` or a type inside it, such as a field added,
removed, reordered or retyped, a variant added, removed or moved, or the argument
of a generic, bumps `MODEL_LAYOUT` in the same change. Then
`TYPED_OPENAPI_WRITE_LAYOUT=1 cargo nextest run -p typed-openapi --test layout`
writes `tests/layout.json` again, and `just bless` writes the blob again.
`tests/layout.rs` fails on a layout that moved under the same number and says
so. It traces types, so no fixture can trip it. It lists every enum of the model
by hand, because the tracer reads one variant of an enum it meets and the
registry refuses to describe one it has not read whole; an enum added to the
model is added there. It traces `Bounds<f64>` apart from `Bounds<i64>`, because
the tracer names a container without its arguments, and `arguments` in the same
file stops compiling when `Scalar` changes one. The format of the stamp itself
never changes, and `tests/blob.rs` writes it out by hand to hold it. Write mode
of `tests/layout.rs` refuses while the layout differs from the description and
the number did not move, so the command it names is no way past the guard.

`Document::from_blob` checks a blob that decodes against the rules a load keeps,
because a binary starts from the blob and nothing else has read the document.
`Document::check` in `src/model/blob.rs` repeats the confirmation word and, for
each operation, its names, its effect, its gates, its flags and each `requires`,
and then the names two operations share. An operation is asked whole before the
next one, and the names two operations share after every operation. The error
names the operation, or both of two, except for the document's own confirmation
word, which has none. Each rule is asked in one place that the load and the check
share: `spends`, `gate_on_read`, `first_collision`, `Effect::of_method` and
`unmet` in `src/model.rs`, because `reduce` is behind the `document` feature and
`blob` is not, and `names::is_spelled`. `first_collision` makes one pass over the
operations, because the check runs at every start-up of a shipped binary and the
number of operations is the document's. `spends_command` is beside them and only
the check asks it: the load accepts a group or a command called `help`.
`check_requires` matches `Unmet` with no `_` arm, so a new way for a `requires`
to fail stops compiling until the decode names it.

The flags of an operation share one namespace: the confirmation word of a write,
the gates, the parameters that have a flag and the properties of a body, with
`help` and the five `TRANSPORT` words spent. `Namespace::guarding` and
`gates_of` in `src/model/reduce.rs` keep it at load and
`Operation::check_flags` keeps it on the way in, so a rule added to one is added
to the other. A read declares no confirmation word, so a parameter of a read may
be spelled like it.

The blob holds two facts of the document only in part, and the check cannot find
what the blob does not hold. It holds the effect and not `x-cli-writes`, so the
check finds a method that writes held as a read, and a read that names a gate
(`GatedRead`: a `GET` that the document made a write and gated), and cannot find
an ungated `GET` that the document made a write. It holds the gates and not
`x-cli-gates`, so it cannot find a gate that the document named and the blob
dropped; `Plan::decide` still asks for the confirmation of a write, so nothing
is sent on sight.

**The check repeats a rule of the load when a break of it is inside a line, and
then it repeats the whole rule.** A value in the blob can break a rule, and the
break does one of two things. The binary panics: clap does when it builds the
command tree in a debug build, or other code that reads the value, and in a
release build one declaration silently takes the place of another. Or a request
that the gate holds back is sent on sight. Those two are the damage the check
stands in front of, because a damaged blob must not make clap panic and the gate
is the first invariant of the crate. The line is drawn round a rule and not
round one break of it: where a blob can hold a break of a rule that is inside the
line, the check asks the load's own function, so it refuses every break of that
rule, also one that costs only the run. A second rule beside the load's, for the
breaks inside the line alone, would drift away from it, and the rule is asked
once for each name in any case. A rule of which no break is inside the line is
left out, because a break costs only the run that meets it: a `pattern` that no
engine runs refuses every value of its flag when the flag is parsed, and
repeating the rule would compile every pattern of every operation at each
start-up to run one subcommand. A refusal whose input the blob does not hold (the
document's text, a `$ref`, an `x-cli-` marker) cannot be broken from the blob at
all. Three rules are repeated whatever their breaks, because F-70, the finding
that asked for the check, names them: the confirmation word, which is the
validation of `Loading::commit`, the spelling of each flag, and each `requires`.
The spelling of a flag and the confirmation word of a write have breaks inside
the line (a word with a leading `-` makes a debug build panic), and no break of
the document's own confirmation word or of a `requires` is. A slice that adds a
refusal at load asks whether a value in the blob can break it, reads what a
binary does with the blob that does, and adds the check when the answer is
inside the line.

Postcard drops the message of a `serde` error, so a check inside a `try_from`
could say only that the blob is damaged, and a rule that compares two flags or two
operations cannot sit in one field. The check runs after the decode for both
reasons. `Document` deserialises its flags as plain strings, and a `Document`
read through `Deserialize` in another format is not held to the check; `Gate` and
`CommandName` keep their `try_from`, which refuses on every path. Bytes after
the encoding are `Trailing`.

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
