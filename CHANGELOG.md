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
- **`Call::plan`, `client::Planned` and `tree::confirmation`: the typed call and
  a verb of your own take the gate a command line has.** `Call::plan(&Answers)`
  puts a typed call to the gate and returns a `Planned`. It holds the verdict
  (`plan()`) and the request that was decided on (`request()`), and
  `Planned::send` and `send_async` send that request and answer `Some(T)`, or
  send nothing and answer `None` for a dry run. The request is built once, so
  what a caller prints as a dry run is what a confirmed run sends, and no
  signature takes an operation and a request side by side. A `Planned` is
  `#[must_use]`, so a plan that is dropped unused is a warning.
  `tree::confirmation(cmd, op)` adds the confirmation flag to a command of your
  own in the word the operation carries, as `tree::command` does, and adds none
  for a read. Its help says "Send the request"; a verb that sends several
  replaces it with `.mut_arg(COMMIT_ID, |arg| arg.help(..))` on the command of
  a write, because `mut_arg` panics on a read, which has no such flag. A verb
  that spelled `Arg::new(COMMIT_ID).long(COMMIT)` itself accepted `--commit`
  only, so with `Loading::commit("yes")` the help of its gate flags named
  `--yes`, a flag the verb refused. Repair: use `tree::confirmation(cmd, op)` in
  the place of that `Arg`, and `Call::plan` where you called `Plan::decide` on a
  typed call.

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
  `client::Error::{Document, Request}`,
  `LoadError::ServerUrl` and `GenerateError::{Yaml, Schema, Typify, NotRust,
  Blob}`. These are worded anew (the four call-time variants of `client::Error`
  are in the entry that names their operation, under *Changed*):
  - `ValueError::BadValue`: ``<op>: `<name>` was given a value the document
    refuses``
  - `LoadError::Unresolvable`: ``<op>: `<name>` has a `$ref` that cannot be
    followed``
  - `LoadError::Unrunnable`: ``<op>: `<name>` states a rule that cannot run``
  - `DispatchError::Transport`: `the transport failed`
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
- **A base URL with userinfo, a query or a fragment is refused.** A dry run
  printed `user:password` in its `host:` line, a confirmed run sent it as a
  `Basic` header, and the query was dropped from every request in silence.
  Whoever names the base is held to it, and a request is never built from one
  that holds any of the three:
  - `RequestError::Base(BaseUrlError)` comes from `Invocation::request`, so the
    gate, the typed call and the command line all end in it, for the document's
    first server, `Client::with_base`, and the `base` of `Plan::build`,
    `Selection::plan`, `Selection::send` and `tree::dispatch`. `BaseUrlError` is
    `Userinfo`, `Query` or `Fragment`, and an `http::Uri` drops a fragment when
    it parses, so only the text of a document holds one.
  - `LoadError::ServerBase { source }` refuses a document whose first server URL
    holds one of the three, variables filled in. A document that loaded with
    one is refused now. Repair: an Overlay that corrects `servers[0].url`
    (`docs/overlay.md`).
  - No text that the library makes holds a base URL's userinfo. The refusals of
    `BaseUrlError` and `LoadError::ServerBase` print no part of the URL, and
    `render` prints the host and the port of a request and never its userinfo.
    Userinfo is looked for where an authority is read: in the text up to its
    first `/`, `?` or `#`, and, when that character starts a `//`, in the text
    after it up to the next one. So a base is refused for `//alice:s3cret@host/`
    too, which the `http` crate takes for a path with no authority and a request
    was built from, and a dry run, a message and a `Debug` leave that userinfo
    out.
  - A server URL that holds userinfo is `ServerBase` also when it is no URL, and
    whatever stands before its `//` but a `/`, `?` or `#` (a variable that
    nothing declares, a space, a character that no scheme holds, no colon): it
    was `LoadError::ServerUrl`, which printed the URL, the userinfo among it.
    `ServerUrl` prints the URL as the document spells it beside the one with the
    defaults in it, so a URL that is refused is read in both. A document that was
    refused as `ServerUrl` for that reason is refused as `ServerBase` now. A text
    with an `@` in none of the places that are read, such as `http:/u@h/a b`,
    stays `ServerUrl`, and its message prints it.
  - The `Debug` of a `Client` and of a `Document` prints the base without its
    userinfo and its query. It printed the base whole, so a `{client:?}` in a log
    held the credential of a `Client::with_base`, and a `Document` that
    `from_blob` read held the one its blob was written with.

  Repair for a caller: pass a base URL with the scheme, host, port and path
  only, and set the credential in the client adapter (`docs/client.md`).
  `LoadError` is `#[non_exhaustive]`, so the variant is no break.
- **`Invocation::request` returns `RequestError`, not `http::Error`.**
  `RequestError` is `#[non_exhaustive]`: `Base(BaseUrlError)` and
  `Http(http::Error)`. `PlanError::Request` and `client::Error::Request` carry a
  `RequestError` and print what they printed. Repair: match `Http(error)` where
  you matched the `http::Error`, or print the error and its `source()` as before.
- **A path value that is empty, `.` or `..` is refused.** `DELETE /things/..` was
  built and sent as written, a server or a proxy read it as the collection above,
  and reqwest sent `DELETE /` itself. `ValueError::PathSegment { op, name, value }`
  refuses it in `Invocation::new`, so the command line and the typed call refuse
  the same value, and a list is judged as the segment it makes. Repair: send
  a name.
- **A header value that a receiver would read as another value, or another
  list, than the caller gave is refused by name.** The `http` crate refused a
  control character late, as "failed to parse header value", with no name; the
  rest was sent as it came. Four refusals come from `Invocation::new`, each
  `ValueError::<variant> { op, name }`, and none prints the value:
  - `HeaderValue`: a byte that `http::HeaderValue` refuses (a control character
    other than a tab).
  - `HeaderWhitespace`: a space or a tab at the start or the end of a value, or of
    one item of a list, which a receiver strips (RFC 9110 sections 5.5 and 5.6.1).
    `" a"` was sent as `" a"`.
  - `HeaderItemComma`: a comma inside one item of a header list, which a receiver
    reads as two items. `["a,b", "c"]` was sent as `a,b,c`.
  - `HeaderItemEmpty`: an empty item of a header list, which a receiver ignores
    (section 5.6.1.2). `["", "c"]` was sent as `,c`, and `[""]` as an empty value.

  A comma in a query list or a path list is percent-encoded and stays one item,
  and a comma in a header that is not a list is a character of the value. A byte
  of 0x80 or above and an empty value that is no list reach the receiver as they
  are, so they are sent. A header parameter that would claim a flag and whose name
  is no header name, such as `X Bad`, refuses the document
  (`LoadError::HeaderName { op, name }`); it loaded, and a request that gave it a
  value failed. A header with no flag sends nothing and is not asked. `ValueError`
  is `#[non_exhaustive]`, so the variants are no break. Repair: an Overlay that
  corrects the name or removes the parameter; send no control character other
  than a tab in a header value; send a header value, and each item of a header
  list, with no space or tab at its edge; send no comma inside an item of a list;
  and leave out an empty item.
- **A number is sent as it was parsed, and a number that would be sent as
  another one is refused.** A parameter was checked by `Scalar::parse` and its
  raw text went on the wire, so `+5` was sent as `%2B5` and `007` as written,
  where the typed call sent `5`. `Scalar::wire` is the text that is sent, and
  `Invocation::new` keeps it: `+5` is `5`, `007` is `7`, `1e3` is `1000`. Two
  refusals change, as `ScalarError::Rule`:
  - A `type: number` value that does not survive the round trip through a 64-bit
    float is refused. A parameter went out as it was typed, `9007199254740993` and
    `1e-400` among them, and a body field went out as the nearest float:
    `9007199254740992.0` and `0.0`. A value past what a float holds (`1e400`) was
    refused as *not a number*, and is refused as *out of range* now.
  - A `type: integer` value past `i64` is *out of range*. It was refused as *not
    an integer*, and `typify` writes a `u64` for `minimum: 0`.

  A body field is the JSON value that was parsed, as before. Repair: send fewer
  digits, or declare the value a `string` with a `pattern`, as the toy's
  `Voucher.total` does. `docs/validation.md` lists integer formats among the
  places where the two roads part: a flag reads an `i64` whatever `format` says,
  and `typify` writes `i32` for a body property of `format: int32`.
