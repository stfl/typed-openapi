# Structured form requests

An operation with `x-cli-form-json: true` reads JSON input and sends
`application/x-www-form-urlencoded` bytes. Without the marker, a form operation
accepts pre-encoded bytes through `--raw-body` and `Vec<u8>`.

The marker belongs on an operation directly under `paths`, and its value must
be boolean `true`. The request body must be required and declare exactly
`application/x-www-form-urlencoded`. Its schema must reference a named component
object with explicit `additionalProperties: false`. All properties must be
required, non-nullable strings. Ordinary component and request-body references
are followed. Read-only properties, optional properties, compositions, nested
objects, arrays, non-string fields and form encoding directives are refused.
Unsupported validation keywords and lengths that the generator cannot represent
are refused with the operation and reason.

String `enum`, `pattern`, `minLength` and `maxLength` rules apply together.
Lengths count Unicode characters, including combining characters separately.
Formats are descriptive. Defaults and examples do not fill missing keys, and a
singleton enum does not supply a value. Unknown keys and non-string values
are errors. An empty string is accepted where its rules permit it.

## One input, two callers

The toy's `submitForm` operation accepts this complete JSON object:

```json
{"choice":"1","digits":"42","kind":"Toy","ref[id]":"A+B %&=ä","text":" ~*._-\n"}
```

Save it as `form.json`. Inspect the request, then send it by adding `--commit`:

```sh
toy raw forms create --json-body form.json --toy-form
```

Help describes JSON input and form wire output. This mode offers neither
per-field flags nor `--raw-body`. The generated validator checks its named
request type, and the shared invocation checks the form contract before a plan
can exist, including for direct `Values::json` input.

The Rust operation takes the generated named request. Its optional builder
uses the same type:

```rust,ignore
let body: api::SubmitFormBody = serde_json::from_str(input)?;
let call = api.submit_form(&body)?;
let planned = call.plan(&typed_openapi::Answers::new().gate("toy-form"))?;
println!("{}", typed_openapi::render(planned.request()));
```

The request type is named through the generator's operation-body naming path.
It is independent of the original component type: form input cannot inherit a
component's defaults, native format types or adopter-owned format replacements.
JSON operations using that component keep their generated component type.

## Wire bytes

The example sends these bytes, with no terminal newline:

```text
choice=1&digits=42&kind=Toy&ref%5Bid%5D=A%2BB+%25%26%3D%C3%A4&text=+%7E*._-%0A
```

Pairs are sorted by the original key's UTF-8 bytes. Each name and value is
encoded once: ASCII letters, digits, `*`, `-`, `.` and `_` stay literal; spaces
become `+`; other UTF-8 bytes use uppercase percent escapes. Brackets are literal
characters in a flat key. The content type is exactly
`application/x-www-form-urlencoded`, with no parameters.

Raw and typed calls use the same invocation encoder. Planning retains the
request that sending consumes. A dry run makes no transport call, and named
gates and confirmation keep their existing authority. See
[the encoder decision](adr/0001-structured-form-requests.md) and
[CLI body flags](cli.md#flags).
