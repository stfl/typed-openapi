# Adopting a document you did not write

The library refuses what it can refuse. A `pattern` no engine runs, a `content`
key that is not a media type, two operations reducing to one command name: each
is a `LoadError` naming the operation and the thing that could not be read, at
the moment the document is reduced. This page is the residue — five judgement
calls no check reaches, because each is a claim about an API nobody has called
yet. Read it if you are pointing this crate at a vendor's document rather than
at the toy.

The primer is [the README](../README.md); the mechanism the decisions below are
written in is [docs/overlay.md](overlay.md), and what fails when one of them
stops fitting is [docs/drift.md](drift.md).

## Contents

- [Route an action by its audience, never by keyword](#route-an-action-by-its-audience-never-by-keyword)
- [A narrowing refuses reads as well as writes](#a-narrowing-refuses-reads-as-well-as-writes)
- [A vendor may state one rule twice and disagree with itself](#a-vendor-may-state-one-rule-twice-and-disagree-with-itself)
- [A documentation site may be a view of the document you hold](#a-documentation-site-may-be-a-view-of-the-document-you-hold)
- [Evidence belongs in the action, with its date](#evidence-belongs-in-the-action-with-its-date)
- [Prior art, and one hazard in it](#prior-art-and-one-hazard-in-it)
- [Before you commit a correction](#before-you-commit-a-correction)

## Route an action by its audience, never by keyword

**The question that sorts an action is *who else could use this?*, and it is the
only question that sorts it.** What is true of the API is a correction anyone
consuming the document could use. What is true of your client alone is a
narrowing. [overlay.md](overlay.md#layers) has the three layers and what each
one holds; the part that is judgement is which of them an action belongs in.

The failure mode is a rule that can be applied without reading the action.
"Anything about an `enum` goes in corrections" sorts these two together, and
they belong apart:

```yaml
# spec/corrections.yaml — true of the API
  - target: $.components.schemas.VoucherStatus
    description: The server returns `void` for a reversed voucher; the enum omits it.
    update:
      enum: [void]
```

```yaml
# spec/client.yaml — true of this client
  - target: $.components.schemas.VoucherStatus.enum
    description: This client never sets a voucher to `paid`; the server does that.
    remove: true

  - target: $.components.schemas.VoucherStatus
    description: The two statuses this client sets.
    update:
      enum: [draft, open]
```

One schema, one keyword, opposite audiences. The tell is not the word `enum`; it
is the direction of the claim. The first says the API does something the
document omits, and a TypeScript generator, a mock server and a request
validator all want it. The second says this client does less than the API
allows, and handing it back would be a bug report about an API behaving
correctly.

The mechanism leans the same way, which is a hint rather than a check: adding a
value is one merging action, and taking one away is two — a `remove` and an
`update` that writes the shorter list back, because [a correction adds and
cannot subtract](overlay.md#a-correction-adds-it-cannot-subtract). A layer whose
actions are all merges is usually a layer of corrections.

**What routing by keyword costs is the layer, not the action.** The vendor's
document plus `corrections.yaml` *is* the document the vendor should have
shipped, and any Overlay 1.1 implementation produces it. Mix one narrowing in
and the file stops being that, permanently: a year later nobody can tell which
rows are the vendor's mistakes and which are your house style, because what
separated them was in the author's head and never written down. Two files are
cheap; unmixing one is not.

A test reaches only the edges of this. The example adoption holds its layers to
one property — that no `x-cli-` key reaches the layer meant for anyone
([overlay.md](overlay.md#corrections-and-the-test-that-holds-it)) — and audience
is a fact about intent, which no file knows.

## A narrowing refuses reads as well as writes

**A narrowing is not a restriction on what you send. It is a claim about what
exists.** The schema that describes what you send is usually the schema that
describes what comes back, so a value you never send is still a value you have
to decode. The first adoption of this crate hit this three times in one day,
from three directions.

The narrowing above is filed in the right layer, and the reasoning behind it is
sound the whole way. Your client creates vouchers and opens them; the server
marks one `paid` when the money arrives, and your client never sends that value.
So you say so, in the layer for what is true of your client. What that buys is a
refusal at the flag for a value you were never going to type. What it costs
arrives on a read:

```
the response is not the shape the document promises: unknown variant `paid`,
expected `draft` or `open`
```

That is an ordinary listing against an ordinary tenant, failing on a voucher
somebody paid. The message blames the response, and the response is right: the
document promises the wrong shape because you narrowed it. Nothing you sent was
involved.

**The question to ask of every narrowing is which way the value travels.** A
page size, a sort key, a filter you construct: outbound only, and a narrowing on
one is a rule about your own behaviour. A schema reachable from a response is
not — there a narrowing is a claim about every record that exists, including
records written before your client existed, by other clients, on tenants you
have never seen. No amount of care about what you send touches it.

Reachability is the part that hides. In this repository's own toy document,
`listVouchers`'s `status` filter and `Voucher.status` are one `$ref` to one
named schema, so the generated wrapper takes the narrowed type as an argument
*and* returns it inside every voucher:

```rust,ignore
pub fn list_vouchers(
    &self,
    status: Option<crate::types::VoucherStatus>,   // outbound
    …
) -> Result<Call<'_, Vec<crate::types::Voucher>>, Error>  // and inbound
```

The target you wrote names one node. The `$ref` decides how many it reaches, and
recursive descent reaches more still. A narrowing on a named schema is a
narrowing everywhere that name is used.

Two ways out, each with a price. Narrow where only your outbound value meets the
rule — the parameter's own schema rather than the name it references — which
detaches the filter from the named schema, so the argument and the field stop
being one type. Or keep the rule out of the document and refuse the value in
your own code, where being wrong refuses a request rather than a record. The
narrowing worth having is the one on a value that only ever leaves — a
`maximum: 100` on a page size is safe for exactly that reason.

No check here could stand in for the question. Reachability is computable from
the document; which action is a narrowing is not, because a narrowing and a
correction are the same YAML.

## A vendor may state one rule twice and disagree with itself

**Where a document states a set twice, neither statement is authoritative until
something measures it.** A correction written from the schema alone is a guess
wearing the clothes of a fact.

A document states the same thing twice in more ways than one. An `enum` and a
`pattern` on a single field. A `description` that lists the values. An `example`
beside a schema it does not satisfy. A `maxLength` beside a `pattern` carrying a
different length. Two named schemas for one concept. Each is a place the
document cannot be its own evidence, and a field carrying a ledger period is the
shape the first adoption met:

```yaml
period:
  type: string
  enum: ["01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12"]
  description: >
    The accounting period this voucher books into: 1 to 12 for a calendar
    month, 13 for the year-end adjustment.
```

The two statements disagree on membership and on spelling. The `enum` is the
machine-readable one, which is exactly what makes it look like the answer — and
in the case this is drawn from, the plain spelling the prose gives is the one
the live API accepts. A correction that points the field at a named schema
carrying `pattern: ^(0[1-9]|1[0-2])$` refuses every ordinary monthly value on
every tenant, and refuses it twice over: the command line runs the document's
rule and so does the generated newtype
([validation.md](validation.md#where-each-rule-runs)). One rule on both roads is
what you want, and it is what makes a guess expensive.

**Measuring it means a call, and a read is worth more than a write.** A server
that accepts a value may normalise it, so a 2xx says less than a record you did
not write coming back with the spelling on it. Send one of each against a
sandbox tenant, then fetch something ordinary and read what it carries. Prose
the vendor publishes outside the document is a third statement worth having, and
whether any exists is
[a question with a cheap answer](#a-documentation-site-may-be-a-view-of-the-document-you-hold).

Where you cannot measure, state the wider set or state nothing. The asymmetry is
the whole argument: a rule that is too wide costs you a check you wanted, and a
rule that is too narrow costs you the records. An `enum` naming both spellings
refuses nothing that works. The vendor's document unchanged already admits
whatever it admits, and the correction is the thing that has to be justified.

## A documentation site may be a view of the document you hold

**Whether the vendor's published documentation adds anything to the file you
already have is a question with a cheap answer, and the cheap answer is a
hash.** A documentation site that renders in the browser fetches the document it
draws, and that request is in the page's network log. Fetch it, hash it, and
hash the copy you vendored:

```sh
# the document the site loads, twice, past any cache — and the copy you hold
for _ in 1 2; do
    curl -fsS -H 'Cache-Control: no-cache' "$SPEC_URL?cb=$(date +%s%N)" | sha256sum
done
sha256sum spec/vendor.yaml
```

Three matching hashes say two things at once: the site is a renderer over the
document you already have, and the origin served those bytes rather than a cache
between you and it. Running it on the first adoption of this crate returned
three matches, so *read the vendor's documentation* and *read the OpenAPI
document* were one instruction — every hazard the site might have explained sat
in a file that can be searched rather than browsed.

**The method is the part that transfers, not the finding.** The weaker check is
to skim the site for an afternoon and conclude that it adds little. That
conclusion cannot be wrong, because no observation would have refuted it, and an
unfalsifiable check is worse than none: it retires the question while leaving it
open. A hash either matches or it does not.

Which is why the finding does not generalise into *vendor prose is worthless*.
[A vendor may state one rule twice](#a-vendor-may-state-one-rule-twice-and-disagree-with-itself)
turns on a case where the prose is the only statement the live API honours, and
a matching hash is what puts that prose in your hands rather than on a site: the
sentence that settles it sits in a `description` you can grep. A hash that
*differs* is the other finding and just as useful — the site carries statements
the document does not, and each is a candidate answer to the question that
section leaves open. Which of the two you are looking at is the thing to
establish before deciding how much reading the site is worth.

A hash holds for the day it was taken. A site can gain a page the document never
gains, so the answer wants a date on it like any other measurement. And it says
nothing about the vendor's other channels: a support desk, a changelog and a PDF
somebody emails are each a source, and none of them is hashed by this.

## Evidence belongs in the action, with its date

**A correction states what was measured and no more; where it rests on an
inference rather than a measurement, it says so.** An Overlay action carries a
`description`, and it is the one field in an action that nothing reads — which
is what makes it the right place for the thing no machine will check.

The example adoption's own line is enough for a toy and not enough for a vendor:

```yaml
    description: Add the undocumented `internal_ref` field.
```

The next reader cannot act on it. Was the field observed, or inferred from a
name in a support reply? Against which tenant? Over how many records? The same
action, with what was measured in it:

```yaml
  - target: $.components.schemas.Voucher.properties
    description: >
      Add the undocumented `internal_ref`. Observed on every voucher returned by
      the list and fetch operations, sandbox tenant, 2026-03-11. The server
      writes it and ignores it on input — sent a voucher with it set, and it
      came back carrying the server's value.
    update:
      internal_ref:
        type: string
```

**What the date buys is the next decision, not this one.** A year on, somebody
has to decide whether the correction still holds. The
[tripwire form](overlay.md#the-tripwire-form) tells them the vendor has not
moved the thing being corrected; it cannot tell them whether what the correction
*asserts* is still true, because the assertion is about the server and the
tripwire only ever reads the document. The date turns *is this still true?* into
*was this measured recently enough to trust?* — a question someone can answer
without redoing the work.

The other half is marking the inference. Three readings sit behind corrections
that look identical on the page — **observed**, **stated by the vendor** outside
the document, and **inferred** — and only the third needs testing first. The
content-type repair in [overlay.md](overlay.md#1-plain-corrections), where the
vendor writes `form-data` and means `multipart/form-data`, is an inference until
somebody sends a body and gets a 2xx. Written as one, it tells the next reader
where to start.

Nothing here is checked. `just gate` passes on a page of confident guesses, and
the test holding every row of `CORRECTIONS` to both documents
([drift.md](drift.md#test-time-the-committed-artefacts)) compares a document
with a document — neither of them is the server. An action carrying no evidence
is not wrong; it is indistinguishable, a year later, from one that was measured.
That is the cost.

## Prior art, and one hazard in it

[progenitor] is the prior art for this shape — an OpenAPI document in, typed
Rust calls and a clap tree out — and it has been carrying real documents for
years. Several questions it answers well are answered again here, for one narrow
reason: a command line in this crate is decided while the document is reduced
and travels to a shipped binary as a blob, so the binary derives nothing and the
generated types are not in the picture. Asking the generated types is the better
instinct wherever they are available, and here they are not. What progenitor
does with an error response is worth copying outright: the typed body of a 4xx
is kept rather than discarded, which is the rule an adapter here is held to
([client.md](client.md#writing-an-adapter)).

The hazard is one method. `Error::is_retryable()` answers `true` for every
communication error — in progenitor 0.14, `Error::CommunicationError(_) => true`
with no further question — alongside 429, 502, 503 and 504. A connection reset
or a timeout is two events wearing one error: the request never left, or it left
and the server may have done everything. For an idempotent read the distinction
does not matter and the default is a convenience. On a write it is the
difference between an operation that did nothing and one that did all of it, and
the method name says nothing about which. (The status half carries the same
ambiguity in miniature: a 429 is genuinely safe to repeat, and a 504 is a
gateway giving up on an upstream that may well have finished.)

**The rule worth carrying: a retry predicate that does not distinguish the two
silences is a write hazard.** Ask it of every client you adopt, because a
predicate you get for free is answering a question you did not ask, and the
distinction has no short name to give it away. This crate declines to answer —
it sends nothing, so it has no far side to read — and the reading belongs in the
crate that wrote the adapter, where a failure nobody has classified counts as
one that may have arrived: [client.md](client.md#classifying-a-transport-failure).

## Before you commit a correction

Seven questions. None of them has a checker.

- **Who else could use this?** Anyone, and it is a correction; only your client,
  and it is a narrowing. The answer decides the file.
- **Is the schema this edits reachable from a response?** Follow the `$ref`s
  both ways. If it is, a narrowing is a claim about every record that exists.
- **Does the document state this rule anywhere else?** A description, an
  example, a second keyword. If the statements disagree, which one did you
  measure?
- **Is the vendor's published documentation a second source, or a view of the
  file you hold?** A hash answers that in minutes. An impression never answers
  it, and retires the question all the same.
- **What did you measure, when, and against what?** If the answer is nothing,
  does the `description` say so?
- **Would this correction still be right if the vendor's prose were absent?** If
  it only makes sense as a reading of the vendor's wording, it is an inference —
  is it marked as one?
- **If it is wrong, what fails and who sees it?** A wrong rule on an outbound
  value is refused at a flag in front of you. A wrong rule on an inbound one is
  a decode failure on data you did not create, in front of somebody else.

Five mistakes and seven questions out of one adoption, which is not a taxonomy.
What they have in common is the only general thing here: the document is
evidence about the API and is not the API, and every correction is a bet on the
difference.

[progenitor]: https://docs.rs/progenitor