- **A flag takes a value that starts with a dash.** The word after an `<INT>`,
  `<NUMBER>` or `<STRING>` flag is its value, whatever it looks like: `--limit -5`
  and `--total -12.50`, where clap read `-5` as a flag and only `--limit=-5`
  worked. A `<NUMBER>` flag takes `-.5` and `-1e-3` as a separate word the same
  way. A word that looks like a flag is a value too. `--type --commit` gives
  `type` the text `--commit` and confirms nothing; a read is sent at once, so
  `vouchers list --type --commit` sends `?type=--commit`, and an unconfirmed write
  shows the value in its dry run. A number flag refuses a word that is no number,
  and an `enum` flag takes a word that starts with a dash only when it is one of
  its values. Repair: none; a script that relied on clap's refusal of the word
  after a flag sees the word taken as the value.
- **`tree::values` reads a flag of another type as an error, and a field is held
  to its rule.** A verb of your own that declares `--id` with
  `value_parser!(i64)` where the document types it as text made `values` panic.
  It reads every flag with `try_get_*` now, as `answers` does: a flag the command
  never declared is a flag that was not given, and a flag of another type is
  `ArgError::FlagType { flag, source }`, which names it. A per-field flag that the
  document's rule for the field refuses is `ArgError::FieldValue { flag, source }`
  where the field was dropped from the body in silence. `ArgError` is
  `#[non_exhaustive]`, so both are no break. Repair: declare the flag as text,
  which is what `values` reads, and type a value that the field's rule accepts.
