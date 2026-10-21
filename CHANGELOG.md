# Changelog

Every change an adopter of `typed-openapi` can see after 0.0.2 is listed here,
with the repair it asks for. The format follows [Keep a Changelog][keep], and
the crate follows [Semantic Versioning][semver].

[keep]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## Unreleased

### Added

- **A stamp on the model blob, and `Document::MODEL_LAYOUT`.** `to_blob` writes
  a header before the encoding: a magic of 8 bytes, the layout number as a
  `u32`, and the version of typed-openapi that wrote it. `MODEL_LAYOUT` is the
  layout this build reads, 1 in this release, and it moves with any change to
  what the blob holds. `from_blob` reads the header first, so a blob from
  another build fails with a message that says what to do and never decodes into
  a wrong model. All of its errors say to run the bless step again:
  - `DocumentError::Unstamped { reads }`: the bytes carry no stamp, so they are
    an empty file, a blob of typed-openapi 0.0.2 or older, or no model blob. It
    names the layout this build reads.
  - `DocumentError::Stale { layout, version, reads }`: the stamp names another
    layout. It names that layout, the version that wrote the blob and the layout
    this build reads.
  - `DocumentError::Blob` and `DocumentError::Trailing { bytes }`: the stamp is
    this build's and the blob is damaged. In `Blob` the bytes after the stamp do
    not decode, and its message is worded anew (see *Changed*). In `Trailing` the
    bytes decode and more bytes follow the encoding, and the message counts them.
- **A decoded blob is checked again for what a load checks.** `from_blob`
  refuses a confirmation word that `Loading::commit` refuses. In each operation
  it refuses a flag that is not spelled as a load spells it, a flag that every
  subcommand spends (`help` and the five flags that carry a body), two
  declarations that take one flag (any two of a parameter, a body property, a
  gate and, on a write, the confirmation word, and a gate named twice), a gate
  on a read, a method that writes held as a read, a group or a command called
  `help`, and a `requires` that names a parameter the operation does not declare
  or one that has no flag. It refuses two operations under one
  `<group> <command>`. A blob like that used to decode. In a debug build of the
  binary, clap panicked at start-up on a flag declared twice, on a flag or a
  confirmation word that begins with `-`, on `help` or a body flag that the
  subcommand declares already, on two operations under one name and on a group
  or command called `help`; in a release build one declaration took the place of
  the other. A write held as a read, a `GET` that names a gate among them, was
  sent without `--commit`. The other refusals stopped no build.
  Eleven variants carry these refusals: `DocumentError::{Confirmation,
  OperationConfirmation, Flag, FlagSpent, FlagTwice, GatedRead, WriteAsRead,
  NameSpent, DuplicateCommand, RequiresUnknown, RequiresUnreachable}`. Nine name
  the operation, `DuplicateCommand` names both of two, and `Confirmation` names
  the document's confirmation word. Repair: run the bless step again. A blob that
  a load wrote never meets them. Only `from_blob` applies these rules; a
  `Document` read through `Deserialize` in another format is not held to them.
- **`OverlayError::Shape`.** An Overlay that is YAML or JSON and has the wrong
  shape (no `actions`, an action with no `target`, a version other than 1.1) is
  `Shape`. It was `Syntax`, whose message said the text "is not valid YAML or
  JSON". Repair: where you reported `Syntax` for a wrong shape, report `Shape`
  too.
- **`overlay::Cause`**, the payload of every `OverlayError` variant: the
  `message()`, and the `place()` where the thing that refused said one. An
  `overlay::Place` is the line and column of the text, or the index of the
  action in the Overlay.
- **A body property that no schema declares stops the request.** `client::fits`
  refuses a body that carries a property which the target type does not take, at
  any depth and inside a union or a `flatten`ed struct, as
  `BodyError::Undeclared { op, path }`. It runs before a request exists, so a dry
  run and a confirmed run both stop, and it names the operation and the path in
  the `jq` spelling (`address.extra`, and `place.address.extra` for an externally
  tagged union) and never prints the value. The generated
  `OperationId::check_body` calls it, so a CLI with the seam of `toy raw` refuses
  the body and sends nothing. A property that a map admits
  (`additionalProperties: {type: string}`, `{}` or `true`) passes. The generated
  types do not get `deny_unknown_fields`, because they decode responses too.
  There is no flag or setting that allows an undeclared property on a call.
  Repair: an Overlay that declares the property, then the bless step
  (`docs/overlay.md`, *Plain corrections*). A committed `--json-body` file with
  vendor extras fails now, and the repair is the same. A schema that states no
  `additionalProperties` is closed. What the check does not see is in
  `docs/validation.md`, *What `fits` refuses*. The crate depends on
  `serde_ignored` for this.
