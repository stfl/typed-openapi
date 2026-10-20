# Security policy

## Reporting a vulnerability

Report it privately through GitHub's private vulnerability reporting: open the
repository's **Security** tab and choose **Report a vulnerability**, or go
straight to
<https://github.com/stfl/typed-openapi/security/advisories/new>.

Never report a vulnerability in a public issue, a pull request or a discussion.

## Supported versions

Only the latest 0.x release gets a fix. Before 1.0 a release can break the API,
so an older release is not patched.

## What counts

The project treats these as vulnerabilities:

- A write sent without its confirmation or a gate. A write is held back until
  its confirmation and every gate its operation names, and a request that goes
  out without them is the failure the crate exists to prevent.
- A hostile OpenAPI document that runs code in the adopter's build or tests, or
  that steers the generated code or the command line.
- A credential that reaches output.

## What to expect

This policy sets no time to answer a report and makes no promise beyond what it
says here.
