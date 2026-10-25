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
  layout this build reads, and it moves with any change to what the blob holds.
  `from_blob` reads the header first, so a blob from another build fails with a
  message that says what to do and never decodes into a wrong model. All of its
  errors say to run the bless step again:
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
  `help`, and a `requires` that names its own parameter, a parameter the
  operation does not declare or one that has no flag. It refuses two operations
  under one `<group> <command>`. A blob like that used to decode. In a debug
  build of the binary, clap panicked at start-up on a flag declared twice, on a
  flag or a confirmation word that begins with `-`, on `help` or a body flag
  that the subcommand declares already, on two operations under one name, on a
  group or command called `help` and on a parameter that requires itself; in a
  release build one declaration took the place of the other. A write held as a
  read, a `GET` that names a gate among them, was sent without `--commit`. The
  other refusals stopped no build. Twelve variants carry these refusals:
  `DocumentError::{Confirmation, OperationConfirmation, Flag, FlagSpent,
  FlagTwice, GatedRead, WriteAsRead, NameSpent, DuplicateCommand,
  RequiresItself, RequiresUnknown, RequiresUnreachable}`. Ten name the
  operation, `DuplicateCommand` names both of two, and `Confirmation` names the
  document's confirmation word. Repair: run the bless step again. A blob that a
  load wrote never meets them. Only `from_blob` applies these rules; a
  `Document` read through `Deserialize` in another format is not held to them.
- **`OverlayError::Shape`.** An Overlay that is YAML or JSON and has the wrong
  shape (no `actions`, an action with no `target`, a version other than 1.1) is
  `Shape`. It was `Syntax`, whose message said the text "is not valid YAML or
  JSON". Repair: where you reported `Syntax` for a wrong shape, report `Shape`
  too.
- **`overlay::Cause`**, the payload of every `OverlayError` variant but
  `OpenApiVersion`: the `message()`, and the `place()` where the thing that
  refused said one. An `overlay::Place` is the line and column of the text, or
  the index of the action in the Overlay.
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
  `docs/validation.md`, *What `fits` refuses*.
- **`BodyError::NotAnObject { op, path, found }`.** `fits` refuses a list where
  the type reads an object, at any depth, and a list or a scalar as the body
  itself where the type reads an object (a struct, a map or an adjacently tagged
  union): `path` is `.` for the body and `address` or `positions[1]` below it. A
  list of the right length used to pass, because serde fills a struct from a list
  by position. Below the top, a list that the type cannot read and a scalar are a
  `Mismatch`. So is a scalar as the body that no member of an untagged,
  internally tagged or externally tagged union reads, and so is a list that no
  member of one reads. Repair: send an object.
- **A body that the type reads as less than the document says is refused.** Two
  shapes: an `anyOf` that typify writes as an `untagged` enum, given a body that
  fills two members (the enum keeps one member, so a property of the other is
  `Undeclared`), and a `oneOf` of an object and a list given a list that the
  object reads by position (`NotAnObject`). The typed call cannot send such a
  body either. An `anyOf` that typify writes as a struct of `flatten`ed
  `Option`s takes the properties of every member. Repair: an Overlay that makes
  the schema say what the type reads.
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
  signature.** Each `OverlayError` variant but `OpenApiVersion` holds an
  `overlay::Cause`. `GenerateError::Yaml` and `GenerateError::Typify` hold a
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
  `LoadError::ServerUrl` and `GenerateError::{Yaml, Schema, Typify, NotRust,
  Blob}`. These are worded anew:
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
  - `GenerateError::Unusable`: `the document does not describe a usable CLI`.
    The vendor's `openapi` is asked before the first layer, so "overlaid" is not
    true of every cause.