- **`fits` takes a type that implements `Serialize`.** It writes the value back
  and walks the body beside it, which is how it sees inside a union. The
  signature is `fits<T: DeserializeOwned + Serialize>`. Every type the generator
  writes has both, and so has a type named in `Settings::replace` when it can be
  written as JSON. A type that cannot is `BodyError::Unwritable { op, source }`.
  Repair: derive or implement `Serialize` on a type of yours that a body reaches.
- **`BodyError::NotAnObject { op, path, found }`.** `fits` refuses a list or a
  scalar where the type takes an object, at any depth: `path` is `.` for the body
  itself and `address` or `positions[1]` below it. A list of the right length
  used to pass, because serde fills a struct from a list by position. Below the
  top, a list that the type cannot read is a `Mismatch`. Repair: send an object.
- **A body that the type reads as less than the document says is refused.** Two
  shapes: an `anyOf` whose members one body fills together (typify writes an
  `untagged` enum, which keeps one member, so a property of another member is
  `Undeclared`), and a `oneOf` of an object and a list given a list that the
  object reads by position (`NotAnObject`). The typed call cannot send such a
  body either. Repair: an Overlay that makes the schema say what the type reads.
- **`ArgError::FlagBesideNonObject { flag, path, found }`.** A per-field flag
  beside a `--json-body` file whose top level is not an object is refused. It
  was dropped, and the file went out as it was. Repair: drop the flag, or send an
  object.

### Changed

- **Every adopter blesses again after the upgrade.** A blob written by 0.0.2
  has no stamp, so `Document::from_blob` returns `DocumentError::Unstamped` and
  the generated `Api::new()` fails with it. Repair: run your bless step and
  commit `src/model.postcard`. The stamp holds the version of typed-openapi, so
  the blob changes with each release even when the layout does not; a check that
  blesses again and compares asks for a commit after each upgrade.
- **`additionalProperties: true` beside declared properties generates the
  map.** The generator says it as `additionalProperties: {}` before typify reads
  the schema, at any depth, so the struct gains
  `#[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>`, as
  typify writes for `{}`. typify wrote no field for `true`, which would have made
  a property the document leaves open an undeclared one. A struct literal of such
  a type must add `extra`, and a struct whose other properties were all optional
  no longer derives `Default`. A schema that states no `additionalProperties` is
  unchanged, and so is `false`. Repair: run the bless step again and add the
  field to a literal of the struct.
- **`DocumentError::Blob` is worded anew, and `DocumentError::Encode` is the
  failure to write one.** At 0.0.2 `Blob` read `the reduced model the bless step
  writes is not valid: <cause>`. It reads `the model blob is damaged; run the
  bless step again`, and the `postcard` error is its `source()`. `to_blob` used
  to return `Blob` when `postcard` could not write the model; it returns
  `Encode`, whose message is `the model could not be written as a blob`.
  `DocumentError` no longer implements `From<postcard::Error>`, so a `?` on a
  `postcard` error does not turn into `Blob`. Repair: match `Encode` where a
  bless step handled `Blob` from `to_blob`, and write
  `map_err(DocumentError::Blob)` where your own code read a blob with `postcard`
  and wanted `Blob`.
- **The YAML reader is serde-saphyr 2.0 in place of `serde_yaml_ng`.** Every
  document and every Overlay reads into the same value as before, and `yes`,
  `no`, `on`, `off`, `y` and `n` stay strings, as does `<<` as a key. Five
  things differ:
  - A mapping that repeats a key is refused, with the line and column of the
    second one. The last value used to win in silence. Repair: delete the key
    you do not mean.
  - `1_000` reads as the number 1000 and `017` as the float 17. Both were
    strings. Repair: quote the value where it is a string.
  - `.inf` and `.nan` are refused where they stand. They were `null`.
    Repair: quote the value or remove it.
  - The messages for a text that is not YAML are the new reader's words, and
    each names its line and column.
  - A value past the reader's budget is refused: more than 2 500 000 nodes or
    64 MiB of scalar text, each counted with every alias expanded. So are
    anchors nested inside each other that keep more than 10 000 000 events or
    64 MiB between them. The old reader read documents that the new one
    refuses, among them 3 000 000 scalars (a JSON array of 3 000 000 single
    digits is 6 MB), an anchor of 10 000 items used by 1 000 aliases
    (10 000 000 leaves), a 1 MB string used by 100 aliases, and five anchors
    nested around 2 000 000 items (28 MB). It refused an alias document of
    100 000 leaves, which the new reader reads. There is no repair: the limits
    are fixed, and the read comes before the first Overlay, so no Overlay can
    shrink a document that reaches one.

  The `document` feature names `serde-saphyr` where it named `serde_yaml_ng`.
  Repair: a test of yours that read YAML with `serde_yaml_ng` names its own
  dependency, or calls `overlay::parse`.
