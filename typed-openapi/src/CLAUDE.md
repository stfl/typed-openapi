# typed-openapi/src — the library's modules

The invariants the library's modules keep. A path that starts with `src/` or
`tests/` is relative to `typed-openapi/`, and one that starts with `examples/`
is relative to the repository root. The rules that reach past this directory are
in the root `CLAUDE.md`, and the rules of the example are in
`examples/toy/CLAUDE.md`.

**The gate is default-closed and derived from the document.** `Plan::decide` in
`src/plan.rs` is the only place that decides whether a request is sent. A write
without confirmation is a `DryRun` carrying the exact request a confirmed run
would send — the same value, so the two cannot drift. `Plan::decide` takes the
`Invocation`, which holds its operation, and builds the request from it, so the
verdict and the request are about one operation by construction: no signature
takes an operation and a request side by side, and a read's verdict cannot be
put on a write's request. A typed call reaches it through `Call::plan`, whose
`Planned` holds the plan and is the only thing that sends it (`Planned::send`),
so the request a caller printed is the request that goes out and a dry run
cannot be sent. Whether an operation writes comes from the HTTP method plus the
document's `x-cli-writes` marker, never from a name or a heuristic.

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
`Loading::commit` or `Settings::commit_word`. `help` and the five body flags are
not consent and still yield, to `--param-<name>` or `--body-<name>`. They are
the six words of `SPENT` in `src/model.rs`, which the load puts in every
`Namespace` and `spends` asks of a decoded blob, and a gate or the confirmation
that wears one of the six is refused (`ReservedGate`,
`ConfirmationError::Taken`): `--help` is typed to read a page, and a word that
`--help` answers is a write that somebody sent by asking for help. A group or a
command called `help` is refused for the same collision with clap's own
subcommand (`LoadError::NameSpent`), and `x-cli-group` or `x-cli-command` is the
repair.

The confirmation is guarded on writes only, because only a write declares it,
and a gate on a read is refused while the document is reduced: a request sent on
sight has nothing for a gate to hold. The chosen word travels in the blob on the
document and on every operation, so a help page, the summary and a generated doc
comment all name the flag the binary accepts.

`tree::gates` is the one place a gate becomes a flag, and `tree::confirmation`
the one place the confirmation does, in the word the operation carries.
`tree::command` goes through both, so a hand-written verb and a generated
subcommand cannot spell one gate, or the confirmation, two ways, and a verb
spells neither word itself. A read declares no confirmation, so
`tree::confirmation` hands its command back as it was given. `tree::answers`
reads a flag the command never declared as unanswered rather than panicking,
which is what makes the reading safe to point at a command this crate did not
build. `tree::values` reads every flag the same way, with `try_get_*`: a flag
the command never declared was not given, a flag stored as another type is
`ArgError::FlagType`, which names it, and a field's flag that the field's rule
refuses is `ArgError::FieldValue`. A command of the adopter's own can declare a
flag with any parser, and nothing after `values` holds a body field to its rule.

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

**This crate owns the `x-cli-` prefix, and a marker is read where it stands or
the load is refused.** `src/markers.rs` holds the five keys, what each is read
off, and the scan that `Document::load_with` runs on the overlaid document
before the object model reads it, because the model drops a key beside a `$ref`
without a word. The four on an operation are read on a method of a path item
under `paths`, and `x-cli-requires` on a parameter that is not a `$ref`: a path
item's, an operation's, or one under `components.parameters`, which is a place
the key is read whether or not the `$ref` of an operation names it. An `x-cli-`
key that nothing reads is `UnknownMarker`, one that stands where nothing reads
it is `MisplacedMarker`, and each names the key and the node that holds it,
spelled as an Overlay names a target. A new key joins `READ` with the subject it
is read off, and the wording of both refusals follows from that table. The value
of a marker is judged where it is read (`named`, `listed`, `requires_of` and
`effect_of` in `src/model/reduce.rs`), and `x-cli-writes` is the boolean `true`
or it is `Writes`: the marker adds a write and never removes one, so `false`
says something it cannot do.

**The scan enters what OpenAPI 3.0.3 defines, and the table it reads is
shared.** `src/shape.rs` says which key of which kind of object leads to another
object, to a list of them or to a map of them, and what the keys of a map are.
The scan enters a value only where the table leads and 3.0.3 defines the route,
and reads nothing under any other key: a field of type Any (`example`,
`default`, `enum`, the `value` of an Example Object, the `parameters` values and
the `requestBody` of a Link Object), the value of an extension, and a key that
3.0.3 does not define. The last is one class: a field of a later version
(`webhooks`, `components.pathItems`), a keyword of a later draft (`$defs`, `if`,
`dependencies`), and a key that no version defines (a key of `paths` that does
not start with `/`, and a key of a Responses Object that is neither `default`
nor a status code that `openapiv3::StatusCode` reads). The model drops each of
them and nothing builds a request or a flag from what stands there. A scan that
entered them would read a 3.0 document as 3.1, and refuse a document for data
that nobody reads. An `x-…` key of an object it enters is an extension. A
property, a component, a media type and a response header may be spelled
`x-cli-writes`, so the keys of a map of names somebody chose are names and are
not read; the Paths Object and the Responses Object are the two maps whose `x-…`
keys are extensions.

A route is one that 3.0.3 defines or one that only the schema walk enters
(`Reach::Defined` and `Reach::Whole`), and each has one place in the table. A
map added for a route that 3.0.3 defines is added for all three walks. The
schema walk also enters the rest, because `required::check` judges a schema by
what it holds and not by where it stands: wherever it can tell that a name is
declared nowhere it refuses the phantom, although typify writes no type there
(F-81), and the refusal names an Overlay. A marker is judged by where it stands,
so the scan stops at what the model reads. So does the walk of `src/patterns.rs`,
because a generated type is made of nothing that the model drops. A route that
the table gains for the schema walk alone gets a row in `tests/required.rs` and
one in the marker test of the keys that 3.0.3 does not define.