- **A numeric or boolean `enum`, and a list's `minItems`, `maxItems` and
  `uniqueItems`, are enforced on flags.** `--status 7` against
  `enum: [100, 200]` was sent, `--help` listed no value, and the generated type
  refused the `7` that the flag sent. `--ids 1 --ids 1 --ids 3` against
  `maxItems: 2` was sent too. `Scalar::parse` checks the enum and
  `Scalar::note` renders it (`one of 100, 200`), as a bound is, and a value is
  admitted whatever its spelling (`+200`). A list is held to its rules in
  `Invocation::new`, so the command line and the typed call are refused alike,
  as `ValueError::BadList { op, name }` with the values as they were given in
  its cause (`` `1, 1, 3` is not a list of at most 2 values ``). `ValueError`
  is `#[non_exhaustive]`, so the variant is no break. sevdesk-cli has 14 numeric
  enums in its corrected document, and four of them are query parameters
  (`GET /Invoice`, `/Order` and `/Voucher` `status`, and
  `getTotalDataExportCount`'s required `status`): its CLI enforces them when it
  moves its rev. `--json-body-template` shows the first value that an `enum` of
  integers or numbers lists, where it showed `0` or `0.0`, and the one value that
  a boolean `enum` admits, where it showed `false`; an `enum` that lists both
  booleans still shows `false`. Repair: none, the template is a shape and not a
  request. The model changes, so this is a break of three public shapes
  and of the blob:
  - `Scalar::Boolean` is `Scalar::Boolean(Option<bool>)`, with the one value that
    an `enum` of `type: boolean` lists, and `None` where there is none or it
    lists both. Repair: match `Boolean(_)`, and build `Boolean(None)`.
  - `scalar::Bounds<T>` gains `allowed: Vec<T>`, the `enum` in the document's
    order, and is no longer `Copy`. Repair: build it with `..Bounds::default()`,
    and clone where you copied.
  - `Shape::Flag`'s `join: Option<Join>` is `list: Option<List>`, where
    `List { join: Join, items: scalar::Items }` and `Items { min, max, unique }`
    hold the three rules of the array. `Shape::repeatable` is unchanged. Repair:
    read `list.map(|list| list.join)` where you read `join`.
  - `Document::MODEL_LAYOUT` is 6. Repair: run the bless step again and commit
    `src/model.postcard`; a blob of layout 5 is `DocumentError::Stale`.
- **A refusal is worded from the note of its rule.** The sentences that
  `Scalar::parse` gives for a length and a `pattern` are the note of the rule
  after "is not", as those for a bound already were:
  `` `EU` is shorter than 3 characters `` is `` `EU` is not at least 3 characters ``,
  `longer than` is `is not at most`, and `` does not match ^[A-Z]+$ `` is
  `` is not a match for ^[A-Z]+$ ``. The note on the help line reads `a match for
  ^[A-Z]+$` where it read `matches ^[A-Z]+$`. The variant is `ScalarError::Rule`
  as before. Repair: none for a user; a test or a script that matches the old
  sentence reads the new one.
- **`multipleOf` is decided on the digits of the value and of the step.** A
  binary float divides `0.07` by `0.01` to `7.000000000000001`, so 159 of the
  first 1000 amounts in cents, `0.07` and `0.29` among them, were refused with a
  false reason, and `1e17` was admitted as a multiple of `3`. The digits
  are exact, so `0.07`, `0.29` and `19.99` are multiples of `0.01`, and the
  remainder of an integer cannot panic. A value that the flag admitted wrongly is
  refused now: `1e17` is no multiple of `3`. Repair: none for a value that the
  flag refused wrongly, which is accepted now; for a value that it admitted
  wrongly, send a multiple.
- **A `pattern` with `\p{`, `\P{` or `\u{` is refused while the document is
  loaded, in every schema the model reads.** Without the Unicode flag `\p{L}` is
  the text `p{L}`, and with it a letter. The command line and the generated type
  run a `pattern` without the flag, and each generated type that holds one
  refuses `Müller` and accepts `p{L}` where a vendor's server that runs it with
  the flag does the opposite. On a `pattern` that a path, query or header
  parameter or a property of a JSON body carries, with a flag or without one, the
  document is refused as `LoadError::Unrunnable { op, name, source }`. In any
  other schema (a response, a property of a nested object, the items of an array
  body, a named schema) it is refused as the new
  `LoadError::UnrunnablePattern { op, at, source }`, which names the operation
  where one holds the schema and the schema as an Overlay names a target. A
  schema beside a `$ref` that is text, in an object that can be a reference, is
  not read, because the model keeps the reference alone. `LoadError` is
  `#[non_exhaustive]`, so the variant is no break.
  The cause prints the pattern and the escape. A backslash takes the character
  after it, so `\\p{3}` loads. sevdesk-cli's 43 patterns use none. Repair: an
  Overlay that states the pattern another way, and the class is a choice. A
  class such as `[A-Za-zÀ-ÿ]` refuses `Łukasz` and admits `a×b`, where `\p{L}`
  with the flag does the opposite; an Overlay that removes the `pattern` leaves
  the rule to the server.
- **An `enum` that lists `null` and no other value is refused while the document
  is reduced.** A flag cannot send `null`, so the `enum` left no rule and the
  flag admitted every value, where JSON Schema admits no integer under
  `{ type: integer, enum: [null] }`. The document is refused as
  `LoadError::Unrunnable { op, name, source }` on a path, query or header
  parameter, on the items of such a list and on a property of a JSON body whose
  schema is an object. `null` beside a value stays the way OpenAPI 3.0 says that
  a value may be `null`. Repair: an Overlay that removes the `enum`, or lists the
  values.
- **A `multipleOf` that is not more than 0 is refused while the document is
  reduced.** A step of 0 refused every value, and `multipleOf: -1` with the value
  `i64::MIN` made `Scalar::parse` panic. The document is refused as
  `LoadError::Unrunnable { op, name, source }`, as a `pattern` that no engine
  reads is. sevdesk-cli states no `multipleOf`. Repair: an Overlay that removes
  the keyword or states the step as a positive number.
- **A property of a body is read for its rules wherever it stands among its
  siblings.** A body goes whole through `--json-body` at its first property that
  has no flag, a nested one or one whose name has no spelling, and the
  properties listed after it were not read for their rules, so a `pattern` that
  no engine reads refused a document or not by the order of the properties.
  Every property of the object that a JSON body's schema is, is read now, and the
  three refusals above hold wherever it stands. A document that loaded and
  failed the bless is refused with the property named: a `pattern` such as `[`,
  an `enum` of `null` alone (`LoadError::Unrunnable`) and a `$ref` that does not
  resolve, which made typify panic (`LoadError::Unresolvable`). A document that
  loaded and blessed is refused too, after a nested property or one whose name
  has no spelling: a `multipleOf` that is not more than 0, and a `$ref` that the
  reduction cannot follow because it is a cycle or points inside a component
  (`LoadError::Unresolvable`). Repair: an Overlay that states the rule another
  way or removes it, or that makes the `$ref` name a component schema
  (`#/components/schemas/<name>`) that does not lead back to itself.

- **A body goes out under the media type that the document declares.** A JSON
  body was sent as `application/json` whatever its `content` key said, so a body
  declared as `application/merge-patch+json` or `application/vnd.api+json`
  reached its server under another type, which answers 415 or reads it with the
  semantics of the other. The `Content-Type` is the key as the document spells
  it, parameters included and less a space and a tab at its two ends, on the
  command line and in the typed call alike. A
  document that declares `application/json` sends what it sent, and so does
  sevdesk-cli's, whose 76 JSON bodies all declare it. The model carries the key,
  so two variants of `Body` change:
  - `Body::JsonFields` is the struct variant `JsonFields { media_type, fields }`,
    where it was the tuple variant `JsonFields(Vec<Field>)`;
  - `Body::JsonWhole` has the field `media_type` beside `required` and
    `template`.

  `Body::media_type()` is new: it answers the key of a JSON body and of a body
  sent verbatim, and `None` for no body and for `multipart/form-data`, whose
  `Content-Type` carries a boundary chosen when the parts are encoded.
  `Document::MODEL_LAYOUT` is 7. Repair: a `match` that binds `JsonFields(fields)`
  binds `JsonFields { fields, .. }`, and one that binds the fields of `JsonWhole`
  by name adds `media_type` or `..`. Run the bless step again and commit
  `model.postcard`: a blob of layout 6 is `DocumentError::Stale`. A body that
  is declared under several `content` keys is sent under the first that names
  JSON, wherever it stands, and else under the first of all. That is the rule
  the crate applied, and it reads `application/*+json` as JSON, as it did;
  `docs/validation.md` states it and `tests/media.rs` pins it.
- **A `content` key that is a media range is refused while the document is
  reduced.** `*/*` and `image/*` were media types to the check and became the
  request's `Content-Type`, which no server reads. A key with a `*` in its type
  or its subtype, when it is the key that the body is sent under, is refused as
  the new `LoadError::MediaRange { op, media_type }`, which names the operation
  and the key. `LoadError` is `#[non_exhaustive]`, so the variant is no break. A
  range that is not the key that is sent is not read, and its document loads as it
  did, for example `*/*` or `image/*` beside a key that names JSON, and any range
  after a key that does.
  - `application/*+json` is a range, and it names JSON by its suffix, so the rule
    that picks the key reads it as JSON. Where it is the first key that names
    JSON it is the key that is sent, and the document is refused. That is
    `application/*+json` before `application/json`, and after a key that names no
    JSON (`text/plain`, `application/x-www-form-urlencoded`,
    `multipart/form-data`). Such a document loaded before, with the flags of the
    range's schema and under `application/json`. Where `application/json` or
    another key that names JSON stands before it, it is not read and the document
    loads as it did.
  - sevdesk-cli declares no range: its 76 JSON bodies declare `application/json`,
    and its other 6 are `multipart/form-data` (2) and
    `application/x-www-form-urlencoded` (4).

  Repair: an Overlay that removes the range key, which leaves the key that names
  JSON beside it to be sent, or that removes the range and states the type the
  server reads, as for a key that is no media type
  ([docs/overlay.md](docs/overlay.md#1-plain-corrections)).
- **A `content` key that no header carries is refused as a media type, when it
  is the key that goes out as the `Content-Type`.** The check read the type and
  the subtype and nothing else of the key, so these loaded:
  - an ASCII control character other than a tab in a parameter after the `;`;
  - a white space other than a space and a tab before the type or after the
    subtype, which `str::trim` took off before the check: a line feed, vertical
    tab, form feed or carriage return, a no-break space, or a next-line
    character, such as `application/json` followed by a no-break space.

  A JSON body ignored the key and went out as `application/json`, so each of
  them loaded and worked. A body sent verbatim loaded too, and went out with the
  white space in its `Content-Type`, which is no media type to RFC 9110 (a
  no-break space is no `token` character and no optional white space), or failed
  every request with `RequestError::Http` for a control character. A JSON body
  now goes out under the key, so the load refuses each of them as
  `LoadError::MediaType`, for a JSON body and a body sent verbatim, when the key
  is the one that is sent. A C1 control character in a parameter is a byte that a
  header carries, and it loads as it did. A space and a tab before the type or
  after the subtype are optional white space in RFC 9110 and are no fault, and
  the request goes out without them, because RFC 9113 calls a field value that
  starts or ends with one malformed: a JSON body with such a key went out as
  `application/json`, and a body sent verbatim went out with them. A space or a
  tab inside the key, as before the `;`, is sent as the document spells it.
  A `multipart/form-data` key is not asked what a header carries, because that
  body writes its own `Content-Type`, with a boundary, and its key is never sent:
  a document that holds one with such a byte loaded and its requests went out as
  `multipart/form-data; boundary=…`, and it still does. The message prints the
  key as the document spells it, and writes each character that does not show as
  its code point (`\u{a0}`): a control character, a white space other than a
  space, a format character such as a zero width space, and a combining mark.
  sevdesk-cli declares no such key: its 82 body keys are `application/json`,
  `multipart/form-data` and `application/x-www-form-urlencoded`. Repair: an
  Overlay that replaces the key with the type the server reads.
- **An optional body can be left out, and a required one cannot.** The model
  lost whether `requestBody` is required for a flat JSON body: it folded the
  body's `required` into each property's, so an optional flat body and a required
  one in which no property is required read the same. Each road then did what
  the model allowed. The command line sent `{}` for an optional flat body when
  nobody gave a flag, and the type of the body refused it when the schema
  required a property. `Invocation::new` accepted a required flat body that was
  missing, so `Plan::build` with the commit made a `POST` with no body. And the
  wrapper took every body as a required argument. Now the model carries it for
  every kind of body, and both roads read it:
  - `Body::JsonFields` has the field `required`, beside `media_type` and
    `fields`, as the other three kinds of body have. `Field::required()` is
    whether the schema's `required` list names the property, whether or not the
    body is required; it was true only where both were. `Document::MODEL_LAYOUT`
    is 8. Repair: a `match` that binds every field of `JsonFields` adds
    `required` or `..`, and a caller that read `Field::required()` as "the flag
    is demanded" asks it of a body that is required as well. Run the bless step
    again and commit `model.postcard`: a blob of layout 7 is
    `DocumentError::Stale`.
  - `Invocation::new` refuses a required body that is missing, of every kind, as
    `ValueError::MissingBody`. It was a flat JSON body that got through. Repair:
    give the body with `Values::json`.
  - The command line sends no body for a flat body when no flag and no
    `--json-body` is given, and no `Content-Type` with it; it sent `{}` for every
    flat body. An optional body then goes out with no body, and a required one is
    refused as `ValueError::MissingBody`, as it is for the other kinds of body.
    Repair: `--json-body` with a file that holds `{}`, or `-` with `{}` on stdin,
    sends the empty object. The flag of a required property is demanded of a
    required body only, and `--json-body` stands for it, as before.
  - A generated wrapper takes a body the document leaves optional as an `Option`:
    `Option<&T>` for a JSON body, `Option<Vec<u8>>` for one sent verbatim and
    `Option<Vec<Part>>` for a multipart body, and `None` sends the request with
    no body. A body with `required: true` is the argument it was. So the
    generated code of a document whose bodies are all required does not change,
    and the toy's `ops.rs` is the same file. Repair: pass `Some(&body)`, or
    `None`. With the `builder` feature the setter is `.body(&body)` as before, and
    may be left out.

  sevdesk-cli met this. 66 of its wrappers take an `Option` body: 20 flat JSON
  bodies, 42 JSON bodies that go whole, 3 sent verbatim and 1 multipart, and each
  call of them needs `Some(..)` or `None`. The 20 flat ones are the optional ones
  of its 30 flat JSON bodies. 8 of the 20 have no required property, so the
  command line sent `{}` to its server for them, and sends no body now. `addContactAddress` is the
  one flat body there that is required and has no required property, so it is
  refused when no flag and no `--json-body` is given. Its `client.yaml` removes
  the optional body of `createInvoiceReminder` because the generator made it a
  required argument. That correction can go.

- **A `readOnly` property gets no flag, and a type that a request reaches does
  not require it.** OpenAPI 3.0.3 says that a request "SHOULD NOT" send a
  `readOnly` property and that its `required` "will take effect on the response
  only". One schema for a create and a read, with `id` required and read-only,
  made `--id` a required flag, wrote `id` into `--json-body-template`, and made
  `id` a required field of the generated type, so a caller had to invent an id
  that the API sets. Now:
  - A `readOnly` property has no flag, whether or not the schema requires it. It
    neither sends the body whole nor needs to be a scalar. `--id` is a clap
    error, where it was demanded. The property is read-only when its own schema
    says so, when it points with a `$ref` at a schema that says so, and when it
    is an `allOf` of one element (the spelling that keeps a flag beside a
    reference) with the flag on the wrapper or on the schema it wraps.
    `writeOnly` is not read.
  - A body that went whole only for a `readOnly` property is flat now: a
    `readOnly` property that is nested, a list, no scalar this crate reads
    (`{ readOnly: true }`, a `oneOf`, an `allOf` of more than one schema) or
    named with no spelling (`*`) no longer sends the body whole. It has a flag
    for each of its other properties, `--json-body` as a base document that the
    flags merge over, and no `--json-body-template`; it was `Body::JsonWhole`,
    which takes the file whole and has the template.
    `{ type: object, required: [name], properties: { meta: { type: object,
    readOnly: true }, name: { type: string } } }` had the template and has
    `--name`. The repair is the one of every body that is flat now, in the entry
    on scalars below.
  - The flag of a `readOnly` property goes to a later property with the same
    spelling. In `{ fooBar: { type: string, readOnly: true }, foo-bar: { type:
    string } }`, `--foo-bar` set `fooBar` and `--body-foo-bar` set `foo-bar`;
    `--foo-bar` sets `foo-bar` now, and `--body-foo-bar` is a clap error. The same
    command line sends another body. Repair: a command line drops the flag that
    set the `readOnly` property, which the API sets, and names the other property
    with `--foo-bar`.
  - `--json-body-template` leaves out a `readOnly` property, one that the schema
    requires and one that an `example` carries. An `example` is the template as
    it stands, but it is walked beside its schema through an object and a list
    that this crate reads, into the value of each key that the object's
    `properties` declare, and a `readOnly` key goes from it at every depth of
    that walk: `example: { id: 7, name: Rex }` with a read-only `id` is
    `{ "name": "Rex" }`. Wherever the walk does not go, the `example` stays as
    the document has it. The walk goes where the schema is an object or a list
    that this crate reads and the `example` has its shape. So it does not go
    where the schema is none, such as below a composition (an `allOf` of more
    than one schema, `oneOf`, `anyOf`), a schema with `properties` and no
    `type`, an object or an array that states a keyword its `type` does not
    have, an object that `openapiv3` holds as `AnySchema`, or a schema that
    cannot be followed, and it does not go under a key that `properties` does
    not declare. A `readOnly` key there stays in the template and is named
    nowhere.
    The long help names each `readOnly` property of the body's own object, where
    its flag would have been, and each one that the template leaves out below it,
    by its path as a `BodyError` spells a key, with `[]` for any item of a list:
    `address.aid`, `positions[].id`, `[].id` for a body that is a list. The
    template writes the required properties and the keys of an `example` only, so
    a `readOnly` property below the body's own object that the document does not
    require and no `example` carries is left out of nothing and is not named. A
    body whose object `openapiv3` holds as `AnySchema` (it states a keyword of
    another type, or a keyword with no content such as `enum: []`) goes whole
    with no template but its `example`, and the long help names none of its
    `readOnly` properties.
  - `Body::JsonFields` and `Body::JsonWhole` have the field `read_only`: the
    body's own properties first, in document order, then the paths below them in
    the order the template's walk meets them. `Body::read_only()` answers it for
    every kind of body. `Document::MODEL_LAYOUT` is 9. Repair: a `match` that
    binds every field of either variant adds `read_only` or `..`. Run the bless
    step again and commit `model.postcard`: a blob of layout 8 is
    `DocumentError::Stale`.
  - A generated type that a request body reaches does not require a `readOnly`
    property: `id: i64` is `id: Option<i64>`, with the `serde` attributes typify
    writes for an optional field. A request body reaches the schema it names or
    states and every schema those refer to with a `$ref`. A type that only
    responses reach keeps the document's `required`, because there it takes
    effect. A schema that is an `allOf` is one type, so a property that the
    schema or any member declares read-only comes off the `required` of the
    schema and of every member stated inline in it, whichever member lists it.
    Repair: code that builds such a type writes `Some(id)` or leaves the field
    out, and code that reads it handles `None`. Run the bless step again and
    commit `types.rs`. One limit: a member that is a `$ref` keeps its own
    `required` unless it declares the property read-only itself, because a named
    schema is one type wherever it is used.
  - A `readOnly` property is read for no rule of a flag, so a `multipleOf` of 0
    on one loads, where it was `LoadError::Unrunnable`. A `pattern` that no
    engine runs on one is still refused, as `LoadError::UnrunnablePattern`,
    which names the schema, where it was `Unrunnable`, which names the
    operation.
  - A `readOnly` property takes no name from the namespace of its operation,
    because it has no flag. One named like a gate of `x-cli-gates` or like the
    confirmation (`force`, `commit`) loads, where it was
    `LoadError::GateTakenByName` or `LoadError::CommitTakenByName`.

  sevdesk-cli reads the same as before at the level of its flags: of its 30 flat
  JSON bodies none has a `readOnly` property, so no flag goes, and 8 of its 46
  bodies that go whole name `readOnly` properties on the long help. Its
  generated types and wrappers do not change. Its `model.postcard` is written
  again for the layout.
- **A scalar that `openapiv3` holds as `AnySchema` keeps its flag, and a shape
  this crate cannot read names its keyword.** `openapiv3` cannot fit an integer
  with a bound spelled `0.0` or `1e3`, a `string` beside an `allOf`, a `pattern`
  beside an `integer` or an `enum` that lists a value of another type in one of
  its types, and holds each as `AnySchema`. This crate read every such schema as
  no scalar. A parameter was then "neither a value nor a list of values", which
  is false of a number, a required one refused the document under that reason,
  and a body that held one as a property went whole: it had the template and no
  flag. A list parameter, a JSON body and a multipart body that is an `allOf` of
  one element were not read through it either, because the reading of a list's
  array, of a JSON body and of the parts of a multipart body did not unwrap it.
  The items of a list were unwrapped.
  Now:
  - An `AnySchema` whose `type` is a scalar and whose keywords all belong to
    that type is read as that scalar. `{ type: integer, minimum: 0.0 }` is an
    integer with a lower bound of 0, and a required parameter of that schema
    loads. The template of a body that still goes whole writes such a value as
    the empty value of its type (`0`), where it wrote `null`. A body that is the
    value has the template `0`, where it had none, so its operation has
    `--json-body-template`. A body that has such a value as a property is flat
    now where each of its other properties has a flag or is `readOnly`, as the
    bullet on flat bodies below says.
  - An integer's bound that `openapiv3` holds as a double is read where the
    double names the integers the bound admits. A bound with a fraction is the
    integer next inside it, whatever `exclusiveMinimum` or `exclusiveMaximum`
    says: `minimum: 0.5` is at least 1. A whole bound below 2^53 is that
    integer, whatever text gave it: the reader keeps the double and not the text,
    so `minimum: 1.00000000000000001` is at least 1, and the flag accepts 1
    where the document admits only 2 and above. A step (`multipleOf`) is read as
    an integer only where it is whole: a whole double below 2^53 is that
    integer, whatever text gave it, so `multipleOf: 2.00000000000000001` is the
    step 2. A `maximum` above every `i64` bounds nothing, and so does a
    `minimum` below every one, so an unsigned 64-bit id (`minimum: 0`,
    `maximum: 18446744073709551615`) is an integer of at least 0. A `minimum`
    above every `i64`, a `maximum` below every one and a whole bound from 2^53
    to 2^63, which is the double of more than one integer's text, are refused by
    their keyword (`Unsupported::Keyword`) and never rounded. A step that is a
    double with a fraction, or a whole one from 2^53, is refused by its keyword
    too.
  - An array that states a keyword with no content (`properties: {}`,
    `required: []`, `enum: []`, `allOf: []`, `oneOf: []`, `anyOf: []`) is the
    array it is. `openapiv3` holds such an array as `AnySchema`, and this crate
    read it as "neither a value nor a list of values". A list parameter of it
    has the list flag of the same array without the keyword. A JSON body that is
    such an array has a template, which writes one element of its items, where
    it had none, so its operation has `--json-body-template`. A required
    property of such an array is written as a list in the template of the body
    that holds it (`"tags": [""]`), where it was `null`. The long help names the
    `readOnly` properties of the items by their paths (`[].id` for the body,
    `tags[].id` for the property), as it does for an array that `openapiv3`
    holds as a type.
  - A single-element `allOf` is read as the schema it wraps for a list
    parameter, a JSON body and a multipart body, as it is for a value and for
    the items of a list. A list parameter inside one had no flag, and has the
    flag of the list it wraps. A JSON body that is one is read as the object it
    wraps, and is flat when that object is (the next bullet). A multipart body
    that is one names its parts in the help of `--file` and in the doc of its
    generated wrapper (`The document declares: file, note.`), where it named
    none. Run the bless step again and commit `ops.rs`.
  - A JSON body that went whole is flat now when each of its properties has a
    flag or is `readOnly`. Such a body has a flag for each property that is not
    `readOnly`, `--json-body` as a base document that the flags merge over, and
    no `--json-body-template`. It was `Body::JsonWhole`, which takes the file
    whole and has the template. Three kinds of body are flat now:
    1. A body that is an `allOf` of one element around such an object.
    2. A body with a property that is a scalar that `openapiv3` holds as
       `AnySchema`, bare or inside an `allOf` of one element.
       `{ type: object, required: [n, s], properties: { n: { type: integer,
       minimum: 0.0 }, s: { type: string } } }` had the template
       `{"n": null, "s": ""}` and has `--n` and `--s`.
    3. A body that went whole only for a `readOnly` property (the entry above).

    A body with a property that is not `readOnly` and is nested, a list, no
    scalar this crate reads or named with no spelling still goes whole. Repair:
    a command line that passes `--json-body` is read as before, because the file
    is the base document and, with no flag beside it, the body goes out as the
    file has it. A command line that passes `--json-body-template` to such an
    operation is a clap error, and the long help lists the flag of each
    property. Code that reads `Body::JsonWhole` for such an operation finds
    `Body::JsonFields`.
  - An object or an array that states a keyword its `type` does not have is
    `Unsupported::Keyword`, naming the keyword, as a scalar is. A list of
    strings that states `minLength` was "neither a value nor a list of values".
  - A generated wrapper takes such a parameter as an argument, typed as a bare
    one is: an integer with a bound spelled `0.0` is `Option<i64>`, an `allOf`
    of one element over a named schema keeps its type, and a list whose array is
    a `$ref`, bare or inside a wrapper of one element, is a `Vec` argument of
    its items' type. `Settings::write_to` failed with
    `GenerateError::Operation`, "only `type:` schemas have a scalar spelling",
    for a wrapper over a named schema and for the items of a list inside one,
    and "must declare `items` inline" for a list whose array is a `$ref`, and it
    generates them now. A parameter that had no flag and has one now is a new
    argument of its wrapper. Repair: a caller of the typed wrapper passes it,
    `None` for an optional scalar and an empty `Vec` for a list.
  - Every other shape is `Shape::Unreachable` under a reason that is true of it,
    and a required parameter is refused under the same words, as is a parameter
    whose `x-cli-requires` lists it. `Unsupported` has two variants more, and it
    is not `#[non_exhaustive]`: `Keyword(String)` names a composition (`oneOf`,
    `anyOf`, an `allOf` of more than one schema, `not`), a keyword the `type`
    does not have, an `enum` that lists a value of another type, a `type` that
    is none of the six, and an integer's bound or step that is refused as above;
    `Untyped` is a schema with no `type` and no composition. An object whose
    keywords all belong to it, and an array that states no `items` or whose
    items are no value, stay `Structured`. `Document::MODEL_LAYOUT` is 9.
    Repair: a `match` over `Unsupported` adds the two arms. A body property of
    these shapes sends the body whole, as a nested property does, and the long
    help names no reason for it: the whole body is read as JSON, and the
    adopter's seam (`tree::select`, then `check_body`) holds it to the generated
    type.
  - A value that has a flag takes it before the parameters after it and before
    the properties of the body, as every parameter does, so a flag that a later
    parameter or a property had moves aside, and the same command line sends
    another request. A query parameter `id` of `{ type: integer, minimum: 0.0 }`
    beside a body property `id` sets the parameter with `--id`, and the property
    is `--body-id`; `--id` set the property, where the parameter had no flag. A
    header `Id` after that query parameter is `--param-id`, where it was `--id`,
    and `fooBar` before a header `foo-bar` is `--foo-bar`, with the header
    `--param-foo-bar`. The same holds where the query parameter is a list inside
    an `allOf` of one element or an array that states an empty keyword. Repair:
    a caller passes the flag whose help line says "sends `<name>`", and the
    command line that set the value that moved is changed to name it.
  - A document that was refused and loads now is a required parameter, in the
    path, the query or a header, whose value has a flag now: a scalar such as
    `{ type: integer, minimum: 0.0 }`, `{ type: integer, minimum: 0.5 }` or
    `{ type: integer, minimum: 0, maximum: 18446744073709551615 }`; an array that
    states an empty keyword
    (`{ type: array, items: { type: string }, properties: {} }`); an array whose
    items are `{ type: integer, minimum: 0.0 }`; and a list inside an `allOf` of
    one element, bare (`{ allOf: [{ type: array, items: { type: string } }] }`)
    or over a `$ref` to an array. It was `LoadError::Parameter`. A parameter
    whose `x-cli-requires` lists one of these values loads too, where it was
    `LoadError::RequiresUnreachable`.
  - A document that was refused and loads now is also a body that goes whole and
    has a property named like a gate of `x-cli-gates` or like the confirmation of
    a write (`force`, `commit`) listed before the first property that sent the
    body whole. It was `LoadError::GateTakenByName` or
    `LoadError::CommitTakenByName`: `{ type: object, properties: { force: { type:
    integer }, n: { type: object } } }` on a write with `x-cli-gates: [force]`.
    A body that goes whole has no flag, so none of its properties is asked its
    name, whatever the order they stand in. The same properties with `n` listed
    first loaded all along.
  - A document that loaded and is refused now. Four kinds of value have a flag
    now, or are read for their rules now, and each is held to what every flag is
    held to:
    1. A scalar that `openapiv3` holds as `AnySchema`, such as
       `{ type: integer, minimum: 0.0 }`, as a parameter, as the items of a list
       parameter and as a property of a JSON body, wrapped or not.
    2. A list parameter inside an `allOf` of one element.
    3. An array that states a keyword with no content (`properties: {}`,
       `required: []`, `enum: []`, `allOf: []`, `oneOf: []`, `anyOf: []`), as a
       list parameter and as a JSON body.
    4. A property of a JSON body that is an `allOf` of one element, whatever its
       scalar is (`{ type: integer }`, `{ type: string }`, `{ $ref: Code }`),
       because the wrapper is read now.

    The rules of a property of a JSON body are read whether or not its body goes
    whole, so a property of the first or the fourth kind is held to its rules in
    a body that has a nested property too. Its name is asked only where the body
    is flat, because a body that goes whole has no flag for the name to clash
    with, and the order of the properties decides nothing.
    - A rule that cannot run is `LoadError::Unrunnable`, as it is on any scalar
      that has a flag: a step of 0 or less and an `enum` that lists `null`
      alone. The scalar of the first kind is, for example,
      `{ type: integer, minimum: 0.0, multipleOf: 0 }` or
      `{ type: integer, minimum: 0.0, enum: [null] }`, as a parameter, as the
      items of a list parameter and as a property of a JSON body. The items of a
      list parameter of the second and third kind are, for example,
      `{ type: integer, multipleOf: 0 }`. A property of the fourth kind is, for
      example, `{ type: integer, multipleOf: 0 }` or
      `{ type: string, enum: [null] }`. Repair: an Overlay that corrects the
      step or the `enum`, or removes it.
    - A `$ref` that leads nowhere is `LoadError::Unresolvable`. It is refused in
      the items of a list parameter of the second and third kind, bare or inside
      an `allOf` of one element. It is refused in a JSON body that is an array
      that states an empty keyword
      (`{ type: array, items: { $ref: Missing }, enum: [] }`), bare or inside an
      `allOf` of one element, and in a property that a JSON body requires and
      whose schema is such an array, because the template follows the items
      now. It is refused in a property of the fourth kind: `v: { $ref: Missing }`
      in a body that is an `allOf` of one element, with an `allOf` of its own
      around the reference or without; a required one was refused under the
      words of `requestBody` and is refused under its own name. The same
      property in a body that is no wrapper is refused without any of this.
      Repair: an Overlay that corrects the reference.
    - A parameter or a property that has a flag takes its name from the
      namespace of its operation. A query or header parameter of the first,
      second or third kind is held to it. A property of a JSON body is held to
      it where its body is flat (above), whatever its own scalar is. A property
      of a body that goes whole has no flag, so its name is not refused.
      - One named like a gate of `x-cli-gates` is `LoadError::GateTakenByName`:
        a query parameter `force` on a write with `x-cli-gates: [force]`, bare
        or inside an `allOf` of one element, and a property `force` of a JSON
        body that is flat now and went whole before. Such properties of a body
        that loaded and are refused are, for example:
        - `force: { type: integer, minimum: 0.0 }` alone, or before or after
          `name: { type: string }`;
        - `force: { type: integer }` after a property of the first kind
          (`a: { type: integer, minimum: 0.0 }`);
        - `force: { type: integer }` after a `readOnly` property that is
          nested (`a: { type: object, readOnly: true }`), a list, no scalar this
          crate reads (`{ readOnly: true }`, a `oneOf`, an `allOf` of two) or
          named with no spelling (`'*'`);
        - `force` with any scalar in a body that is an `allOf` of one element
          and is flat:
          `{ allOf: [{ type: object, properties: { force: { type: integer } } }] }`.

        A body with no wrapper asked a property its name before, when the
        property stood before the first one that sent the body whole, so such a
        property is not new in this list. Repair: an Overlay that renames the
        gate in `x-cli-gates`, as the refusal says.
      - One named like the confirmation of a write (`commit`) is
        `LoadError::CommitTakenByName`, for the same parameters and the same
        bodies with `commit` in place of `force`. Repair: confirm with another
        word, `Loading::commit` where the document is loaded or
        `Settings::commit_word` where a bless step generates, as the refusal
        says.
      - A header parameter whose name is no header name (`X V`) is
        `LoadError::HeaderName`. Repair: an Overlay that corrects the name or
        removes the parameter, as the refusal says.
    - A multipart body that is an `allOf` of one element whose reference does
      not resolve is `LoadError::Unresolvable`; it was a body with no parts.
      Repair: an Overlay that corrects the reference.
  - A document that was refused is refused under another variant now:
    - A `pattern` that holds `\p{`, `\P{` or `\u{` is `LoadError::Unrunnable`,
      which names the operation and the value, where it was
      `LoadError::UnrunnablePattern`, which names the schema. The value is a
      scalar of the first kind, the items of a list parameter of the second or
      third kind, or a property of a JSON body of the first or the fourth kind,
      whether the body is flat or goes whole. Repair: an Overlay that corrects
      the `pattern`, as before.
    - A required parameter of the first, second or third kind is refused for
      its rule, its reference or its name, where it was `LoadError::Parameter`,
      which says that the parameter has no flag: `Unrunnable`, `Unresolvable`,
      `GateTakenByName`, `CommitTakenByName` or, for a header, `HeaderName`. A
      parameter whose `x-cli-requires` lists such a value is refused the same
      way, where it was `LoadError::RequiresUnreachable`. Repair: as above.
- **`Plan::decide` takes an `Invocation` and builds the request itself.** At
  0.0.2 it was `Plan::decide(op, &answers, request) -> Plan`, which took any
  request beside any operation: a write's request beside a read's `op` was
  `Send`. It is
  `Plan::decide(&invocation, &base, &answers) -> Result<Plan, RequestError>`,
  and the gate reads the operation that the invocation holds, so the verdict and
  the request are about one operation. The error is the request's: a base with
  userinfo or a query, or a URI or a header that `http` cannot build.
  `Plan::build` keeps its signature. Repair:
  `Plan::decide(&Invocation::new(op, values)?, &base, &answers)?`, or
  `Plan::build(op, &base, values, &answers)?`; for a typed call,
  `call.plan(&answers)?`.
- **`Client::over` returns `Self`.** It returned `Result<Client, client::Error>`
  and never failed. Repair: drop the `?` or the `expect` after it. The generated
  `Api::new` still returns a `Result`, for the blob it reads and the inventory
  it checks.
- **The four call-time variants of `client::Error` name their operation.** Each
  gains a field `op`, the `operationId` of the operation whose call failed:
  `Encode(source)` is `Encode { op, source }`, `Transport(source)` is
  `Transport { op, source }`, `Status { status, body }` is
  `Status { op, status, body }`, and `Decode { source, raw }` is
  `Decode { op, source, raw }`. The messages begin with it:
  `<op>: cannot serialise the request body`, `<op>: the transport failed`,
  `<op>: the server answered <status>` and
  `<op>: the response is not the shape the document promises`. Repair: a pattern
  that binds the fields of one of them adds `op` or `..`, and a tuple pattern of
  `Encode` or `Transport` becomes a struct pattern.
- **The `source` of `client::Error::Decode` is a `client::DecodeError`, not a
  `serde_json::Error`.** The message of `serde_json` can quote the value, the
  key or the variant that it refused, and the chain of an error is printed where
  a log reads it: a string value of 3,000 characters made a chain of 3,138
  bytes. `DecodeError` holds `class` (a `DecodeClass`: `Syntax`, `Eof`, `Data`,
  and `Io`, which a slice cannot raise), `line` and `column`, and no byte of the
  body. It prints the words of its class, the line and the column, such as `the
  body does not fit the type at line 1 column 48`. The text stays in `raw`, and
  `Error::body()` reads it. `docs/client.md` says what the position and the
  class mean. Repair: where a caller read the kind or the position from the
  `serde_json::Error` (`downcast_ref` on `source()`), read `class`, `line` and
  `column` from the `DecodeError` and the text from `Error::body()`; the
  expected type is not named, so look at the body near that line and column, and
  at all of it where the line is 0.
- **`client::to_json` takes the operation, and a generated wrapper passes it.**
  It is `to_json(op, &value)` where it was `to_json(&value)`, so that the
  `Encode` error of a body that does not serialise names whose body it was. The
  generated `ops.rs` calls `crate::to_json("createVoucher", body)?`, so an
  `ops.rs` written by an earlier release does not compile. Repair: run the bless
  step again and commit `ops.rs`; a wrapper of your own passes its
  `operationId`. A bless step that links the crate it writes, as sevdesk-cli's
  `xtask` does for its `surface` task, does not build until that crate compiles,
  so the first bless after the upgrade is run with that dependency cut for the
  one run.
- **`client::Error::Status` no longer prints the server's body.** It printed
  `<status>: <body>`, and the body can hold a credential or a client's data,
  where an error is printed to a log. It prints
  `<op>: the server answered <status>`. The body stays in the field `body`, and
  `Error::body()` reads it for `Status` and for `Decode`, which holds the text
  it could not read. Repair: where a caller showed the server's reason through
  `{error}`, show `error.body()` as well, where that is allowed.
- **The summary page escapes `|` in a table cell.** A name that holds one, an
  `operationId` or a parameter, ended its cell in the table of parameters
  carried without a flag, so the row had more cells than the header. It is
  written `\|`, which a Markdown reader shows as `|`, in a code span too. A page
  that names no such value is unchanged. Repair: run the bless step again, where
  the adoption commits the page (`Settings::summary_page`).
- **A vendor's description is written so that rustdoc runs none of it, and its
  HTML and its links to a script are text on your pages.** A `summary` or a
  `description` is a doc comment, and rustdoc compiles and runs the code blocks
  in a doc comment. The generator found a block by reading the text a line at a
  time, so a block inside a Markdown container was left for rustdoc, and so was a
  fence whose info string rustdoc reads as Rust without being a word it lists.
  `cargo test --doc` in the crate that holds the generated file ran the vendor's
  sentence. The generator reads each description with `pulldown-cmark`, the
  parser rustdoc is built on, with the options rustdoc passes, and writes:
  - the language `text` on every fence that rustdoc reads as Rust, wherever the
    block stands: at the top, in a block quote, in a list item (also when the
    fence opens on the line of the marker) or in a footnote, and whatever the
    info string is: none, `rust,ignore`, `rust2018`, `{.json}`, `ignore-wasm32`.
    A fence that names a language rustdoc leaves alone, such as `json`, is as it
    was written.
  - an indented code block in a block quote or a list item indented by three
    columns at most, as the generator did for one at the top, so that it is no
    code block.
  - `\<` for a `<` outside a code block and a code span, which the page shows as
    `<`. A `<script>` in a description was an element of your rendered docs, and
    `cargo doc` with warnings denied accepted it, because the generated module
    allows `rustdoc::all`. A description that is written in HTML, such as
    sevDesk's `<ul>`, `<li>`, `<br>` and `<a href=...>`, shows its tags as text
    where it showed a list, a break and a link, and an autolink such as
    `<https://example.com>` shows as text too. There is no setting to turn that
    back on, and a description in Markdown is the repair, in an Overlay:
    `[text](https://example.com)` is a link.
  - a backslash before the `[` of a link or an image whose destination is not
    relative and is not `http:`, `https:` or `mailto:`, so that the page shows
    the text, the brackets and the destination. `[a](javascript:alert(1))` was a
    link that ran the vendor's script on your page, in the spellings
    `JaVaScRiPt:` and `&#106;avascript:` too, and so was a reference link whose
    definition holds one. Any other scheme (`data:`, `file:`) shows as text as
    well, because the rule lists what is followed and not what is refused. A
    description in Markdown is still Markdown: its emphasis, its lists and its
    links to a page render. Repair: an `https` link, in an Overlay.
  - the code point, `\u{202e}`, for a bidirectional control (U+202A to U+202E
    and U+2066 to U+2069). rustc refuses the character in a comment with a lint
    that no `allow` reaches, so a description that held one stopped the build of
    the crate that holds the generated file.
  - a line break for a carriage return, alone or before a line feed. `rustfmt`
    refused the file for a carriage return alone, with a message that named the
    file and not the schema, and the bless stopped there.
  - the indentation that all the lines of a description share taken off, as
    rustdoc takes it off. The doc lines that typify writes itself, such as
    `/// Error types.`, lose their leading space.
  - one `///` line for each line of every doc comment of several lines, where it
    was a `/** */` comment: a description of several lines, and the doc of each
    wrapper in `ops.rs`, which holds the generator's paragraphs after the
    summary even when the summary is one line. `rustfmt` lays out the body of a
    block comment by rules of its own, which differ with the settings you format
    with (`hard_tabs` with a `tab_spaces` of 2 or 3 wrote a vendor's indented
    line as a code block) and with a `"` that runs over a line break, and rustc
    takes a column of stars out of one (`**bold**` was shown as `*bold**`). A
    `///` line is left as it stands. Each line with text in it has one space
    after the slashes, and so has a description of one line that starts with a
    `/`. A list the vendor bulleted with `*` keeps its `*`.

  Repair: run the bless step again and commit `types.rs` and `ops.rs`. The
  `generate` feature gains `pulldown-cmark` 0.11, which is the version rustdoc
  is built with, and its dependency `unicase`; a build that does not turn the
  feature on compiles neither.
- **A wrapper answers with the type its success responses state, and a request
  body can be a component.** The generator typed the answer from the first
  status from `200` to `299` of an operation, in document order, and read no
  other success response and no range. It typed that answer only when the
  response was written on the operation with a JSON body. Any other first
  response made the wrapper return `NoContent`, which takes an empty body or any
  JSON and drops it: a response that is a `$ref` to `#/components/responses/`,
  the range `2XX`, and a `200` listed after a `204`.
  `api.get_thing(5)?.send(&client)?` succeeded and the caller never saw the
  answer. A `200` listed first was typed, and then a `204` answer was an
  `Error::Decode` where the type refuses `null`. The generator reads every
  success response, which is a status
  from `200` to `299` and the range `2XX`, and follows a `$ref` to
  `#/components/responses/`, a chain of references included. `default` is not
  read.
  - A wrapper that returned `NoContent` for such a response returns its body
    type: `Thing` for a response that is a `$ref` to a component whose schema is
    `Thing`, `Vec<Thing>` for a `2XX` that states an array of them. A response
    that states a JSON body and no schema returns `serde_json::Value`. Code that
    names `NoContent` for such a call, as a return type or a pattern, no longer
    compiles. Repair: run the bless step again, commit `ops.rs` and `types.rs`,
    and name the type of the body.
  - An operation with a JSON success response beside one with no `content`, such
    as a `204` or a response component with no `content`, answers `Option` of
    the type, whichever is listed first. `None` is the answer with no body. A
    `204` answer was `NoContent`, or an `Error::Decode` where the type refuses
    `null`, by the order of the two. Repair: run the bless step again, commit
    `ops.rs`, and match `Ok(None)` of `Call::send` for the answer with no body.
    `Planned::send` answers `Option` of that, where `None` is a dry run, so on
    the planned road the answer with no body is `Ok(Some(None))`. A `match` with
    the arms `None` and `Some(t)` takes a dry run for a `204`.
  - The JSON bodies of the success responses have to agree, under every JSON
    media type of each response, because the client sends no `Accept` header and
    a server may answer with any of them. Two that do not state one schema leave
    the wrapper with no one type, so the bless is refused with
    `GenerateError::Operation` around `GenerateError::Unsupported`, naming the
    operation, the status and the media type of each body, the response
    component that states it where there is one, and what each states. A schema
    and a JSON body with no schema do not agree either. The first body decided,
    silently: a `200` with `application/json: A` and `application/vnd.x+json: B`
    was `A`, and a `200` with `Thing` beside a `204` with `application/json: {}`
    was `Thing`. Repair: an Overlay that gives the bodies one schema, or that
    removes the one the wrapper should not read. Two bodies that state the same
    schema, named or inline, are one answer, as sevDesk's two operations with a
    `200` and a `201` are.
  - A success response that cannot be followed (a reference to a response that
    the document does not hold, one that points inside a component, or a cycle)
    is refused by name with the reference. The wrapper answered `NoContent`, or
    the type of an earlier status, and did not follow it. A response whose
    schema is a `$ref` outside `#/components/schemas/` is
    refused with that reference. Repair: an Overlay that corrects or removes the
    reference.
  - `NoContent` stays the answer where no success response has a JSON body: a
    `204`, a response with no `content`, and `content` under a media type that
    is not JSON. A success response with a body that is not JSON adds nothing to
    the type, so a `200` with a JSON body beside a `201` with a PDF answers with
    the type of the `200`. A body that is not JSON is an `Error::Decode`, as it
    was, because the typed call reads JSON only.
  - A `requestBody` that is a `$ref` to `#/components/requestBodies/` is
    followed, as the command line has always followed it, so the bless is not
    refused with "requestBody $refs are not followed". The wrapper takes the
    type of the component's schema, and its argument is optional where the
    component is not `required`. The reduction and the generator read a
    `requestBody` through one function, so a reference one follows is a
    reference the other follows. A type that only such a body reaches stops
    requiring its `readOnly` properties, as one that a body stated on the
    operation reaches does. A schema that a component states inline is a type
    for each operation that takes the component, named after the operation.
    Repair: none. Run the bless step again and commit `ops.rs` and `types.rs`.
  - A name that two schemas stated inline take is one type where the schemas are
    equal, wherever each is stated, and it was refused as
    `GenerateError::OneType`. Under a `title`, a component that two operations
    share is one type that both take. So are three shapes that were refused: two
    operations that each state the same titled response schema inline, the same
    with a request body, and one operation whose body and response state the
    same titled schema. Different schemas under one `title` are still refused.
    So is one schema whose `required` names a `readOnly` property where a body
    and a response both state it, because the type of the body does not require
    that property, and typify is given two schemas. A refusal
    over a schema that a component states names the component and the operation
    that takes it. Repair: none. Run the bless step again and commit `ops.rs`
    and `types.rs`.
- **A nullable schema admits `null`, so its field is an `Option`.**
  `nullable: true` is how an OpenAPI 3.0 document says "or `null`", and the
  generator left it to typify, which reads it as a keyword it does not know. A
  required, nullable property was a plain type, and the vendor's own `null`
  failed to decode. Every shape that states the keyword is an `Option` of what
  the schema describes: a scalar, an enum, an object stated inline, an array and
  its items, a composition, a format you own through `Settings::replace`
  (`Option<money::Money>`) and the schema of a response. A property that is
  required and nullable is an `Option` whose key still has to be there. A schema
  that holds anything admits `null` already and stays `serde_json::Value`.
  Repair: run the bless step again, commit `types.rs` and `ops.rs`, and handle
  `None` where the code reads such a field. What changes in sevdesk-cli:
  - A required, nullable property is an `Option`:
    `ModelCheckAccountTransaction.payee_payer_name`,
    `ModelContactAddress.category` and `ModelCreditNote.address_country` were
    `String` and two structs. The fourth of its four, `entryDate` of
    `Model_CheckAccountTransactionResponse`, states `nullable` beside a `$ref`,
    which is not read (below), so it is `SevEntryDate` as it was.
  - An optional, nullable property that was an array, a map or an integer with
    a default is an `Option` too: it was a `Vec`, a `Map` or an `i64` that took
    its default where the key was missing and refused a `null`. These are ten
    fields such as `GetChangesFromModelResponse.objects` and
    `ModelContact.status`.
  - A component that is nullable is a newtype over an `Option`, because every
    use of it takes `null`: `SevCompareHash(String)` is
    `SevCompareHash(Option<SevCompareHashInner>)`, and the `pattern` and its
    `FromStr` are on `SevCompareHashInner`. Its three uses are optional fields
    and read `Option<SevCompareHash>`.
  - `nullable` beside a `$ref` is not read. OpenAPI 3.0.3 says that properties
    added to a Reference Object "SHALL be ignored", and the object model does
    not hold them, so `{ $ref: X, nullable: true }` is `X` in the type as it
    was. Repair: an Overlay that writes `allOf` of the one reference with
    `nullable: true` beside it.
  - A field of a nullable schema carries the schema's `description` as its own
    doc comment, where it carried it on the type alone.
- **A bless that is refused writes nothing.** `Settings::write_to` wrote the
  corrected document and `types.rs` before it emitted `ops.rs` and the blob, so
  a vendor revision that the generator refused left the new types beside the old
  wrappers and the old blob, a tree that compiled and described two APIs. It
  builds every artefact in memory first, `rustfmt` formatting the Rust there,
  and writes none of them until the document, both emitters, the blob's round
  trip and `rustfmt` have all said yes. A refused bless leaves the committed
  artefacts as they were, byte for byte. Each file is then written beside its
  destination as `<name>.bless-tmp` and renamed over it once every file is
  written, so a disk that fills part-way stops the bless before any file is
  replaced and the files beside are removed. Repair: none. What an adopter can
  see besides:
  - A file at a destination is replaced and not written through: a symlink
    there becomes a file, and the new file takes the permissions that the
    process creates a file with, where the old one kept its own.
  - `rustfmt` reads standard input, in the nearest directory above the file
    that exists, and so reads the same `rustfmt.toml` it read for the file.
    `GenerateError::RustfmtSpawn` also says that the input or the output of
    `rustfmt` failed.
  - `GenerateError::Write` also names a destination that is a directory, which
    is refused before any file is placed, and one that a file written beside it
    could not be renamed to.

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
- **A description in the vendor's document cannot run code in
  `cargo test --doc`, and cannot put a script on your pages.** A code block in a
  block quote or a list item, or behind an info string such as `{.json}`,
  `ignore-wasm32` or `rust2018`, was compiled and run by rustdoc in the crate
  that holds the generated `types.rs` and `ops.rs`: code from the document, on
  the machine of whoever ran the tests and in CI. A link to `javascript:` was a
  link on your rendered docs. *Changed* says what the generator writes. The
  guarantee is for the stable options of `rustfmt`. `wrap_comments` rewraps a
  doc comment, is a nightly option and is outside it: under it `rustfmt` can
  refuse a generated file, and the bless stops there with `rustfmt rejected the
  generated …`. Repair: run the bless step again and commit what it writes; if
  you set `wrap_comments`, leave it out of the `rustfmt.toml` beside the
  generated files.