- **A refusal names its operation and the place it stopped.** A document of tens
  of thousands of lines gave a message with no pointer to where to look. These
  payloads and messages change:
  - `LoadError::Shape` is `Shape { at, source }`, and `GenerateError::NotOpenApi`
    is `NotOpenApi { at, source }`. At 0.0.2 each held a `serde_json::Error` and
    said nothing of a place, because the OpenAPI model drops the path inside
    `paths`. `at` is the first node that the model refuses. For a fault inside
    `paths` the document is read again one path item at a time, in the order it
    lists them, and `at` is the item and the node inside it: `paths[/b]` for an
    item that is no object, `paths[/b].get.deprecated`, or
    `paths[/b].parameters[0]`. For a fault anywhere else `at` is the place the
    model gives, such as `info.title` or `components.schemas.Voucher`, or `.`
    for the document itself. The message is
    ``the overlaid document is not an OpenAPI 3 document: refused at `<at>` ``,
    and serde's reason is the cause. Repair: match `Shape { .. }` and
    `NotOpenApi { .. }`, and print `at`.
  - `LoadError::Overlay` is the failure to read the vendor's document. An Overlay
    layer that fails is `LoadError::Layer { layer, source }`, with the index of
    the layer in the `overlays` you gave `Document::load`, counted from 0. Its
    message is ``cannot use `overlays[<layer>]` ``. Repair: match `Layer` where
    you matched `Overlay` for a layer that did not apply.
  - `LoadError::Name(NameError)` is `Name { op, source }` for a group or a
    command. Its message names the operation and the repair:
    ``<op>: no command name can be read off this operation; name it with
    `x-cli-group` and `x-cli-command` in an Overlay``. A gate that cannot be
    spelled as a flag is the new `GateName { op, source }`, whose repair is
    `x-cli-gates`. The root path `/` has no segment, so its cause reads
    ``the path `/` does not kebab-case into [a-z0-9-]`` and not "the path
    segment". The message of `GateName` is ``<op>: a gate in `x-cli-gates` is
    not a word that can be a flag; correct it in an Overlay``. Repair: match
    `Name { .. }` and `GateName { .. }` where you matched `Name(_)`. To mend the
    document, write the two markers on the operation, or correct the gate, in an
    Overlay.
  - `LoadError::Reference(RefError)` is gone. A path item that is a `$ref` is
    `PathItem { path, reference }`, and a parameter whose `$ref` cannot be
    followed is `ParameterRef { op, source }`. A path item said
    `` `paths[/a]` does not resolve ``, a place the document does not spell,
    and said it of a reference that resolves as well. This crate follows no
    path item's `$ref`, so `PathItem` holds the reference and no cause. Its
    message is ``<path>: the path item is the `$ref` `<reference>`, which this
    crate does not follow``. The message of `ParameterRef` is ``<op>: a
    parameter is a `$ref` that cannot be followed``, and its cause is the
    `RefError`. Repair: match the two new variants.
  - `LoadError` no longer implements `From<OverlayError>`, `From<NameError>` or
    `From<RefError>`. A `?` that turned one of them into a `LoadError` does not
    compile. Each of the three is now one variant among several, and the
    variant says which document, layer or operation it came from. Repair:
    build the variant with `map_err`: `LoadError::Overlay` for the vendor's
    document, `Layer { layer, source }` for an Overlay layer,
    `Name { op, source }` or `GateName { op, source }` for a name, and
    `ParameterRef { op, source }` or `Unresolvable { op, name, source }` for a
    reference.
  - **A document that loaded is refused.** A multipart body whose schema is a
    `$ref` that cannot be followed loaded with no part names. It is
    `LoadError::Unresolvable { op, name: "requestBody", source }`, as for any
    other body schema. Repair: correct the `$ref` in an Overlay.
- **A document that is not OpenAPI 3.0 is refused, and the refusal names the
  version.** `LoadError::Version { version }` is asked of the vendor's document
  before any Overlay is laid and before anything else reads it, and its message
  is ``the document says `openapi: 3.1.0`, and this crate reads OpenAPI 3.0
  only``. `3.0`, and `3.0.` with a patch number after it, are read; a
  pre-release label may follow the patch, as in `3.0.0-rc1`. A 3.1 document
  loaded when none of its spellings was one that the model refuses, and then
  the reduction dropped the rules that 3.1 means and 3.0 does not: a `$ref`
  keeps the `maximum` beside it, and `const` is a rule. The rest were refused
  with serde's message, which names a spelling and not the version. There is no
  repair that makes 3.1 readable; support for it is out of scope. The README
  says "OpenAPI 3.0".
