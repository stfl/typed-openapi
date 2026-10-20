# Changelog

Every change an adopter of `typed-openapi` can see after 0.0.2 is listed here,
with the repair it asks for. The format follows [Keep a Changelog][keep], and
the crate follows [Semantic Versioning][semver].

[keep]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## Unreleased

### Added

- **`OverlayError::Shape`.** An Overlay that is YAML or JSON and has the wrong
  shape (no `actions`, an action with no `target`, a version other than 1.1) is
  `Shape`. It was `Syntax`, whose message says the text "is not valid YAML or
  JSON". Repair: where you reported `Syntax` for a wrong shape, report `Shape`
  too.
- **`overlay::Cause`**, the payload of every `OverlayError` variant: the
  `message()`, and the `line()` and `column()` of the text, or the `action()`
  index in the Overlay, where the thing that refused said them.

### Changed

- **The YAML reader is serde-saphyr 2.0 in place of `serde_yaml_ng`.** Every
  document and every Overlay reads into the same value as before, and `yes`,
  `no`, `on`, `off`, `y` and `n` stay strings, as does `<<` as a key. Five
  things differ:
  - A mapping that repeats a key is refused, with the line and column of the
    second one. The last value used to win in silence. Repair: delete the key
    you do not mean.
  - `1_000` reads as the number 1000 and `017` as the float 17. Both were
    strings. Repair: quote the value where it is a string.
  - `.inf` and `.nan` are refused where they stand. They read as `null`.
    Repair: quote the value or remove it.
  - The messages for a text that is not YAML are the new reader's words, and
    each names its line and column.
  - A document nested to the limit of 128 levels needs up to 4 MiB of stack in
    a debug build and up to 512 KiB in a release build, where the old reader
    needed under 1 MiB. On a thread with less, a document nested to the limit
    or past it stops the process and is not returned as an error. Repair: read
    documents on the main thread, or on a thread with 4 MiB or more.

  The `document` feature names `serde-saphyr` where it named `serde_yaml_ng`.
  Repair: a test of yours that read YAML with `serde_yaml_ng` names its own
  dependency, or calls `overlay::parse`.
- **No type of the YAML reader, of `roas-overlay` or of typify is in a public
  signature.** Each `OverlayError` variant holds an `overlay::Cause`.
  `GenerateError::Yaml` and `GenerateError::Typify` hold a `Box<dyn Error + Send
  + Sync>`, and the cause is still `source()`. Repair: a `match` that bound
  `serde_yaml_ng::Error`, `roas_overlay::validation::Error`,
  `roas_overlay::apply::ApplyError` or `typify::Error` reads the `Cause`, or
  prints `source()`.
- **A bless step may write the overlaid YAML with different text.** The emitter
  double-quotes every string a YAML 1.1 reader would misread (`"N"`, `"yes"`,
  `"1.0"`) and folds long strings into `>-` blocks. The value is the same, read
  as JSON. Repair: run the bless step again and commit the result.
- **`rust-version` is 1.89**, the floor serde-saphyr declares. CI builds and
  tests the crate on current stable.
- **An error describes its own layer and leaves the cause to `source()`.** Every
  `#[error]` string that printed the `#[source]` or `#[from]` field it also
  marks stops where its own layer stops, so a walk down the chain (the toy's
  `main` and bless step, sevdesk-cli's `error::chain`, anyhow's `{:#}`) prints
  each cause once. Repair: walk `source()` wherever an error is reported, and
  read the cause in a test with `error.source()`; printing `{error}` alone no
  longer shows it. Most messages lose only the `: <cause>` they ended with:
  `OverlayError`, `PlanError::Request`, `DocumentError::Blob`, `ArgError`,
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