An object of a kind that can be a reference, and that holds a string `$ref`, is
to the model the reference alone, and OpenAPI 3.0.3 says of the Reference Object
that "any properties added SHALL be ignored". `Kind::is_read_as_reference`
answers it for the kinds the model reads as a `ReferenceOr`, and the walk of
`src/patterns.rs` judges nothing in such an object and enters none of its other
keys. A `pattern` beside a `$ref` there is not a `pattern` of the document, and
refusing it would refuse a document for data that nobody reads. In an object
that cannot be a reference (a media type, an operation, the path item of a
callback, an encoding) a `$ref` is a key like any other, and the schema beside
it is read. A callback is a reference under `components.callbacks` only, because
the model refuses a callback of an operation that is a `$ref`
(`LoadError::Shape`), so the kind needs no second answer.

A list of what is entered fails toward a marker missed, which the docs accept. A
list of what is not entered fails toward a document wrongly refused at each
field that nobody put on it, which they forbid. So an object that the table
leaves out and the specification gives fixed fields is a marker missed, and a
kind added to the table is added with a row in the tests of `src/markers.rs`.
`required::check` judges only `Kind::Schema`, so a kind that holds no schema
changes none of its answers.

**Corrections are an ordered list of Overlays, and the library reads nothing
into it.** `Settings::overlay` and `Document::load` take layers and apply them
in order. What an adoption puts in which layer is a convention `docs/overlay.md`
recommends and the example keeps — there is no enum, no schema and no naming
rule in the library. `examples/toy/api/tests/corrections.rs` is what holds the
example to its own convention.

**Every rule the document states about a value is enforced, in one place.**
`Scalar` carries what the schema said — `pattern`, `minLength`/`maxLength`, the
bounds, `multipleOf`, and an `enum` of strings, numbers or booleans — and
`Scalar::parse` is the only thing that checks any of them. `Scalar::wire` is the
text of a parameter that goes on the wire, and it asks `parse`'s own reading of
a number, so there is no second check: the number that is checked is the number
that is sent, and `Invocation::new` keeps the text. An `f64` reads the nearest
float of its digits, so a number is refused when that float is another number
(`exact` compares decimal numbers, not text), and an integer past `i64` is out of
range and not "not an integer". A numeric `enum` admits a value and not a
spelling (`+200` for `200`), and a boolean `enum` of both values is `None`,
because it states what `type: boolean` states.

A rule that reaches `--help` and not the parser is the defect this invariant
exists to prevent, so a rule has one note, the words that finish "is not …", and
its refusal is those words after "is not" (`ScalarError::refusal`). A refusal
that is worded on its own is a second rendering that can drift from the first. A
string `enum` is the one rule with no note, because the flag lists its values and
clap's parser enforces them as `parse` does. `ScalarError::Choice` is every
`enum`'s refusal, and it and the note spell the list once (`one_of`).

`multipleOf` is decided on digits and never in `f64`: the value is the text
`exact` accepted and the step the shortest text that reads back as the float,
both as `Decimal`, and the quotient is whole when the digits of the step divide
the digits of the value once the powers of ten are lined up
(`Decimal::is_multiple_of`). A float quotient refuses `0.07` against `0.01` and
admits `1e17` against `3`. An integer's remainder is `checked_rem`, so no value
makes `parse` panic, whatever step a hand-built `Scalar` holds.

A list parameter's `minItems`, `maxItems` and `uniqueItems` are one value,
`Items`, in the `List` that `Shape::Flag` carries beside the `Join`. A scalar
cannot hold them because no one value breaks them. `Items::check` is the only
place they are enforced and `Items::note` the only rendering, and its refusal is
the note after "is not a list of". `Invocation::new` asks it (`listed`) for each
parameter given at least once, after every value has met the rules of `items`,
so the command line and the typed call are refused alike
(`ValueError::BadList`). `uniqueItems` compares what the items' rule reads, so
`1` and `01` are one integer. `Items` is read for parameters only. A list rule
that a body's array states is checked by no road, bar one case. typify 0.8
enforces neither `minItems` nor `maxItems` and writes a `Vec`, except for an
array whose two are equal and more than 0 and whose `uniqueItems` is not true:
that is a fixed-size array `[T; N]`, which both roads hold (`tests/shapes.rs`).
`docs/validation.md` says so.

`pattern` runs on `regress`, the engine typify puts inside a generated newtype's
`FromStr`, so a value the command line accepts is a value the generated type
accepts by construction. No `format` is special-cased: a format names a rule and
is not one, and a document that wants a rule states it in JSON Schema everything
can read.

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
*not* on `Scalar`: everything a `Scalar` carries is a rule `Scalar::parse`
enforces and `Scalar::note` renders, bar a string `enum`, whose values the flag
lists, so a format placed there would be a rule by the back door — the defect
the invariant above exists to prevent. Where there is no value there is no kind,
which is why an `Unreachable` parameter and a `JsonWhole` body report none, and
why the format is read off the same schema the `Scalar` was read off — the
items' for a list. `schema::format_of` reads it off `stated()`, which is what
`Settings::replace` keys on too, so the generated types and the reduced model
name one vocabulary. Unlike `description_of`, the named schema wins over the
field: a sentence is about the field, a kind is about the value.

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

**A JSON body goes out under the `content` key it was declared under.**
`Body::JsonFields` and `Body::JsonWhole` carry the key as the document spells it,
parameters included, as `Body::Opaque` does, and `Invocation::request` sends it
through `Body::media_type`, the one reading of it, which leaves off a space and a
tab at the two ends of the key: a field value does not start or end with one
(RFC 9113, section 8.2.1). So the command line and the typed call send one
`Content-Type`, and `Values` has no way to name one. The key that is sent among
several is picked in `Body::build`: the first that `is_json`, wherever it stands,
and else the first of all, so a multipart key is no better than another. The
generator does not pick again: it reads the `media_type` field of the two JSON
variants where it types an argument (`src/generate/names.rs` and
`src/generate/ops.rs`), and `json_body` finds the entry under that key, so the
type of a wrapper's argument and the request come from one entry. A response is
another pick, over its own `content` with the same `is_json`. Only the key that is
sent is read, so only that key can be refused (`MediaType`, `MediaRange`), and a
range that stands after a key that names JSON loads. A media range is a `*` in
the type or in the subtype (`is_media_range`). `is_json` reads the `+json` suffix
and nothing else, so `application/*+json` names JSON: where it is the first key
that does, it is the key that is sent, and it is refused as a range. A response
is typed from it.