- **An Overlay that changes `openapi` is refused.**
  `OverlayError::OpenApiVersion { before, after }` is what `overlay::apply`
  returns when a layer leaves the key with another value: another version,
  another patch of 3.0, a value that is not text, or no key. A layer that
  writes the version the document already has changes nothing and is not
  refused. The version is the vendor's. It decides how every schema is read, and
  a label that an Overlay writes can drop rules that nobody who reads the
  Overlay can see. `before` and `after` are the key as the layer found it and
  left it, `None` for no key, and a boxed `serde_json::Value` so that the number
  `3.0` is not the text `3.0`. The message names the version before the layer
  and the version after it, ``the Overlay changes `openapi` from `3.0.3` to
  `3.1.0`; the version is the vendor's, and no Overlay changes it``. A value
  that is not text is named for its kind, as in ``from `3.0` to the number
  `3.0` ``. `LoadError::Layer` names the layer and `GenerateError::Overlay`
  names the file. `OverlayError` is `#[non_exhaustive]`, so a new variant is not
  a break. A 3.1 document that an Overlay held to 3.0 loaded at 0.0.2 and is
  refused now, first for its version, and then there is no repair: the document
  says 3.1.
- **A server URL with variables loads, and the two refusals that remain name
  their repair.** The first server's URL takes the `default` of each variable
  in `variables`, which OpenAPI requires, and `Document::base` holds the result.
  It was refused as `LoadError::ServerUrl`. A variable that `variables` does not
  declare stays in the URL, which is still `ServerUrl`. `ServerUrl` has the new
  field `filled`, the URL with the defaults in it, because a default can make
  the URL bad. Where a variable was filled in, the message shows both, ``
  `servers[0].url` is not a URL: `https://{tenant}.example.com`, which is
  `https://my tenant.example.com` with the default of each variable``. Repair:
  a `match` that binds the fields of `ServerUrl` adds `filled` or `..`. A document with no
  `servers` is `LoadError::NoServer`, whose message is now ``the document lists
  no `servers` entry to send requests to; add one in an Overlay``. An operation
  with no `operationId` is `LoadError::NoOperationId`, whose message is now
  ``<method> <path> has no operationId; add one in an Overlay``. Both are
  refused as before. Repair: the Overlay each message names, shown in
  `docs/overlay.md`. The README's *What it does not do* lists both.
- **A name with a character outside ASCII is refused and not mangled.**
  `names::spelled` kept the ASCII letters of such a name and made a separator of
  the rest, so `straße` was the flag `--stra-e`, `/kontakte/änderungen` the group
  `kontakte nderungen`, and `é` written as `e` and a combining accent was a plain
  `e`. It refuses the name and transliterates nothing. The rule is that the name
  is ASCII, so a letter written as a base and a combining mark, a dash, a
  no-break space and a character that nobody sees (a soft hyphen, a zero-width
  space) are refused as a letter is. The message of the `NameError` names the
  first such character by its code point, such as `U+00AD`, because a soft
  hyphen shows as nothing and an en dash as a hyphen; a name of ASCII that
  kebab-cases to nothing keeps ``does not kebab-case into [a-z0-9-]``. The line
  that names a parameter with no flag reads ``named with a character outside
  ASCII, or in a way that does not kebab-case into [a-z0-9-]``, in the long help
  and in the documentation of a generated wrapper, so a bless step writes that
  sentence where it wrote the shorter one. A document that loaded is refused
  where the name is:
  - a path segment (`Name`). Repair: `x-cli-group` and `x-cli-command` in an
    Overlay;
  - a gate in `x-cli-gates` (`GateName`). Repair: write the gate with an ASCII
    word in `x-cli-gates`, in an Overlay that removes the list and writes it
    again, because an `update` appends to a list;
  - a required parameter (`Parameter`). Repair: for a parameter that is not in
    the path, drop its `required` in an Overlay. For a path parameter, rename it
    and its variable in the path together, in an Overlay that moves the path
    item. The message says which repair loads;
  - the word of `Loading::commit` and `Settings::commit_word`
    (`ConfirmationError::Spelling`). Repair: an ASCII word.

  It loads and changes where the name is an optional parameter, which is carried
  with no flag and named on the subcommand's help, or a property of a flat body,
  which sends the whole body through `--json-body`. No Overlay gives such a
  parameter its flag back, because its name is the name on the wire. A script
  that typed `--stra-e` no longer finds the flag.