- **No type of the YAML reader, of `roas-overlay` or of typify is in a public
  signature.** Each `OverlayError` variant holds an `overlay::Cause`.
  `GenerateError::Yaml` and `GenerateError::Typify` hold a
  `Box<dyn Error + Send + Sync>`, and the cause is still `source()`. Repair: a
  `match` that bound `serde_yaml_ng::Error`, `roas_overlay::validation::Error`,
  `roas_overlay::apply::ApplyError` or `typify::Error` reads the `Cause`, or
  prints `source()`.
- **A bless step may write the overlaid YAML with different text.** The emitter
  double-quotes every string a YAML 1.1 reader would misread (`"N"`, `"yes"`,
  `"1.0"`) and folds long strings into `>-` blocks. The value is the same, read
  as JSON. Repair: run the bless step again and commit the result.
- **`rust-version` is 1.89**, the floor serde-saphyr declares. CI builds and
  tests the crate on current stable. Repair: build with Rust 1.89 or newer.
- **An error describes its own layer and leaves the cause to `source()`.** Every
  `#[error]` string that printed the `#[source]` or `#[from]` field it also
  marks stops where its own layer stops, so a walk down the chain (the toy's
  `main` and bless step, sevdesk-cli's `error::chain`, anyhow's `{:#}`) prints
  each cause once. Repair: walk `source()` wherever an error is reported, and
  read the cause in a test with `error.source()`; printing `{error}` alone no
  longer shows it. Most messages lose only the `: <cause>` they ended with:
  `OverlayError`, `PlanError::Request`, `ArgError`,
  `client::Error::{Document, Request, Encode, Decode}`,
  `LoadError::{Shape, ServerUrl}` and `GenerateError::{Yaml, Unusable,
  NotOpenApi, Schema, Typify, NotRust, Blob}`. These are worded anew:
  - `ValueError::BadValue`: ``<op>: `<name>` was given a value the document
    refuses``
  - `LoadError::Unresolvable`: ``<op>: `<name>` has a `$ref` that cannot be
    followed``
  - `LoadError::Unrunnable`: ``<op>: `<name>` states a rule that cannot run``
  - `client::Error::Transport` and `DispatchError::Transport`: `the transport
    failed`
  - `GenerateError::Read`, `Write` and `RustfmtSpawn`: `cannot read <path>`,
    `cannot write <path>` and `cannot run rustfmt on <path>`
  - `GenerateError::Overlay`: `cannot use <path>`
  - `GenerateError::Operation`: `the wrapper for <op> cannot be generated`
- **Eight error enums are `#[non_exhaustive]`:** `LoadError`, `ValueError`,
  `client::Error`, `DispatchError`, `ArgError`, `PlanError`, `OverlayError` and
  `DocumentError`. A `match` on one needs a `_` arm, and a refusal added later
  is a new variant, not a break. A match on an error is the same in every
  feature build, `_` arm included.
- **`client::BodyError` is an `#[non_exhaustive]` enum.** At 0.0.2 it was a
  struct with `op`, `path` and `source`. Those fields are the variant
  `BodyError::Mismatch { op, path, source }`, with the same message, beside
  `Undeclared`, `NotAnObject` and `Unwritable`. Repair: match the variant where
  you read the fields, with a `_` arm for a refusal added later.

### Removed

- **Items that no caller outside the crate used are private to it.** No
  replacement is offered:
  - the `schema` module (`resolve`, `resolve_schema`, `scalar_of`,
    `description_of`, `format_of`, `template`, `is_media_type`, `is_json` and
    `is_multipart`); `schema::RefError`, which `LoadError` carries, is
    `typed_openapi::RefError`
  - `names::Grouping`, `names::Namespace`, `names::Protected` and
    `names::Clash`
  - `scalar::Numeric`
  - the `multipart` module (`encode` and `Encoded`)
  - `kebab`, at the root and in `names`

### Security

- **The dependency tree holds no RUSTSEC advisory.** `unsafe-libyaml`
  (RUSTSEC-2024-0449, unmaintained) leaves it with `serde_yaml_ng`.