`MediaType` is two questions. Every request body key that is picked is asked
`is_media_type`, a `type/subtype` of two `token`s, and reads as `is_json` does,
with `str::trim`. A key that goes out as the request's `Content-Type`, a JSON
body's and one that is sent verbatim, is asked `is_content_type` as well, after
the question of a range: the header is read as RFC 9110 reads it, so only a space
and a tab before the type and after the subtype are trimmed, and the whole key is
a header value (an ASCII control character other than a tab is not one, and a
byte of 0x80 or above, a C1 control character among them, is). A
`multipart/form-data` key is never asked `is_content_type`, because that body
writes its own `Content-Type` with a boundary, and a refusal over a key that is
not sent would refuse documents this crate serves correctly. The message prints
the key as the document spells it and writes each character that does not show as
its code point (`with_code_points` in `src/model/reduce.rs`: every character that
`char::escape_debug` does not print as itself, a quote and a backslash excepted),
because an Overlay's target is matched against the spelling and a key refused for
a character that shows as nothing would read as a media type with no fault in it.
`docs/validation.md` states the rule and `tests/media.rs` pins it.

**Whether a body is required is the body's, and both roads read it from the
model.** `requestBody.required` is one fact and the `required` list of its schema
is another: the list says what a body holds once there is one. `Body::JsonFields`
carries the first beside `Field::required`, which is the list's alone, as
`JsonWhole`, `Multipart` and `Opaque` carry it, so a required flat body in which
no property is required differs in the model from an optional one. `check_body`
in `src/request.rs` refuses a required body that is missing, whatever its kind
(`ValueError::MissingBody`), and `tree::payload` sends none for a flat body that
nobody filled in, as it does for the other kinds: no file and no flag is no body,
and not `{}`, which the schema may refuse for want of a required property.
Whether that is a refusal is `check_body`'s alone, so a required flat body in
which no property is required is refused on the command line as on the typed
road, and a `--json-body` file that holds `{}` is how an operator asks for the
empty object. A property's flag is demanded of the command line where the body
and the property are both required (`json_field_args`), and `--json-body` stands
for it. The generator reads the same field (`add_body` in `src/generate/ops.rs`):
a wrapper takes a required body as its type and an optional one as an `Option` of
it, and `None` is `Values::body(None)`, so a body that one road can leave out is
a body the other can. A body kind that is added carries `required`.

**A shape the reduction cannot read is refused by name, never approximated.** A
`pattern` no engine runs, a `content` key that is not a media type and one that
is a media range are each the document saying something this crate has no reading
for, so each is a `LoadError` naming the operation and the thing that could not
be read — guessing what the vendor meant is a correction, and a correction is the
adopter's to write in an Overlay where a reviewer can see it. `Scalar::runnable`
asks the rules of a value that run but not as the vendor means: a `pattern` with
`\p{`, `\P{` or `\u{`, which has another meaning with the Unicode flag than
without it, and the generated type cannot be given the flag, and a `multipleOf`
that is not more than 0. Both are `LoadError::Unrunnable`, whose cause prints the
pattern or the step, and so is an `enum` that lists `null` and no other value,
which `runnable_scalar` in `src/model/reduce.rs` asks beside it because `Scalar`
holds no `null` and the rule would otherwise read as no rule. The escape is found
by reading the pattern, a backslash taking the character after it, so `\\p{3}` is
not one. Compiling both sides with the flag is the alternative that is out of
scope, because it is a change to typify.

One reading approximates, and no code here can mend it: `openapiv3` keeps an
integer's bound and step as a double and not as their text, so a text with a
fraction that rounds to a whole double (`minimum: 1.00000000000000001`,
`multipleOf: 2.00000000000000001`) is read as that whole number. The flag then
accepts or refuses one integer against the document for a bound, and holds the
integers to the multiples of the whole number and not of the text for a step.
*A scalar is read however the object model holds it* says which doubles are
read.

The escape rule is asked of every schema the model reads, and not only of the
`pattern` that a scalar carries, because the generated types are made of the
named schemas, the inline body and the inline response, and each of them runs its
`pattern` without the flag. The line between the two refusals is
`runnable_scalar` and not the flag. A `pattern` that the scalar of
`runnable_scalar` carries is `LoadError::Unrunnable`, which names the value, and
it is that with a flag or without one: a body property listed after a nested
one, a parameter with a `deepObject` style, a parameter whose name has no
spelling. `src/patterns.rs` walks the overlaid document by the table of
`src/shape.rs` at `Reach::Defined`, after the reduction, and every other
`pattern` is `LoadError::UnrunnablePattern`: one in a schema that
`runnable_scalar` does not read (a response, a named schema, the items of an
array body, a property of a nested object, a cookie parameter), and one that the
scalar drops because it keeps the `enum`. It names the operation where the
schema stands in one and the schema as an Overlay names a target. A component
that operations reach names none, because one schema serves many and the repair
is made at the component. The walk reaches a few schemas that no type is made
of, such as an error response's. Both ask `scalar::unflagged`, so the rule is
written once.

`runnable_scalar` in `src/model/reduce.rs` is where the rules of a value are
asked, `Scalar::runnable` and the `enum` of `null` alone among them. It reads
the values that the reduction reads for a scalar, with a flag or without one: a
parameter that `declared` keeps and that is sent in the path, the query or a
header with a `schema` (its items, for a list), and a property of a JSON body
whose schema is an object. `json_body` sends the whole body through
`--json-body` when a property has no flag, a nested one or one whose name has no
spelling, wherever it stands, so it reads every property (`properties_of`)
before it decides whether the body is flat. A property the API sets is read for
no rule. A rule asked only of the properties listed before the one that sends
the body whole would make a document that loads depend on which property comes
first. A cookie parameter, a parameter stated with `content`, a property of a
form or multipart body or of a body whose schema is an `allOf` of more than one
schema, and the items of an array body are not read by it. The walk asks them
the escape rule and nothing else, so a `multipleOf` of 0 or a `pattern` that no
engine reads loads there. A rule of this kind that is added for a value is added
in `runnable_scalar`, which `shape_of` and `properties_of` share, and a rule of
every schema is added to the walk.

**A document is read as the version it says, or refused by name.**
`Document::load_with` asks the vendor's `openapi` right after it reads the
document and before the first Overlay, and a version that is not `3.0` or
`3.0.x` is `LoadError::Version`, which prints it. `3.0.x` is `3.0.` and a patch
number, and a pre-release label may follow it, as in `3.0.0-rc1`. 3.1 spells
some rules another way, and the model refuses those with no place, and it means
others that 3.0 does not have, and the model drops those in silence: a `$ref`
keeps its sibling `maximum`, and `const` is a rule. Support for 3.1 is out of
scope, so the refusal names no repair.