- **A gate, a group or a command named `help` is refused.** clap declares
  `--help` on every subcommand and a `help` subcommand on every group, and
  nothing can move either. `x-cli-gates: [help]` loaded, and in a release build
  `--help` answered the gate: `create --commit --help` sent the write and
  `create --help` printed a dry run in place of a help page. A path such as
  `/help/articles` or `/articles/help` loaded and mounted a subcommand that
  clap's own shadows, and a debug build panicked at start-up. The gate is
  `LoadError::ReservedGate`, as for the flags that carry a body. The group or
  the command is the new `LoadError::NameSpent { op, what, name }`, whose
  `what` is `group` or `command` and whose message names the marker that mends
  it. `LoadError` is `#[non_exhaustive]`, so a new variant is no break. Repair:
  rename the gate in `x-cli-gates`, and name the group or the command with
  `x-cli-group` or `x-cli-command` in an Overlay.
- **A parameter or a property named `help` moves to `--param-help` or
  `--body-help`.** It took `--help`: a debug build panicked at start-up, and a
  release build lost the subcommand's help page. `help` is spent in every
  subcommand beside the five flags that carry a body, so the parameter or the
  property moves aside as a field named `raw-body` does, the flag says which wire
  name it carries, and the request still sends `help`. The flag that moved is
  in the model blob, so a bless step writes `param-help` where it wrote `help`.
  Repair: run the bless step again, and type `--param-help` where a script typed
  `--help` for that parameter.
- **An `x-cli-writes` that is not the boolean `true` is refused.**
  `LoadError::Writes { op }`. `"true"`, `yes`, `1`, `[true]` and `false` each
  read as no marker, so a `GET` the document meant to gate was sent on sight, in
  silence. The marker adds a write and never removes one, so `false` is refused
  too, and `null` with it. Repair: write `true`, or remove the marker.
- **An `x-cli-` key that this crate reads nowhere, or reads where it does not
  stand, is refused.** This crate owns the prefix. `x-cli-command`,
  `x-cli-gates`, `x-cli-group` and `x-cli-writes` are read on an operation, a
  method of a path item under `paths`, and `x-cli-requires` on a parameter that
  is not a `$ref`. Every other key under the prefix is
  `LoadError::UnknownMarker { key, at }`, and a key of the five on any other
  object of the document is `LoadError::MisplacedMarker { key, at }`, among them
  `x-cli-writes` on a path item, `x-cli-requires` on an operation and
  `x-cli-requires` beside a `$ref`. `at` is the node that holds the key, spelled
  as an Overlay names a target, such as `$.paths['/vouchers/{id}/render']`. Each
  used to load and read as no marker, so a typo such as `x-cli-write: true` left
  a writing `GET` sent on sight. A name or a value spelled like a marker, such
  as a property called `x-cli-writes` or an `example` that holds one, is not a
  marker and is not read. Nothing under a key that OpenAPI 3.0 does not define
  is read either, such as `webhooks` or a schema's `$defs`. Repair: spell the
  key as one of the five, move it to where it is read, or give an extension of
  your own another prefix.
- **An `x-cli-requires` that names its own parameter is refused.**
  `LoadError::RequiresItself { op, name }`. It loaded, and a debug build of the
  binary panicked at start-up, because clap does not let an argument require
  itself. Repair: list only the other parameters the parameter needs.
- **`GET /{tenant}/vouchers` is `vouchers list`.** Whether a path addresses one
  resource is read from the segments below the group, as `docs/cli.md` says, and
  not from a template above it. A `GET` with a template above the group and none
  below it was `get`, and a document with `getVoucher` beside it was refused as
  `DuplicateCommand`. Repair: use the new name, or keep the old one with
  `x-cli-command: get` in an Overlay.
