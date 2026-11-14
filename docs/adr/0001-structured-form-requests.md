# Structured form validation and encoding at the invocation seam

Status: accepted for implementation.

## Context

Raw JSON input and a generated Rust request must produce the same form request.
A caller can also construct public `Values` directly. Validation in a command
parser or only in generated types would leave that third path unchecked.
Existing scalar reduction chooses an enum separately from text constraints;
using it for form fields would lose conjunctions.

## Decision

`Body::Form` carries required literal names and both enum and text constraints.
The layout stamp changes when this model changes. Source reduction reads marked
forms before the OpenAPI object model can discard an unsupported keyword and
uses the existing reference resolver. Other body modes keep their reduction.

`Invocation::new` validates the complete JSON object without repair.
`Invocation::request` owns the form encoding: original-key byte order, one pass
of form component escaping, and the fixed content type. Generated wrappers
serialize a request into `Values::json`; raw commands read the same JSON payload.
Neither caller owns an encoder. No HTTP client is added to the library.

The generator builds a named operation-body input type from the reduced form
contract through its existing inline type naming path. It includes each string
rule and requires every key. Descriptive formats, defaults and examples are not
part of that input type. Original component schemas remain available unchanged
to JSON operations and responses, including their existing format replacements.
This avoids changing JSON semantics to implement the form input contract.

`Plan::decide`, `Call::plan` and `Planned::send` retain their authority. Encoding
adds no send decision and a plan retains the request that it sends.

## Consequences

Marked requests use named generated input types; unmarked forms keep their byte
interfaces. Unsupported marked schemas fail explicitly instead of falling back
to bytes. Form fields have their own reduced representation so enum, pattern
and length rules survive source loading and blob decoding together. The generic
toy, direct values and generated callers exercise these boundaries with literal
wire goldens. A consumer needs no endpoint-specific serializer.
