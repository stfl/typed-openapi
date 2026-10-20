# Changelog

Every change an adopter of `typed-openapi` can see after 0.0.2 is listed here,
with the repair it asks for. The format follows [Keep a Changelog][keep], and
the crate follows [Semantic Versioning][semver].

[keep]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## Unreleased

### Changed

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
  - `ValueError::BadValue`: `<op>: `<name>` was given a value the document
    refuses`
  - `LoadError::Unresolvable`: `<op>: `<name>` has a `$ref` that cannot be
    followed`
  - `LoadError::Unrunnable`: `<op>: `<name>` states a rule that cannot run`
  - `client::Error::Transport` and `DispatchError::Transport`: `the transport
    failed`
  - `GenerateError::Read`, `Write` and `RustfmtSpawn`: `cannot read <path>`,
    `cannot write <path>` and `cannot run rustfmt on <path>`
  - `GenerateError::Overlay`: `cannot use <path>`
  - `GenerateError::Operation`: `the wrapper for <op> cannot be generated`
- **The error enums are `#[non_exhaustive]`:** `LoadError`, `ValueError`,
  `client::Error`, `DispatchError`, `ArgError`, `PlanError`, `OverlayError` and
  `DocumentError`. A `match` on one needs a `_` arm, and a refusal added later
  is a new variant, not a break. A match on an error is the same in every
  feature build, `_` arm included.

### Removed

- **Items that only the bless step used are private to the crate.** None had a
  user outside it, and no replacement is offered:
  - the `schema` module (`resolve`, `resolve_schema`, `scalar_of`,
    `description_of`, `format_of`, `template`, `is_media_type`, `is_json` and
    `is_multipart`); `schema::RefError`, which `LoadError` carries, is
    `typed_openapi::RefError`
  - `names::Grouping`, `names::Namespace`, `names::Protected` and
    `names::Clash`
  - `scalar::Numeric`
  - the `multipart` module (`encode` and `Encoded`)
  - `kebab`, at the root and in `names`