- **`RefError::Inside { reference }`.** A `$ref` into the inside of a component,
  such as `#/components/schemas/Holder/properties/code`, was `Missing` and
  printed that it does not resolve, though the pointer is valid. It is `Inside`,
  and its message says that this crate follows only
  `#/components/<section>/<name>`. A pointer inside a name that no component has
  is still `Missing`. `RefError` is not `#[non_exhaustive]`, so a
  `match` on it with no `_` arm stops compiling. Repair: add an arm for
  `Inside`. The document was refused before and is refused now, and only the
  reason changed. Repair for the document: point the `$ref` at the component.
- **`GenerateError::OneType` has the field `inline`, and its repair is a
  `title`.** Where one of the two shapes that reduce to one Rust name is stated
  inline by an operation, the message ends ``give an inline schema a `title` in
  an Overlay``, which names a type for the shape and renames no schema of the
  vendor's. The message for two declared schemas is unchanged. `inline` is true
  when either shape is inline. Repair: a `match` that binds the fields of
  `OneType` adds `inline` or `..`.
- **A chain of `allOf` hops does not overflow the stack.** The check for a
  required name that nothing declares followed each hop with a call, and a chain
  of about 7,000 hops aborted the bless step with a stack overflow. It walks a
  work list now, and the chain loads. Nothing else changes: the check refuses
  the same documents.
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
- **`client::fits` takes a type that implements `Serialize`.** It writes the
  value back and walks the body beside it, which is how it sees inside a union.
  The signature is `fits<T: DeserializeOwned + Serialize>`. Every type the
  generator writes has both, and so has a type named in `Settings::replace` when
  it can be written as JSON. A type that cannot is
  `BodyError::Unwritable { op, source }`. Repair: derive or implement
  `Serialize` on a type of yours that a body reaches.
- **A parameter that an operation overrides is one parameter, and the operation
  can be sent.** OpenAPI identifies a parameter by its name and its place, and
  the operation's entry replaces the path item's. The model listed both, so the
  operation had two required flags (`--id` and `--param-id`) and the typed
  wrapper two arguments (`get_thing(id, id_2)`), both filed under `id`: a
  request that gave one lacked the other flag, and one that gave both failed
  `Invocation::new` with "given 2". It is one parameter now, in the place the
  path item lists it, held to the rules the operation's entry states, so
  `Operation::params()` lists it once, the command line has `--id`, and the
  wrapper is `get_thing(id)` with the type of the operation's entry, whether the
  operation states it inline or by a `$ref`. A parameter of the operation that
  overrides nothing follows the path item's. The same name in another place does
  not override: it is the next entry. A parameter that the operation gives by a
  `$ref` alone has its wrapper argument too, where the bless step refused it
  with "is not declared on this operation". Repair: run the bless step again,
  and drop the extra argument at a call of such a wrapper. A script that typed
  `--param-id` loses the flag.
- **A second parameter with one name is carried with no flag.** `Values` and
  `Operation::param` find a parameter by its name, so of `ref` in the path and
  `ref` in the query only the first can be given a value. The second had a flag
  (`--param-ref`) and a wrapper argument (`ref_2`) that could never send,
  because every value was filed under the first. The one that comes later in
  `Operation::params()` is now `Shape::Unreachable(Unsupported::Shared(place))`,
  and its long help and the documentation of the wrapper say it is sharing its
  name with the `path` parameter. That list is the path item's parameters in the
  order the path item lists them, with the operation's override in the slot of
  the entry it replaces, and then the operation's other parameters in the order
  it lists them. `Unsupported::Shared(Place)` is a new variant of `Unsupported`,
  which is not `#[non_exhaustive]`, so a `match` on it with no `_` arm stops
  compiling. `Place` is a new public enum (`typed_openapi::Place`: `Path`,
  `Query`, `Header` and `Cookie`) that displays as the `in` of the document. The
  variant and the enum are in the blob, so `Document::MODEL_LAYOUT` is 5. A
  document that loaded is refused where the later parameter is `required`
  (`LoadError::Parameter`, with the new reason), and the earlier one may be a
  parameter that has no flag itself, such as an `in: cookie`. Repair: run the
  bless step again; for a document that is refused, the message names the
  Overlay (the next entry), and `docs/cli.md` says what the rule costs. A script
  that typed `--param-ref` loses the flag.