The version is the vendor's, and no Overlay changes it. `overlay::apply`
compares `openapi` before and after each layer and refuses any difference,
another version, another patch of 3.0, a value that is not text, or no key, as
`OverlayError::OpenApiVersion`. Both roads put the layer on that error already
(`LoadError::Layer`, `GenerateError::Overlay`), so the refusal names it on both.
A label that an Overlay writes changes how every schema is read, and nobody who
reads the Overlay can see which rules it drops: it is the approximation that
the rule above forbids, and not a correction. A document with no `openapi` is refused
by the model at `.`, a value that is not text at `openapi`, and no Overlay can
add the key. The bless step lays the layers itself, so `Settings::correct` asks
the vendor's version before its first layer, as `load_with` does, and the
refusal of a 3.1 document comes before the refusal of a layer that does not
apply to it.

**A document that leaves out what a request needs is refused with the Overlay
that adds it.** `NoServer` is a document with no `servers`: `Document::base` is
the first server, the default that the command line and the typed client both
start from, and a caller replaces it where the host is theirs to choose, so a
document that lists none has no default to give. A relative first server such as
`/` loads for the same reason, as a default the caller replaces. `NoOperationId`
names the method and the path of an operation with no `operationId`, because
every later refusal and every generated wrapper calls the operation by it. A
server URL with variables takes the `default` of each, which OpenAPI requires for
every variable; a variable that `variables` does not declare stays in the URL,
which is then no URL (`ServerUrl`, which prints the URL as the document spells it
and as it was refused, with the defaults in it). A URL that holds userinfo, a
query or a fragment is `ServerBase`, which says which and prints none of the URL,
because the part that is refused is the part that is a credential or a key.
Userinfo is read from the text before the text is parsed
(`BaseUrlError::userinfo_in`, over `authority_spans` in `src/request.rs`: the
text up to its first `/`, `?` or `#`, and, when that character starts a `//`, the
text after it up to the next one), so a URL that holds userinfo and is no URL is
`ServerBase` as well, and `ServerUrl` prints a URL that holds none. What stands
before the `//` is not judged to be a scheme, because a document writes whatever
it writes there (`{scheme}`, a space, no colon), and a credential is a credential
in a URL that nothing parses. `ServerUrl` prints the spelling beside the text
with the defaults, so a text that is refused as a URL is read in both (`base_of`
in `src/model/reduce.rs`): a default that holds a `/` can move the `@` out of the
authority of one and not of the other. A text that parses is read once, with the
defaults, because that is the URL a request goes to. A text with an `@` in none
of the places that are read stays `ServerUrl`, which prints it: `http:/u@h/a b`,
and `{a/b}://u@h/ x`, where a `/` stands before the `//`. The same reading holds
a caller's base to the rule in `Invocation::request`: the `http` crate takes
`//alice:s3cret@host/` for a path with no authority, and it is `Userinfo` all the
same.

**A refusal names its operation and the place it stopped, and its repair only
where the repair is true.** A `LoadError` from one operation carries the
`operationId`; one from a path item carries the path (`PathItem`), and one from
an Overlay carries the index of the layer in the list `Document::load` was given
(`Layer`). A refusal of an `x-cli-` key (`UnknownMarker`, `MisplacedMarker`)
carries the node that holds the key and no `operationId`: the scan runs before
any operation is reduced, and the key may stand on an object that is no
operation. A name that cannot be spelled is `Name` for a group or a command,
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
name neither it nor anything it composes with declares, walking the whole table
of `src/shape.rs`. The reduced document is the vendor's plus the adopter's
Overlays — the one that has to be right — so a name nothing describes is a
vendor mistake the adopter repairs in an Overlay, where the correction is a line
a reviewer reads. What a miss costs is measured rather than argued: typify
writes `pub b: ::serde_json::Value`, required and with no `serde(default)`, so
the generated type refuses the vendor's own responses.

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
have, and a document that is not 3.0 is refused first. A 3.0 document that
carries one is read all the same: the walk reads the keyword and refuses a
phantom under it, although the model drops it, and the refusal names an Overlay.
So it does below `not`, which is rare.

The walk takes its nodes from a work list that shares the `followed` set, and
makes no call for a hop. A chain of `allOf` hops is as long as the vendor wrote
it, and a call per hop overflows the stack at 7,000 of them, on a thread of
8 MiB too. `schema::follow` is a loop for the same reason, so a reference chain
adds no recursion to either walk.

**A shape this crate cannot spell stops at the operation, not at the document.**
A parameter that is `in: cookie`, described by `content`, an object, or an array
whose items are not values (`Unsupported::Structured`), stated with a keyword
this crate does not read as a value (`Keyword`) or with no `type` (`Untyped`),
declaring a `style` this crate does not serialise, *named* in a way
`names::spelled` refuses, or sharing its name with an earlier parameter of the
operation (`Unsupported::Shared(Place)`), is carried as `Shape::Unreachable` —
named on the subcommand's long help and in the wrapper's documentation, and
given no flag, no argument and no place in the request — and only a *required*
one is a `LoadError`, because an operation that could never build a correct
request is worth refusing by name. It is the rule a body already follows, where
one nested property sends the whole body through `--json-body` rather than
refusing the document that holds it.

