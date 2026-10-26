# What is checked, and what checking costs

Every rule the document states about a scalar value, and about a list of them,
is enforced on a parameter and on a body field's flag. The generated type of a
body field runs fewer of them, and [Where each rule runs](#where-each-rule-runs)
says which. The primer is [the README](../README.md); this page is the reference
for someone deciding how much of their API's contract they get for free, and
what the engine that provides it costs their binary.

## The rules

A scalar is a value that fits on one flag and in one struct field.
[`Scalar`](../typed-openapi/src/scalar.rs) carries what the schema says about
one, and `Scalar::parse` checks all of it.

| the document says | value name | a refusal reads |
|---|---|---|
| `type: string` | `<STRING>` | — |
| `minLength: 3` | | `` `EU` is not at least 3 characters `` |
| `maxLength: 3` | | `` `EURO` is not at most 3 characters `` |
| `pattern: ^[A-Z]+$` | | `` `x1` is not a match for ^[A-Z]+$ `` |
| `enum: [draft, paid]` | `<STRING>` | `` `void` is not one of draft, paid `` — and these complete |
| `type: integer` | `<INT>` | `` `5.0` is not an integer `` |
| `type: integer`, past an `i64` | | `` `9223372036854775808` is out of range: an integer here is between -9223372036854775808 and 9223372036854775807 `` |
| `type: number` | `<NUMBER>` | `` `x` is not a number `` |
| `type: number`, more digits than a 64-bit float holds | | `` `9007199254740993` does not survive the round trip through a 64-bit float, which would send 9007199254740992 `` |
| `minimum: 1` | | `` `0` is not at least 1 `` |
| `minimum: 0`, `exclusiveMinimum: true` | | `` `0` is not more than 0 `` |
| `maximum: 100` | | `` `101` is not at most 100 `` |
| `maximum: 10`, `exclusiveMaximum: true` | | `` `10` is not less than 10 `` |
| `multipleOf: 5` | | `` `7` is not a multiple of 5 `` |
| `multipleOf: 0.01` | | `` `0.005` is not a multiple of 0.01 `` |
| `enum: [100, 200]` on an integer or a number | | `` `7` is not one of 100, 200 `` |
| `type: boolean` | `<BOOL>` | `` `yes` is not `true` or `false` `` |
| `type: boolean`, `enum: [true]` | `<BOOL>` | `` `false` is not one of true `` |

Lengths count characters rather than bytes, so a three-character `€€€` passes a
`maxLength: 3`. A number JSON cannot carry — an infinity, a NaN — is *not a
number* rather than a `null` on the wire.

**A refusal is built from the note of its rule.** Every rule has one note, the
words that finish "is not …", and its refusal is "is not" and those words. The
help line below shows the note and the refusal says it, so the two cannot
describe different rules. The one exception is a string `enum`, which has no
note: the flag lists its values, and they complete.

**A numeric `enum` admits a value, not a spelling.** `enum: [100, 200]` on an
integer admits `+200` and `0200` as it admits `200`, and on a number it admits
`2e2` and `200.0` too; the request carries `200`. An `enum` beside a bound or a
step keeps both. An `enum` that lists `null` and no other value names no value a
flag can send, so it refuses the document while it is reduced, as a `pattern` that
cannot run does. A boolean `enum` of one value, `[true]`,
admits that value only; one that lists both says what `type: boolean` says and
adds no note.

**`multipleOf` is decided on digits.** The value is the digits it was typed
with and the step is the shortest text that reads back as the step, scaled to
whole numbers, so `0.07`, `0.29` and `19.99` are multiples of `0.01`. A binary
float divides `0.07` by `0.01` to `7.000000000000001`, which refused about one
amount in six, and divides `1e17` by `3` to a whole number, which admitted a
value that is not a multiple. A `multipleOf` that is not more than 0 has no
multiples to admit and refuses the document while it is reduced.

**A number is refused when it would be sent as another number.** An integer past
`i64` is a number all the same, and `typify` writes a `u64` for `minimum: 0`, so
it is *out of range* and never "not an integer". A `number` goes through a 64-bit
float, which holds the nearest of its digits: `9007199254740993` would go out as
`9007199254740992.0`, and `1e-400` as `0.0`. The test is whether the shortest
text that reads back as the float is the number that was typed, compared as
decimal numbers and not as text, so `1.0`, `1e0` and `001` are one number and are
accepted. A number with many significant digits can fail it, and so can a number
too small for a float (`1e-400`, `4.9e-325`); `0.30000000000000004` has 17 digits
and passes, because it is the shortest text of its float. A document that needs
a number that fails it states the value as a `string` with a `pattern`, as the
toy's `Voucher.total` does.

The same rules are the flag's help line, rendered by the same code:

```console
$ toy raw vouchers update --help
      --total <STRING>
          A decimal amount carried in a string. (a match for ^-?[0-9]+(\.[0-9]{1,2})?$)
```

A rule that reaches `--help` and not the parser would be a promise the server
has to keep instead, so `Scalar::note` and the refusal are one rendering and
cannot come apart.

### A list parameter

A parameter that is an array of scalars is a flag that may be given again. Each
value meets the rules of the array's `items` above, and the values taken together
meet the rules of the array itself:

| the document says | the help line | a refusal reads |
|---|---|---|
| `minItems: 2` | `at least 2 values` | `` `a` is not a list of at least 2 values `` |
| `maxItems: 2` | `at most 2 values` | `` `1, 2, 3` is not a list of at most 2 values `` |
| `uniqueItems: true` | `different values` | `` `1, 1` is not a list of different values `` |

The three are one value, `typed_openapi::scalar::Items`, with one check
(`Items::check`) and one note (`Items::note`). `Invocation::new` asks it for
every list parameter that was given at least once, after each value has met its
own rules, so the command line and a typed call are refused alike
(`ValueError::BadList`). A list that is not given is not a list of too few.
`uniqueItems` compares the values as the items' rule reads them: `1` and `01` are
one integer, and `0` and `-0` are one number. The refusal prints the values as
they were given.

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
listVouchers: `since` states a rule that cannot run
  caused by: `[unterminated` is not a regular expression: Unbalanced bracket
```

A rule that cannot run refuses the document wherever its value stands, for each
value that the reduction reads for its rules: a path, query or header parameter
with a `schema` (its items, for a list; `Accept`, `Content-Type` and
`Authorization` are not parameters, as OpenAPI says), and a property of a JSON
body whose schema is an object. A body goes whole through `--json-body` at its
first property that has no flag, a nested one or one whose name has no spelling,
and a property listed after it is read for its rules all the same, so the
verdict does not depend on the order of the properties. A cookie parameter, a
parameter stated with `content`, a property of a form, a multipart or an `allOf`
body and the items of an array body are not read that way. Only the escape below
is asked of them.

**A `pattern` with `\p{`, `\P{` or `\u{` is refused the same way.** Without the
Unicode flag `\p{L}` is the text `p{L}` and `\u{41}` is the letter `u` forty-one
times, and with it they are a property and a code point. The command line and the
generated type run a pattern without the flag, and a vendor's server may run it
with, so a name such as `Müller` would be refused by one side and `p{L}` accepted
by the other. A backslash takes the character after it, so `\\p{3}` is a
backslash and then a `p`. The repair is an Overlay that states the pattern
another way, and the class is a choice the adopter makes: `[A-Za-zÀ-ÿ]` refuses
`Łukasz` and admits `a×b`, where `\p{L}` with the flag does the opposite. An
Overlay that removes the `pattern` leaves the rule to the server.

```
listPeople: `surname` states a rule that cannot run
  caused by: `^\p{L}+$` holds `\p{`, which means one thing with the Unicode flag and another without it, and a generated type cannot be given the flag
```

A flag is one place out of many, because the generated types are made of every
named schema, of an inline body and of an inline response, and each of them runs
its `pattern` without the flag too. So the refusal reads every schema that the
object model reads. A `pattern` of a value in the list above is refused as above,
with a flag or without one. A `pattern` in any other schema is refused as
`LoadError::UnrunnablePattern`: a response, a named schema, the items of an array
body, a property of a nested object, a cookie parameter, and a string whose
`enum` the flag keeps and whose `pattern` it drops. It names the operation where
the schema stands in one, and the schema as an Overlay names a target. A
component that operations reach names none, because one schema serves many
operations and the Overlay line is made at the component:

```
getPerson: `$.paths['/people/{id}'].get.responses['200'].content['application/json'].schema.properties.surname` states a rule that cannot run
  caused by: `^\p{L}+$` holds `\p{`, which means one thing with the Unicode flag and another without it, and a generated type cannot be given the flag
```

That reaches the schema of an error response and of a callback, which no type is
made of; the `pattern` there is the same disagreement, and the Overlay line that
repairs it is the one the others need. A schema that stands beside a `$ref` that
is text, in an object that can be a reference (a schema, a parameter, a header, a
request body, a response, or a callback under `components`), is not read: the
object model keeps the reference alone, as OpenAPI 3.0.3 says of the Reference
Object, so a `pattern` there is no part of the document. In any other object (a
media type, an operation, an encoding) a `$ref` is a key like any other, and the
schema beside it is read.

## Where each rule runs

A parameter and a body field take different roads, and the difference is worth
knowing before you decide how much to put in the document.

| the value | checked by | when |
|---|---|---|
| a path, query or header parameter | `Scalar::parse`, in the flag's value parser, and again in `Invocation::new`, which keeps the parsed text (`Scalar::wire`) that is sent | the command line, and every typed call |
| the values of a list parameter, taken together | `Items::check`, in `Invocation::new` | the command line, and every typed call |
| a body field, from a per-field flag | `Scalar::parse`, in the flag's value parser | the command line |
| a body field, in a generated type | the type's own `FromStr` / `Deserialize` | every typed call, and every response |
| a whole body from `--json-body FILE` | `client::fits`, called by the adopter's own seam | the command line, before the request is built |

A parameter never passes through a generated type, so `Scalar` is the only thing
that can ever check one. A body field has both roads, and the flag runs every
rule `Scalar` carries. The generated type runs fewer, as typify 0.8 writes it:

- **Both roads run** a `pattern`, which is the same expression on the same engine
  as the generated newtype's `FromStr`, `minLength`, `maxLength`, and an `enum` of
  strings, integers or numbers.
- **Both roads run an integer's range** only as far as the Rust integer that
  typify picks holds it: `u64` for `minimum: 0`, `NonZeroU64` for `minimum: 1`,
  `i8` for `-128` to `127`.
- **Only the flag runs** a boolean `enum`, `multipleOf`, a bound that no Rust
  integer holds (`maximum: 100` is an `i64`), and a bound of a `number`, which is
  an `f64`. A typed call, and `client::fits` over a whole body, accept what the
  flag refuses, and the server is the one that refuses it.
- **No road runs** `minItems`, `maxItems` and `uniqueItems` of an array in a
  body: typify writes a `Vec`, and `Items` is read for parameters only. The one
  exception is an array whose `minItems` and `maxItems` are equal and more than
  0, with a `uniqueItems` that is not true: typify writes a fixed-size array
  (`minItems: 2, maxItems: 2` is `[String; 2]`), a typed call cannot hold another
  count, and `client::fits` refuses a body list of another count as a `Mismatch`.

The roads part where a field's reading lives in a Rust type rather than in the
schema. **An integer format is one such place.** `format: int32` is a name and not
a rule, so a flag reads any `i64`, while `typify` writes `i32` for a body property
of that format (and `u64` for an integer with `minimum: 0`). The typed road cannot
hold `3000000000` in that property, and `--count 3000000000` is accepted by the
flag: only the check of the whole body (`client::fits`, on the adopter's seam)
refuses it, as a `Mismatch`. A parameter of the same format is an `i64` argument
on both roads, so nothing parts there. To have the flag refuse the value, state
the range in the schema with `minimum` and `maximum`; an Overlay does that.
`tests/generate.rs` holds the two types.

The roads part again where a field's reading lives in a Rust type of the
adopter's. A property whose `format` an adopter claims with `Settings::replace`, on
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

The last row is the one road that this crate cannot walk by itself.
`check_body` in [`src/request.rs`](../typed-openapi/src/request.rs) asks whether
a JSON body was supplied where JSON is wanted and stops there — holding the
*content* of a file to a schema needs the generated `struct`, which only the
adopter's crate can name. [`tree::select`](cli.md#mounting-the-tree) is the seam
for it, and [`examples/toy/cli/src/raw.rs`](../examples/toy/cli/src/raw.rs) is
that check in nine lines: the generated `OperationId::check_body` hands the body
and the operation's type to `client::fits`. A CLI that calls `tree::dispatch`
and nothing more has no such seam and sends the file as it is.

## What goes on the wire

**The value that was checked is the value that is sent.** `Invocation::new` keeps
each parameter as `Scalar::wire` wrote it, so a number is sent as it was parsed
and not as it was typed. A typed caller writes a number with `ToString`, and what
it wrote is sent as it came, so the command line and the typed call send one
request for one number:

| typed | sent |
|---|---|
| `+5`, `type: integer` | `5` |
| `007`, `type: integer` | `7` |
| `-0`, `type: integer` | `0` |
| `1e3`, `type: number` | `1000` |
| `1.0`, `type: number` | `1` |
| `007.25`, `type: number` | `7.25` |

A body field is the JSON value that was parsed, so `007` is the number 7 there
too, and a `number` is a float in JSON: `1e3` is `1000.0`. A text is sent as it
was typed.

`Invocation::new` refuses five more values, none of them a rule of the schema,
because the request would reach another place or carry another header than the
one the value names. A header value is refused when a receiver that follows
RFC 9110 would read another value, or another list, than the caller gave. Each
refusal names the operation and the parameter, on both roads, and none prints a
header value, which may be a credential:

| the value | variant | why |
|---|---|---|
| a path parameter that is empty, `.` or `..` | `ValueError::PathSegment` | a server or a proxy reads such a segment as a step along the path, so `DELETE /things/..` reaches the collection above. A list is judged as the segment it makes, so `..` beside another item is a name |
| a header value that `http::HeaderValue` refuses | `ValueError::HeaderValue` | a control character other than a tab. A line break in a header is how a header is injected |
| a header value, or an item of a header list, that starts or ends with a space or a tab | `ValueError::HeaderWhitespace` | a receiver does not read it as part of the value, so it reads another value than the caller gave |
| a comma inside one item of a header list | `ValueError::HeaderItemComma` | a header list is written with commas between its items and is not percent-encoded, so the receiver reads two items. A comma in a query or a path list is percent-encoded and stays one |
| an empty item of a header list | `ValueError::HeaderItemEmpty` | a receiver ignores an empty list element, so `["", "c"]` written as `,c` is read as the one item `c` |

A byte of 0x80 or above reaches the receiver as it is, and so does an empty value
that is no list, so neither is refused.

A header parameter that would claim a flag, and whose name `http::HeaderName`
refuses (`X Bad`), refuses the document: `LoadError::HeaderName`, and an Overlay
corrects the name or removes the parameter. A header parameter with no flag sends
nothing, so its name is not asked.

**The `Content-Type` is the document's.** A JSON body goes out under the
`content` key it was declared under, spelled as the document spells it, with
its parameters and less a space and a tab at its two ends:
`application/merge-patch+json` is sent as that and not as
`application/json`, and
`application/vnd.api+json; profile="https://example.com/Profiles/Timestamps"` is
sent with its `profile`. A server reads a merge patch and a JSON:API document by
that type. `Body::JsonFields` and `Body::JsonWhole` carry the key, as
`Body::Opaque` does, so the command line and the typed call send one
`Content-Type`, and a caller sets none: `Values` has no way to name a media
type.

**One key is sent, and this is the rule that picks it.** A body may be declared
under several `content` keys, and a request goes out under one. The key is the
first, in the order of the overlaid document, that names JSON (`application/json`
or a type whose subtype ends in `+json`), wherever it stands among the others.
Where none names JSON it is the first key of all, whatever it names, so a
`multipart/form-data` key is no better than another and
`application/x-www-form-urlencoded` listed before it is sent as raw bytes. An
Overlay that removes a key takes it out of the choice. `tests/media.rs` pins each
case.

The suffix is all that is read, so `application/*+json` names JSON: where it is
the first key that does, it is the key that is sent, and it is refused as a media
range (below), whatever stands beside it. After `application/json` or another key
that names JSON it is not the key that is sent and is not read.

The key that is sent is the only one the reduction reads, and so the only one it
can refuse. A key beside it is the vendor's business: it is not sent, and a
document that has one the crate would refuse still loads.

**A media range is refused.** A `content` key with a `*` in its type or in its
subtype, such as `*/*`, `image/*` or `application/*+json`, names a set of types,
and a request goes out under one. Sent as it stands the range would be the
request's `Content-Type`, which no server reads, and the crate cannot choose the
member the vendor means. The refusal is `LoadError::MediaRange`, which names the
operation and the key, when the range is the key that is sent. The repair is the
one for a key that is no media type at all, an Overlay that removes the range and
states the type the vendor's server reads
([overlay.md](overlay.md#1-plain-corrections)). Where the range stands beside a
key that names JSON and is the first to, as `application/*+json` before
`application/json`, removing the range leaves the other key to be sent. A `*` in
a parameter after the `;` is the vendor's to spell and is not read.

**A key that goes out as the `Content-Type` is one that a header carries.** A
JSON body and a body that is sent verbatim go out under the key, so the key is
read as RFC 9110 reads a header value, and one that is not is refused as
`LoadError::MediaType`, when it is the key that is sent:

- an ASCII control character other than a tab (U+0000 to U+001F and U+007F)
  anywhere in it, in a parameter as much as in the type. A byte of 0x80 or above,
  a C1 control character among them, is one a header carries;
- a white space other than a space and a tab before its type or after its
  subtype, such as a no-break space. A space and a tab there are the optional
  white space of RFC 9110 and are no fault, and the request goes out without
  them, because a field value must not start or end with one (RFC 9113,
  section 8.2.1). A space or a tab inside the key, such as before the `;`, is
  sent as the document spells it.

The rest of a parameter is not parsed. A `multipart/form-data` key is not asked
what a header carries, because that body is sent under a `Content-Type` of its
own, with the boundary that is chosen when the parts are encoded, and the key is
never sent: such a document loads. The words spell the key as the document does,
so that the target of an Overlay can match it, and write each character that
does not show as its code point (`\u{a0}`): a control character, a white space
other than a space, a format character such as a zero width space or a soft
hyphen, and a combining mark. A quote and a backslash are printed as they stand.

A base URL is the scheme, the host and the path of every request, and nothing
else of it is kept. One with userinfo (`user:password@`) or a query is refused
when a request is built (`RequestError::Base`), whoever names it: the document's
first server, `Client::with_base`, and the `base` of `Plan::build`, `Selection`
and `tree::dispatch`. Userinfo is looked for where an authority is read: in the
text up to its first `/`, `?` or `#`, and, when that character starts a `//`, in
the text after it up to the next one. So `//user:password@host/`, which the
`http` crate reads as a path, is refused as `http://user:password@host/` is. A
server URL in the document is refused when the document loads
(`LoadError::ServerBase`), with a fragment too, which an `http::Uri` cannot hold
and so only the text of a document can. A credential belongs in the client
adapter ([client.md](client.md#writing-an-adapter)). No text that the library
makes holds the userinfo of a base URL. The refusals of `RequestError::Base` and
`LoadError::ServerBase` print no part of the URL, and so a server URL that holds
userinfo is `ServerBase` also when it is no URL, whatever stands before its `//`
but a `/`, `?` or `#`. `LoadError::ServerUrl` prints the URL as the document
spells it and with the defaults in it, and both are read first. A text with an
`@` in none of the places where an authority is read, such as `http:/u@h/a b`,
stays `ServerUrl`, and its message prints it. A dry run prints the host and the
port, and the `Debug` of a `Client` and of a `Document` prints its base without
the userinfo and the query.

## What `fits` refuses

`client::fits` refuses a body three ways, each its own `client::BodyError`
variant, and a fourth variant says that it could not ask. The path is in the `jq`
spelling, and `.` is the body itself. `Undeclared` and `NotAnObject` print no
value from the body. The cause of a `Mismatch` is serde's own message, and it can
quote the value it refused, so print it only where a value in the body may be
read.

| the body | variant | a refusal reads |
|---|---|---|
| a value of the wrong type, or a missing property | `Mismatch` | `` createVoucher: the request body does not fit the schema the document declares at `status` `` |
| a property that the type does not take, at any depth | `Undeclared` | `` createContact: the request body carries a property that the schema the document declares does not take, at `address.extra` `` |
| a list where the type reads an object, at any depth, or a list or a scalar as the body itself where the type reads one | `NotAnObject` | `` createContact: the request body has an array at `address`, where the schema the document declares takes an object `` |
| a type that cannot be written back as JSON | `Unwritable` | `<operationId>: the type the document declares for the request body cannot be written as JSON` |

They are asked in this order:

1. The body itself: a list or a scalar where the type reads an object is
   `NotAnObject` at `.`. A struct, a map and an adjacently tagged union read an
   object. For an untagged, internally tagged or externally tagged union a scalar
   body that no member reads is a `Mismatch`, and so is a list that no member
   reads. A list that a member reads by position is `NotAnObject` at `.`, found in
   step 4.
2. Whether the values have the right types, and no property is missing:
   `Mismatch`. A scalar below the top where the type takes an object is one of
   these, because serde cannot read an object from it.
3. Whether the type can be written back: `Unwritable`.
4. The body beside what the type wrote back. The first place in the order of the
   body that holds either a property the type did not write back (`Undeclared`)
   or a list the type read by position (`NotAnObject`) is the one named.
5. Where that found nothing, the first property that the type skipped while it
   read is `Undeclared`. It is a property that holds a `null`, an empty list or
   an empty map, and the paragraph *One kind of property is seen only where the
   type reads its object directly* says where `fits` can see one.

Each of `Undeclared` and `NotAnObject` names one place. An operator sees the next
one when they have repaired that one.

**The measure is the type.** `fits` reads the body into the type, writes the type
back as JSON, and walks the body beside what came back. A body key that the type
did not write back is a property it did not take. A list in the place of an
object was read by position, because serde fills a struct from a list, and the
server would read a list. Both hold at every depth and inside every shape typify
writes: a union (untagged, internally tagged, adjacently tagged or externally
tagged), a `flatten`ed struct, a map of objects and a list. In an externally
tagged union the key of the member is a step of the path
(`place.address.extra`). A list at the top, where the type reads an object, is
`NotAnObject` whatever its length. Everywhere else a list that the type cannot
read is a `Mismatch`, like any other value of the wrong type. So is a scalar
below the top where the type takes an object, and a scalar as the body that no
member of an untagged, internally tagged or externally tagged union reads: those
ask for no shape, so the body itself is not asked first, and a union with a scalar
member reads a scalar.

**There is no way to allow an undeclared property on a call.** No flag, no
setting and no cargo feature does it. A server may drop a property it does not
know without a word, and then the operator believes it took effect. The repair
is an Overlay that declares the property, which an adopter blesses and a
reviewer reads: [`corrections.yaml`](../examples/toy/spec/corrections.yaml)
declares `internal_ref` that way, and
[Plain corrections](overlay.md#1-plain-corrections) shows the action.

**A schema that leaves further properties open gets a map for them.** A schema
that types its further properties is a map to typify, and the map takes them.
`setContactLabels` in the toy has `ContactLabels`, which is
`additionalProperties: {type: string}`, so
`{"vip": "yes", "region": "east"}` is sent as it is, and `{"vip": 1}` is a
`Mismatch`. `additionalProperties: true` beside declared properties is the same:
typify would write no field for it, so the generator says it as
`additionalProperties: {}` before typify reads the schema, and the type gets the
`flatten`ed map `extra`. A schema that states no `additionalProperties` is
closed: JSON Schema reads it as open, but then no body would be refused. To open
one, declare `additionalProperties` in an Overlay. `additionalProperties: false`
gets `deny_unknown_fields` from typify, so serde refuses first and the refusal is
a `Mismatch` whose cause says `unknown field`. The generator adds no
`#[serde(deny_unknown_fields)]` of its own, because the same types decode a
vendor's responses and a response may carry more than the document says. Only a
request is held to the rule.

**Where the type reads less than the document says, a body is refused like any
other, because the typed call cannot send it either.** Two shapes do that:

- An `anyOf` that typify writes as an `untagged` enum, given a body that fills
  two of its members. The enum keeps one member, so a property of another member
  is `Undeclared`. typify writes the enum when it judges the members exclusive.
- A `oneOf` of an object and a list, given a list that the object reads by
  position. The body is `NotAnObject`.

The repair is an Overlay that makes the schema say what the type reads: one
member, or the properties declared on one schema.

An `anyOf` whose members typify cannot judge exclusive, such as objects with
nothing required, is a struct that takes the properties of every member, and a
body that fills two members fits. In that struct a value of the wrong type for a
member is `Undeclared` and not a `Mismatch`: serde reads a member that fails as
absent, so the key is one the type did not write back.

**One kind of property is seen only where the type reads its object directly.**
A type leaves a `null`, an empty list and an empty map out of what it writes when
the property is optional, which is what typify's `skip_serializing_if` does. So a
property that nothing declares and whose value is one of those three is found only
where the type skips it while it reads. The refusal names the place the read was
at, and an externally tagged union puts the key of its member in it.

Serde reads some objects from a buffer, and nothing is skipped from a buffer: an
untagged union, an internally tagged union, a `flatten`ed struct, and the content
of an adjacently tagged union when it comes before the tag. A property of that
kind passes inside one of them. It passes at the top as well when the type of the
body itself is an untagged union, an internally tagged union or a `flatten`ed
struct, because every property of the body is then read from a buffer. Everywhere
else it is refused: in a struct, in an externally tagged union, in the content of
an adjacently tagged union whose tag comes first, and among the own keys of an
adjacently tagged union, which serde reads directly whichever key comes first.
`{"kind": "a", "data": {"x": 1, "extra": null}}` is refused, and
`{"data": {"x": 1, "extra": null}, "kind": "a"}` passes. A `zzz` of that kind
beside `kind` and `data` is refused in both orders. A property with any other
value is found everywhere.

**A type you own is written back too.** `fits` needs the type to implement
`Serialize`, and the type has to write the shape it reads: an object as an object
and a list as a list. A generated type does, and so does a type named in
`Settings::replace` that reads a string and writes one. A type that cannot be
written back is `Unwritable`.

Every keyword that describes a shape no flag carries is not read at all:
`minProperties`, and `minItems` on the array of a body, belong to bodies, which
go through a file.

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

It also puts a floor under the compiler: `regress` uses let-chains, which need
Rust 1.88. CI builds on current stable and on nothing older.

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
no document, no reader and no second pass. A guard written over the answer is
general — it covers the field the vendor adds next revision, because the
document is what names it — where a compiled-in list of field names protects
whatever it was written against and nothing else.

The halves are yours too, for a guard that means only one of them:
`Operation::params` with `Param::format`, and `Body::fields` with
`Field::format`. `Body::fields` is the door to a flat JSON body's properties
generally — walk it to render a body, to document one, or to check one — and it
answers with nothing for every body that offers no per-property flag, so the
walk needs no match over the body kinds and covers one added later.

Prefer `carrying` when the question is about the operation. Chaining the two
halves by hand gets the same answer, and a guard that chains only the first gets
a shorter one without saying so: the parameters pass, the body fields were never
looked at, and nothing about the result reads as incomplete.

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