- **`LoadError::Parameter` says where the parameter is, and names a repair that
  loads.** The message named "correct the parameter in an Overlay, or drop its
  `required`" for every refusal. `OptionalPathParameter` (below) refuses a path
  parameter that is not `required`, so the message names dropping `required` only
  off the path. The variant has a new field, `place: Place`, so a pattern on it
  with no `..` stops compiling; `LoadError` is `#[non_exhaustive]`, which does
  not cover a field. The message now names:
  - for a parameter off the path, to correct it or to drop its `required`;
  - for a path parameter, to correct it. For one whose name has no spelling it
    names a rename of the parameter and its variable in the path together, in an
    Overlay that writes the path item again under the new path and removes the
    old one, because the template names the parameter and no `update` of the
    parameter alone loads. The request is the same, because the name of a path
    parameter never goes on the wire;
  - for a name that another parameter has, where one of the two is a path
    parameter and the other is not, to rename the path parameter and its
    variable in the path together, as above, whichever of the two was refused.
    The rename gives up no parameter, so it comes first. After it the message
    names the repairs that give a part of the document up: to remove the other
    parameter, and for a parameter off the path to drop its own `required`. The
    message says whose `required` it is, because it names two parameters of one
    name. A parameter off the path whose namesake is a path parameter is not
    told to remove it, because the template needs the namesake;
  - for a name that two path parameters have, to remove the earlier one, because
    the template has one variable for both and no rename parts them;
  - for a name that two parameters off the path have, to remove the other
    parameter or to drop the `required` of the refused one, because both names
    go on the wire and a rename would change the request. Where both have one
    place, the message calls them the earlier one and the later one.

  Repair: add `..` or `place` to the pattern.
- **A path template and its path parameters must agree.** A request went out
  with `{id}` in its path, or with a required flag whose value was thrown away,
  and `http::Uri` accepted the braces, so a confirmed write was sent too. Each
  of these is a `LoadError` that names the operation, the path and the name, and
  each is mended by an Overlay:
  - `UnboundVariable { op, path, name }`: a variable in the path with no path
    parameter of that name. Repair: add the parameter. A parameter in the query
    or a header with the name does not answer it.
  - `UnusedPathParameter { op, path, name }`: a path parameter with no variable
    of its name in the path. Repair: remove the parameter.
  - `OptionalPathParameter { op, name }`: a path parameter that is not
    `required: true`. Repair: set `required: true`.
  `LoadError` is `#[non_exhaustive]`, so the three variants are no break.
- **A `servers` on a path item or on an operation is refused.** A request goes
  to the first server of the document, so an upload endpoint on another host was
  sent to the wrong one, in silence. `LoadError::Servers { op, at }` names the
  operation and spells the list as an Overlay names a target, such as
  `$.paths['/upload'].post.servers`. A path item's list is refused at an
  operation under it, and a path item with no operation is not refused. An empty
  list is no list. Repair: an Overlay `remove` action with that target
  (`docs/overlay.md`), after which the request goes to the first server of the
  document or to the base URL the caller passes.
- **The header parameters `Accept`, `Content-Type` and `Authorization` are
  dropped.** OpenAPI 3.0.3 says such a definition "SHALL be ignored". The names
  are compared without case, and only in a header. They became flags, so a
  `Content-Type` parameter put a second `content-type` on a request that had a
  body, and an `Authorization` parameter was a required `--authorization` flag,
  which put the credential in argv, in shell history and in every dry run. They
  are not parameters of the operation now: no flag, no wrapper argument, no
  `Shape::Unreachable`, and no refusal of a document that declares one as
  `required`. A value given for one is `ValueError::UnknownParam`. A document
  whose `x-cli-requires` names one of the three loaded, and is refused now
  (`RequiresUnknown`: the header is not a parameter of the operation). Repair:
  set the header in the client adapter (`docs/client.md`), where a credential
  belongs, and drop the argument at a call of a wrapper that took one; for the
  refusal, drop the name from `x-cli-requires` in an Overlay. A script that typed
  `--authorization` loses the flag.

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