**A scalar is read however the object model holds it, and any other shape names
its keyword.** `openapiv3` holds a schema as `AnySchema` when it fits none of
its types: an integer bound spelled `0.0` or `1e3` is an `f64`, a bound past
`i64` is one too, and a `string` beside an `allOf`, a `pattern` beside an
`integer` or a `minLength` beside an `array` has the keywords of no type.
`typed` in `src/schema.rs` is the one place that reads such a schema, and
`scalar_of`, `lists_only_null`, `format_of` and `array_of` ask it. `any_type`
matches the `type` once, and the arm gives the keywords that type has with the
way it is read, so a keyword is named for each of the six types, an object and
an array included. The unit test in `src/schema.rs` puts each keyword beside
each type, because a keyword of the table that nothing exercises is a keyword
read as no word at all. An `AnySchema` whose `type` is a scalar or an array and
whose keywords all belong to that type is read as the type `openapiv3` would
have made. That holds for an array: `openapiv3` holds it as `AnySchema` for a
keyword that states nothing (`properties: {}`, `enum: []`), `array_of` reads it,
and `items_of`, `list_type` and the template walk (`Walk::skeleton` and
`Walk::without_read_only`) ask `array_of`, so such an array has the list flag
its twin has, and a body that is one has the template its twin has. No object is
read as a value, and `json_body` and the template walk read a body's object only
where `openapiv3` holds it as an object. A body that is an object it holds as
`AnySchema` (one that states a keyword of another type, or a keyword with no
content such as `enum: []`) goes whole, with no template but its `example` as the
document has it and with no read-only names. Every other shape is an
`Unsupported` that is true of it:
`Keyword` for a composition, for a keyword the `type` does not have, for an
`enum` that lists a value of another type and for a `type` that is none of the
six, `Untyped` for a schema with no `type` and no composition, and `Structured`
for an object whose keywords all belong to it and for an array whose items are
no value. The keyword is the first the schema states, in one fixed order with
the compositions first, because the object model keeps no order among them. An
integer's bound is read wherever its double names the integers the bound admits
(`integer_end`). A double with a fraction is the integer next inside it, as an
inclusive bound, because the text of an integer gives a whole double and no
integer lies between the text and the double. A whole double below 2^53 is that
integer whatever text gave it, which is the approximation named above. A double
past every `i64` bounds nothing, or admits none and is refused by its keyword. A
whole double from 2^53 to 2^63, both ends included, is the double of more than
one integer's text, so it is refused by its keyword and never rounded again. A
step is read as an integer only where it is whole (`integer_step`): a whole
double below 2^53 is that integer whatever text gave it, as for a bound, which is
the approximation named above, and a double with a fraction or a whole one from
2^53 is refused by its keyword. A
parameter takes the reason as its `Unreachable`, and a property takes it as the
body going whole, as any property with no flag does. Neither refuses the
document that holds it, and only a required parameter does.

`stated` is the one reader of a value's schema, a list's, a JSON body's and a
multipart body's. A single-element `allOf` is a `$ref` that kept its siblings
wherever it stands, so `items_of`, `json_body` and `part_names` unwrap it as
`scalar_of` does. `resolve_schema` follows a `$ref` and no more, and is for the
sentence and the example, which are the field's own before they are the named
schema's.

What has a flag decides which flags move, and what a command line sends. A flag
goes to the first claimant, the parameters in the order of the list and then the
properties of the body, so a value that gains a flag takes it from a later
parameter or property, which becomes `--param-<name>` or `--body-<name>`, and a
value that loses its flag (a `readOnly` property) gives it to a later one. A
body that gains a flat shape loses its template, and a body that gains a
template gains `--json-body-template`. A body is flat when each property has a
flag or is `readOnly`, so a body goes flat when its wrapper is read, when a
scalar that `openapiv3` holds as `AnySchema` gains a flag, and when a `readOnly`
property stops sending it whole. A value that gains a flag is asked its rules
and its name, so a document that loaded can be refused (`Unrunnable`,
`Unresolvable`, `HeaderName`, `GateTakenByName`, `CommitTakenByName`), and the
rules of a property of a JSON body are asked whether or not its body goes whole.
The name of a property is asked only where its body is flat, as a
parameter with no flag is not asked its name: `fields_of` decides that the body
is flat before it claims a flag, and then claims the flags in document order. A
body that goes whole claims none, so none of its properties is asked, in
whatever order they stand, and a name that consent is given by never refuses a
document for a flag that is not made. A change to what makes a body flat moves
which names are asked. A change to what `stated`, `scalar_of`, `array_of`,
`read_only_of` or `json_body` read, or to what makes a body flat, lists all of
these in the CHANGELOG, each with its repair, because they are what an adopter
sees of it.

Every flag a user types passes `names::spelled`, which is why a body property
spelled `*` sends its body whole rather than claiming a flag called `""`. The
body's rule turns on whether a property has a flag and never on why it has none,
so an unspellable property, a nested one and one that is no scalar this crate
reads (a composition, a keyword its `type` does not have, no `type`) reach the
same answer — and the `Body::JsonWhole` template is then the one place the key
is written down. The body is no `LoadError` for any of them and the long help
states no reason for any: a parameter has nothing to fall back on and a body has
the file, which is read as JSON and, at the adopter's seam, held to the
generated type by `fits`. A property the API sets is the one outside that rule:
it is no part of the request, so it has no flag and decides nothing.

`spelled` refuses a name that holds a character outside ASCII, and
transliterates nothing. `kebab` would turn that character into a separator, so
`straße` would be the flag `--stra-e`, which is no word of the vendor's and which
two different names can share. A letter written as a base and a combining mark
is one letter to a reader and would be mangled the same way, and a character
that nobody sees, such as a soft hyphen, would be a separator that nobody sees.
The test is `is_ascii` and not a test for a letter, a digit or a mark, because
those are tables in the standard library that change with the toolchain, and a
name has to be read the same way by every compiler. The `NameError` names the
first such character by its code point (`U+00AD`), because a soft hyphen shows
as nothing and an en dash as a hyphen, so the name beside a message that only
said "does not kebab-case" would look right. A path segment that holds one needs
`x-cli-group` or `x-cli-command`, and `Name` says so; a gate or the confirmation
word is refused; a parameter is `Unreachable`, and only a required one refuses
the document; a body property sends the body whole.

**An operation's parameters are the ones OpenAPI defines, and the request goes
where the document says.** A parameter is its name and its place. `declared` in
`src/model/reduce.rs` follows the `$ref` of each parameter and builds the one
list that everything reads: the path item's entries in the order the path item
lists them, each entry of the operation with the name and the place of one of
them in that one's slot, and then the operation's other entries in the order the
operation lists them. An entry of the operation with a pair that no slot is left
for is another parameter, which is how two entries of one pair stay two. The
list is the one order the document fixes, because `openapiv3` keeps none between
a path item's `parameters` and its methods, and an override that keeps its slot
keeps the wrapper's argument order and the help order where the path item put
them. `declared` drops the header parameters `Accept`, `Content-Type` and
`Authorization`, compared without case and in a header only, because OpenAPI
3.0.3 says they "SHALL be ignored". They are dropped and not carried as
`Unreachable`, because a required one would then refuse a document that is
right, and a flag for the `Authorization` header puts a credential in argv, in
shell history and in every dry run. A credential that the document declares
under another name is a parameter like any other. `place` is the one map from
the four kinds of `openapiv3::Parameter` to `Place`, with no `_` arm.

`Values` is keyed by the name alone, and so is `Operation::param`, which finds
the first. So of two parameters left with one name the one that is later in the
list is `Unsupported::Shared(Place)`, the place of the earlier one, whatever the
shape or the place of either, and unless it has a reason of its own: a reason of
its own wins over `Shared`. It claims no flag, and a required one is
`LoadError::Parameter`. The earlier one keeps the name even when it has no flag
itself (`in: cookie`, `content`), and a required path parameter that comes later
refuses the document. `docs/cli.md` says the repairs of both costs. A rule by
reachability would make `Operation::param` "the first parameter with a flag",
and a rule by place would add a second order beside the list. Both would widen a
public seam for a rare document, and both would contradict spec F-10 ("the one
the document lists later"). The values under a name are the first parameter's,
so `Invocation::new` counts the given values of a parameter with a flag only.
The generator reads the same list: `entry_of_flag` finds the entry that a flag
was read from in what `declared` returns, which is the first entry with the
flag's name, because the first entry keeps the name and no later one has a flag.
So a wrapper's argument is typed by the entry that won, inline or by a `$ref`,
and the generator keeps no second copy of the merge. The entry is read as the
reduction read it: `unwrap_inline` in `src/generate/ops.rs` unwraps an `allOf`
of one element and leaves a `$ref` for `ref_name`, which wants the name of a
named schema where it stops, and `scalar_type` asks `schema::typed`, so an
argument has the type of the flag that carries the value. A list's array is read
through `schema::stated` with the `components` that `gather` holds
(`list_type`), as `items_of` reads it, so a list whose array is a `$ref` is a
`Vec` argument and the reduction and the generator read one schema.

A header parameter that would claim a flag and whose name `HeaderName` refuses
is `LoadError::HeaderName`, because no request could carry it. A parameter with
no flag sends nothing, so its name is not asked, and a parameter in another place
has no header name to be.

`LoadError::Parameter` says where the refused parameter is (`place`), because
the repair depends on it. `repair` names a repair that loads the document when
the parameter is the one fault in it. A document with more faults is refused for
one at a time, and each repair ends its own refusal and brings back none that an
earlier repair ended. A path parameter that is not `required` is refused
(`OptionalPathParameter`), so dropping the `required` of one is never named.
A path parameter is renamed together with its variable (`rename`), because the
template names it and no `update` of the parameter alone loads; no Overlay
action renames a key, so the Overlay writes the path item again under the new
path and removes the old one. The request is the same, because in the `simple`
style, the only one this CLI writes in a path, the name of a path parameter does
not go on the wire. So the rename gives up no parameter, and it is the repair
for a name with no spelling and for a shared name when one of the two is a path
parameter and the other is not, whichever of them was refused.

The words follow one rule. A repair that keeps every parameter the operation
declares is named first. A repair that also ends the fault by giving up a part
of the document is named after it, because only the adopter knows if that part
is the vendor's mistake: removing the namesake, which gives the refused
parameter its flag, or dropping the `required` of a parameter off the path. No
repair is named that does not load, so a parameter off the path is never told to
remove a namesake that is a path parameter, because the template names it. Two
cases get no rename. Both names go on the wire when neither parameter is in the
path, so a rename changes the request and is no correction. Two path parameters
of one name have one variable in the template, so no rename parts them, and the
later one is told to remove the earlier. A message with two parameters of one
name says which parameter each repair acts on (`parameter`: its place and its
name, and "earlier" and "later" where the place is the same), and never writes
"its" for either one: the `required` of the namesake is no repair, and a
pronoun reads as the parameter named last. Each repair that `repair` names has a
test in `tests/wire.rs` that loads the document with the Overlay the words name,
and a new reason or place adds one.

`reconcile` holds the path template to the path parameters, and refuses a
variable with no path parameter, a path parameter with no variable and a path
parameter that is not required, so `{id}` is never on the wire. A `{` that
nothing closes is text and not a variable. `no_servers` refuses a `servers` on a
path item or on an operation (`Servers`), because a request goes to
`Document::base` and the model holds no host per operation; it spells the list
as an Overlay names a target, so the repair is a line to paste.

None of these rules is repeated by `Document::check`. The model holds no
`servers`, and a blob that breaks one of the others sends a request that is
wrong, which costs the run that meets it. A write is still held by
`Plan::decide`, and the flags of an operation are distinct by their own names,
so clap does not panic.

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

**A property the API sets is no part of the request, and both roads say so.** A
`readOnly` property has no flag, whether or not the schema requires it: OpenAPI
3.0.3 says that a request "SHOULD NOT" send it and that its `required` takes
effect on the response only. `properties_of` in `src/model/reduce.rs` sorts
every property of a body's object by what a request does with it (`Role`): the
API sets it, it is a scalar and gets a flag, or it is anything else and the body
goes whole. The first decides nothing about the body, so it neither sends the
body whole nor needs to be a scalar, and its rules are read by no flag. The two
JSON variants of `Body` carry the names (`read_only`), for the long help in
`tree::long_about`, which says where the flag went. `Walk::object_skeleton` in
`src/schema.rs` leaves the property out of the template at every depth of the
skeleton, and notes each one it leaves out below the body's own object, by its
path as a `BodyError` spells a key, with `[]` for any item of a list
(`address.aid`, `positions[].id`, `[].id`): the help names what the template
leaves out, from the walk that writes the template, so the two cannot part. An
`example` is the better source of a template, and `Walk::without_read_only`
walks it beside its schema, through an object and a list that this crate reads
and into the value of each key that the object's `properties` declare. It drops
a read-only property there whether the document requires it or not, naming it by
the same path. The example stays as the document has it wherever the walk does
not go, which is wherever the schema is no object and no list that this crate
reads, or the example has not their shape, or the key is not one that
`properties` declares. Such places are below a composition, below a schema with
`properties` and no `type`, in an object or an array that states a keyword its
`type` does not have, in an object that `openapiv3` holds as `AnySchema`, and
below a schema that cannot be followed. A template is no place to refuse a
document that loads. A read-only key
there stays in the template and is named nowhere, so the template is the
example, not a body that has been checked. The walk enters a required property
only, so a read-only property that the document does not require below the
body's own object, and that no `example` carries, is left out of nothing and is
not named. The own object's names come from `Role`, each of them, and a body
that goes whole carries those first. A property is `readOnly` through
`schema::read_only_of`, which asks every schema that `stated` passes: the
property's own, the one a `$ref` leads to, and an `allOf` of one element and its
element. A `$ref` erases the flag beside it, and the wrapper is the spelling
that keeps it. `writeOnly` is not read.

The generator makes the same decision for the types (`NotRequired` in
`src/generate/types.rs`). A type that a request body reaches does not require
the property, and a type that only responses reach keeps the document's
`required`, because there it takes effect. A body reaches the schemas it names
or states and every schema those refer to (`reached`), found on the schemas as
`schemars` holds them, so an `example` that spells a `$ref` reaches nothing. The
visitor reads a property as `read_only_of` does, in `schemars` types: the
property is marked, it refers to a schema that `read_only_of` found read-only,
or it is an `allOf` of one element that is. `tests/readonly.rs` runs every
spelling through both roads, so the flag is gone exactly where the type stops
requiring the property. A schema that is an `allOf` is one type in typify, so
`NotRequired::declared` reads the properties that the schema and every member
declare read-only, a member that is a `$ref` through the named schema it leads
to, and `relax` takes each off the `required` of the schema and of every member
stated inline in it. A member that is a `$ref` keeps its own `required`, because
the named schema is one type wherever it is used, and a body that names it alone
requires what it declares without `readOnly`. That is the first of two limits.
The second is a body that is a `$ref` to `#/components/requestBodies/`: the
generator follows none, so a type that only such a body reaches keeps the
document's `required`.

**One request builder serves both consumers.** A CLI reaches it through `tree`,
a generated wrapper through `Values` directly. Anything that makes the command
line and the typed call disagree about an operation is a bug, not a feature.

`Invocation::new` refuses five values that the request would carry as another
value than the caller gave, whichever road they came by. Beside the schema it
refuses a path value that is empty, `.` or `..` (judged as the text `url` writes
for a list), and four header values. A receiver that follows RFC 9110 would
read another value, or another list, than the caller gave when the value holds
a byte that `HeaderValue` refuses (`HeaderValue`), starts or ends with a space or
a tab (`HeaderWhitespace`, an item of a list too), and, for a header list, when
one item holds a comma (`HeaderItemComma`: a header is not percent-encoded as a
query or a path list is) or is empty (`HeaderItemEmpty`: a receiver ignores an
empty list element). A byte of 0x80 or above reaches the receiver as it is, and
so does an empty value that is no list, so neither is refused. A header refusal
names the parameter and prints no value, because a header value can be a
credential; `PathSegment` prints its segment, which is one of three fixed words.
`Invocation::request` refuses a base URL that holds userinfo or a query
(`BaseUrlError`), so the gate, the typed call and the command line end in one
refusal whoever names the base, and `render` prints the host and the port and
never the userinfo. An `http::Uri` drops a fragment when it parses, so the
fragment is refused in the text of the document's server
(`LoadError::ServerBase`) and nowhere else.

No text that the library makes holds a base URL's userinfo. The refusals of
`BaseUrlError` and `LoadError::ServerBase` print no part of the URL;
`LoadError::ServerUrl` prints a URL that holds no userinfo where an authority is
read, because the userinfo is read first; `render` prints the host and the port;
and a type that holds a base writes its `Debug` by hand and leaves the userinfo
and the query out (`request::without_userinfo_and_query`). The `Debug` leaves
the userinfo out through `without_userinfo`, which reads the places that
`userinfo_in` reads, so a text is changed exactly when it is refused, and a test
holds the two together. `render` gives it the text of the host, and the request
line only of a URI with no host, because only then are the path and the query
the whole text that is read. With a host a `//` at the start of the path is a
path, an `@` after it is no userinfo, and the line shows the path that is sent
(`http://host//u@h/`). Three types hold a base. `Client` holds the
caller's, and `Document` holds the one its load, its blob or a read through
`Deserialize` in another format gave it: the last two read with no load, and
`from_blob` does not repeat `ServerBase`, so the base of a `Document` can hold
userinfo, and `Document` binds each of its fields by name in its `Debug`, so a
field added there stops it compiling. `Call` holds a reference to the base of
its `Client` and its `Debug` shows none. A type that comes to hold a base owes
the same. The query is left out with the userinfo because a key goes there, and
a request from a base that holds one is refused. The text of the operator's own
argument is the operator's, and clap repeats it in its own errors.

**A call-time error names its operation, and no message or cause of this crate
prints a byte the server sent.**
`client::Error::{Encode, Transport, Status, Decode}` carry `op`, the
`operationId`, and their messages begin with it, so a log line says which call
failed. `Values` names it too, in the message of its `ValueError`. `Encode`
comes from `to_json` while a generated wrapper builds the arguments of a call,
before the call exists, and `Transport`, `Status` and `Decode` come once the
request goes to the client. `Request` is raised while the request is built,
after the call exists and before anything is sent, and names no operation.
`Status` prints the operation and the status and never the body: the body is the
server's and can hold a credential or a client's data, and sevdesk-cli
re-exports the type as its own error, where its rule "No peer bytes in an error"
applies. The cause is printed where the message is, because whoever reports an
error walks `source()`, so the cause of `Decode` holds no byte of the body
either: it is a `DecodeError`, which keeps what `serde_json` says of the kind of
failure (`DecodeClass`) and of the line and the column, and none of its message,
which can quote the value, the key or the variant it refused. What the place and
the class of that cause mean is said in two places that keep the same words, the
docs of `DecodeError` and `DecodeClass` and `docs/client.md`, so a change to one
is a change to the other, and `tests/typed_call.rs` holds the bodies that were
probed. A change to those words is checked against a probe of every failure, as
the rule below says. `Syntax` is `serde_json`'s category and holds valid JSON of
a form it will not take, so its words claim nothing about the JSON. No
`serde_json::Error` is held by `Decode`, because a caller that can get one can
print it. The cause of `Encode` is `serde_json::Error`, which can quote only the
caller's own value, and the cause of `Transport` is the client's error, which
prints what that client prints. The body is the field, and `Error::body` reads
it for `Status` and for the `raw` of `Decode`. A variant added to the call-time
group carries `op`, and a cause of it that could hold the server's bytes is a
type of this crate that keeps none. `to_json` takes the operation, because only
the generated wrapper knows which operation a body is the body of, so an
argument that does not serialise names it.

**A request body is held to the generated type in one place, and an undeclared
property has no way through.** `client::fits` is where a JSON body meets the
type of its operation. The library cannot name that type, so the adopter's seam
calls it (`tree::select`, then the generated `OperationId::check_body`), and a
CLI that only calls `tree::dispatch` has none. Each refusal is its own
`BodyError` variant: is the body an object where the type takes one
(`NotAnObject`), do the values have the right types (`Mismatch`), and does the
type write back everything the body holds (`Undeclared` for a property,
`NotAnObject` for a list that serde read by place). A type that cannot be
written back is `Unwritable`, because there is nothing to hold the body beside,
and the gate fails closed. A new refusal is a new variant, since `BodyError` is
`#[non_exhaustive]`. `Undeclared` and `NotAnObject` print no value of the body,
which can be a credential or a client's data. The cause of a `Mismatch` is
serde's message, which can quote it. A path is spelled as `serde_path_to_error`
spells one (`address.extra`, `positions[1].flag`), and `client::spelled` is the
one place that spells the place of a refusal, whether the walk found it or the
read did. `schema::Place` spells the path of a read-only property with `[]` for
any item, because the path names the property of every element.

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

**Every YAML text is read by one function, and no type of a 0.x crate is in a
public error.** `read` in `src/overlay.rs` is the function every document and
every Overlay goes through, into a `serde_json::Value`, before anything else looks at it. That
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
or typify is never a break here. The foreign types that a public error does
carry come from crates at 1.0 or later: `serde_json::Error`,
`serde_json::Value` (`OverlayError::OpenApiVersion`), `http::Error`,
`http::StatusCode` (`client::Error::Status`), `http::uri::InvalidUri`,
`postcard::Error`, `syn::Error`, `std::io::Error` and
`clap::parser::MatchesError`. A type of a 0.x crate in a public error would make
a release of that crate a break here.

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
with it because every `OverlayError` variant but `OpenApiVersion` carries one,
and `overlay::Place` because `Cause::place` returns it. `required::check` stays
public with the `PhantomKeys` it returns, and `scalar::Text` stays public
because an adopter's test names it. A type that a public variant carries is
public too, which is why `RefError` is re-exported at the root while the rest of
`schema` is not. `unreachable_pub` is a warning the gate denies, so a `pub` item
the crate does not export fails it.

**Every public item is documented, and no `allow` goes on an error.**
`#![warn(missing_docs)]` is in `src/lib.rs`, and the gate's `-D warnings` makes
it a refusal. `missing_docs` takes its level from the item that holds it, so an
`allow` of that lint on an error enum also exempts every variant and field in
it. A variant says when the crate raises it. A field says what the value is or
where it comes from: the `operationId`, the flag as the operator typed it, the
path into the body. It never says the `#[error]` message again, because the
message prints it already. A doc that says what a foreign cause's message holds
says only what a probe of every failure shows, and the same holds for the class
and the position of a `serde_json::Error`: a sentence about them is a rule that
every probe keeps or an example that a test holds, and a list of cases says
"such as". Reading a `serde_json::Value` into a type names a value and a type
for some failures, and for others only a missing key or an unmatched `untagged`
enum. That read, `postcard` and `syn` over tokens carry no position. The adapter
rule, that an adapter must not turn a status code into an error, is on
`SyncClient` and `AsyncClient` themselves, because an adopter writes an adapter
from that page.

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
of a generic, bumps `MODEL_LAYOUT` in the same change. So does a load that
writes something different for the same document with no type changed, such as a
name that `names::spelled` refuses or spells another way; `tests/layout.json`
then differs in `model_layout` only. Then
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
word, which has none. Each rule is asked in one place that the load and the
check share: `spends`, `spends_command`, `gate_on_read`, `first_collision`,
`Effect::of_method` and `unmet` in `src/model.rs`, because `reduce` is behind
the `document` feature and `blob` is not, and `names::is_spelled`.
`first_collision` makes one pass over the operations, because the check runs at
every start-up of a shipped binary and the number of operations is the
document's. `spends_command` is beside them, and `placement` in
`src/model/reduce.rs` asks it of the group and the command, as `check_names`
does. `check_requires` matches `Unmet` with no `_` arm, so a new way for a
`requires` to fail stops compiling until the decode names it.

The flags of an operation share one namespace: the confirmation word of a write,
the gates, the parameters that have a flag and the properties of a body, with
the six `SPENT` words spent. `Namespace::guarding` and `gates_of` in
`src/model/reduce.rs` keep it at load and `Operation::check_flags` keeps it on
the way in, so a rule added to one is added to the other. A read declares no
confirmation word, so a parameter of a read may be spelled like it.

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
all. Two refusals of a load stand outside the line. A blob whose header
parameter has a name that no header has meets the `http` crate when a value is
given for it (`RequestError::Http`), so `HeaderName` is not repeated. A blob whose
base holds userinfo or a query meets `Invocation::request`, which refuses every
base it is given, so `ServerBase` is not repeated either. Neither sends a
request. Three rules are repeated whatever their breaks, because F-70, the finding
that asked for the check, names them: the confirmation word, which is the
validation of `Loading::commit`, the spelling of each flag, and each `requires`.
The spelling of a flag and the confirmation word of a write have breaks inside
the line (a word with a leading `-` makes a debug build panic), and so has a
`requires` that names its own parameter (`RequiresItself`: clap refuses an
argument that requires itself). The other two breaks of a `requires`, a name the
operation does not declare and one that has no flag, cost the run only, because
`companion_flags` in `src/tree.rs` leaves them out of the command. No break of
the document's own confirmation word is inside the line. A slice that adds a
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

`Summary`'s `Display` is the page a bless step renders, and a name the document
chose goes into a table cell through `cell`, which escapes `|` as a Markdown
reader needs, in a code span too. The counts a test asserts against are the
accessors on the same value — so the sentence a reader is shown and the number a
test pins cannot drift apart. The page carries its provenance from
`Settings::summary_header`, beside the headers the other generated files get:
the command that counts it again and the documents it was counted from, because
a count a reader cannot re-take is a claim rather than a measurement, and the
two look identical on the page. `examples/toy/api/tests/summary.rs` is the
demonstration that the shape is right — it builds the toy README's own sentences
out of the summary — and any count this crate adds owes a row there.

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
