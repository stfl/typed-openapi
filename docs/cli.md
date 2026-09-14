# The command line

What a generated command tree offers a user, and how to mount it. The primer is
[the README](../README.md); this page is the reference for someone looking one
thing up.

## The shape of the tree

[`tree::commands`](../typed-openapi/src/tree.rs) turns a `Document` into a
two-level tree: one `clap::Command` per group, in document order, holding one
subcommand per operation, also in document order.

**The group** is a path segment — the first, unless every path shares it, in
which case the rule descends: a document served entirely under `/v1` does not
collapse into one group named `v1`. Descending stops at the first segment that
tells operations apart, and never onto a path parameter.

**The operation's own name** is the last literal segment below the group. Where
the path has none left to spend, the method decides:

| method | path addresses one resource | name |
|---|---|---|
| `GET` | yes | `get` |
| `GET` | no | `list` |
| `POST` | either | `create` |
| `PUT` | either | `update` |
| `PATCH` | either | `patch` |
| `DELETE` | either | `delete` |

So `PUT /vouchers/{id}` is `vouchers update` and `GET /vouchers/{id}/render` is
`vouchers render`, whatever the vendor called them. The name a vendor repeats
in every `operationId` — `renderVoucher`, `getVoucherById` — is the group, and
it is said once:

```console
$ toy raw vouchers --help
Operations on vouchers

Usage: toy raw vouchers [OPTIONS] <COMMAND>

Commands:
  list      List vouchers
  create    Create a voucher
  get       Fetch one voucher
  update    Replace a voucher
  enshrine  Finalize a voucher (irreversible)
  render    Render the voucher to PDF and store it on the server (this GET
            writes)
  archive   Archive a voucher (undocumented; vendor ships it)
  help      Print this message or the help of the given subcommand(s)
```

A group holding one operation stays a group, so every operation is reachable as
`<group> <name>` with no exception to learn.

### Naming an operation yourself

Two `x-cli-` markers on an operation overrule the rule: `x-cli-command`
replaces the operation's own name, `x-cli-group` replaces its group. Both are
written in an Overlay, like every other correction — see
[overlay.md](overlay.md#3-grouping-and-the-command-line).

They are also the only way out of a collision. Two operations reducing to one
`<group> <name>` is a `LoadError` at bless time, naming both `operationId`s:

```
`renderVoucher` and `renderVoucherPdf` are both `vouchers render` on the command line; give one of them an `x-cli-command`
```

Never a silent rename: a name that moves because a *second* operation arrived
is a name that moved without anyone asking.

## Mounting the tree

Where you hang the tree is yours.

**As the whole CLI.** [`examples/toy/cli/examples/root.rs`](../examples/toy/cli/examples/root.rs)
is the entire adopter-written surface — `commands` to build the tree,
`dispatch` to run whichever subcommand was typed:

```rust,ignore
let matches = Command::new("toy")
    .subcommands(tree::commands(api.document()))
    .get_matches();

match tree::dispatch(api.document(), api.base(), &client, &matches)? {
    Outcome::Sent(response) => ...,
    Outcome::DryRun(request) => ...,
}
```

**Under a name.** [`examples/toy/cli/src/app.rs`](../examples/toy/cli/src/app.rs)
mounts the same tree under `raw` and puts hand-written verbs beside it.
`tree::select` and `tree::dispatch` read the group below the `ArgMatches` they
are given and the operation below that, and never look above, which is what
lets the tree sit anywhere:

```rust,ignore
.subcommand(Command::new("raw").subcommands(tree::commands(api.document())))
```

**With a check of your own in the middle.** `dispatch` is `select` followed by
`Selection::send`. Splitting them gives you a `Selection` — the operation, the
arguments under the document's own names, and the gate's answer — with nothing
sent yet. [`examples/toy/cli/src/raw.rs`](../examples/toy/cli/src/raw.rs) uses
that seam to hold a JSON body to the generated Rust type before the request is
built.

The seam earns its keep because the library's own body check is a check of
*kind*, not of content: `check_body` in
[`src/request.rs`](../typed-openapi/src/request.rs) asks whether a JSON body
was supplied where JSON is wanted, and stops there. Run the same bad file
through both CLIs — `root` is the `dispatch` one, `toy raw` the `select` one:

```console
$ echo '{"total":"1.00","currency":"USD","status":"nope"}' > bad.json

$ root vouchers create --json-body bad.json
POST /vouchers HTTP/1.1
host: localhost:9999
content-type: application/json

{"total":"1.00","currency":"USD","status":"nope"}

dry run: nothing was sent. Add --commit to send it.

$ toy raw vouchers create --json-body bad.json
toy: createVoucher: the request body does not fit the schema the document declares
  caused by: unknown variant `nope`, expected one of `draft`, `open`, `paid`
```

`typed-openapi` knows the body is JSON; only the adopter's crate knows which
`struct` that JSON has to be.

## Flags

| what the document says | what the subcommand grows |
|---|---|
| a path, query or header parameter with a scalar schema | `--<name>`, required exactly when the parameter is |
| a JSON body that is an object of scalars only | one `--<property>` per property, plus `--json-body FILE` |
| any other JSON body — nested, an array, no schema | `--json-body FILE` alone |
| `multipart/form-data` | `--file NAME=PATH` and `--field NAME=VALUE`, both repeatable |
| any other media type | `--raw-body FILE`, sent verbatim under that media type |
| no request body | nothing |
| an operation that writes | `--commit` |

One nested property is enough to make the whole body `--json-body` only: no
sibling gets a flag the request builder would then throw away. `contacts create`
is the case — its `address` is an object, so there is no `--name`, and asking
for one is a clap error rather than a value silently dropped.

`-` as the path to `--json-body` or `--raw-body` reads stdin.

Some parameters have no flag at all, and the document is refused rather than
partly mounted: `in: cookie`, a parameter described by `content` instead of
`schema`, and a parameter whose schema is not one of the six scalar kinds each
produce a `LoadError::Parameter` when the document is read.

### Per-field flags are merged over `--json-body`

`--json-body` is the base document and the per-field flags are applied on top,
so a flag beside a file is an edit rather than a value the CLI drops:

```console
$ cat voucher.json
{"total":"1.00","currency":"USD","status":"draft","internal_ref":"AB-7"}

$ toy raw vouchers create --json-body voucher.json --currency EUR --total 99.99
POST /vouchers HTTP/1.1
host: localhost:9999
content-type: application/json

{"total":"99.99","currency":"EUR","status":"draft","internal_ref":"AB-7"}
```

Two limits. A `--json-body` file whose top-level value is not an object passes
through unchanged and the per-field flags are ignored. And because
`--json-body` satisfies clap for every required field, a file that is *missing*
one gets past the parser. In both cases whether anything then objects is the
seam above: `toy raw` refuses them and prints why, `root` builds the request.

### Renaming

`PUT /vouchers/{id}` takes an `id` in the path and an `id` in the body. clap
panics on a duplicate argument name, so the second claimant moves aside: the
body field becomes `--body-id`, and the flag that moved says which wire name it
carries.

```console
$ toy raw vouchers update --help
      --id <INT>
          The `id` path parameter

      --body-id <INT>
          Server-assigned id (sends `id`)
```

Each subcommand's namespace starts with `commit`, `json-body`, `raw-body`,
`file` and `field` already spent, so a document that names a field `commit`
renames instead of colliding at startup. A global flag the surrounding CLI adds
— `toy`'s `--base-url`, for instance — is not in that set.
[`src/names.rs`](../typed-openapi/src/names.rs) holds the rule: a name still
taken after the first prefix gains a counter, `body-id`, `body-id-3`.

## Value checking

A flag's value parser is the document's own schema, so a bad amount reports
itself at parse time the way a bad enum value does — one error shape for one
kind of mistake.

| the document says | value name | accepted |
|---|---|---|
| `type: string` | `<STRING>` | anything. A `pattern` reaches `--help` and is **not** enforced |
| `type: string`, `format: money` | `<AMOUNT>` | an optional `-`, digits, then optionally `.` and one or two more digits |
| `type: integer` | `<INT>` | an `i64`. `5.0` is refused |
| `type: number` | `<NUMBER>` | a finite `f64` |
| `type: boolean` | `<BOOL>` | `true` or `false` |
| `enum: [...]` on a string | `<STRING>` | one of the listed values, and these complete |

Enforcing an arbitrary ECMA-262 `pattern` would cost a regex engine; the one
constraint this crate does enforce is the money rule, in
[`src/scalar.rs`](../typed-openapi/src/scalar.rs).

```console
$ toy raw vouchers create --total 12.505 --currency EUR --status open
error: invalid value '12.505' for '--total <AMOUNT>': `12.505` is not an amount (digits, optionally `.` and one or two decimals)

$ toy raw vouchers create --total 12.50 --currency EUR --status void
error: invalid value 'void' for '--status <STRING>'
  [possible values: draft, open, paid]
```

## The gate

A read runs on sight. A write prints the exact bytes it would have sent and
stops, until `--commit`. The two are one value — `Plan::Send` and
`Plan::DryRun` carry the same built request — so a dry run cannot describe
something other than what a confirmed run sends.

Which operations write is the document's answer, not a guess from the method: a
safe method is a read unless the operation carries `x-cli-writes: true`, and
everything else is a write. The marker can only add writes, never remove them,
so the gate is default-closed. `renderVoucher` is a `GET` that stores a PDF,
and it is gated:

```console
$ toy raw vouchers render --id 5
GET /vouchers/5/render HTTP/1.1
host: localhost:9999
dry run: nothing was sent. Add --commit to send it.
```

Each subcommand's long help says so too, so an agent reading `--help` sees the
method, the path, the `operationId` and the gate without opening the document.
An operation the document does not describe has no subcommand at all.

## Shell completion

One line in [`main.rs`](../examples/toy/cli/src/main.rs) installs a dynamic
completer:

```rust,ignore
CompleteEnv::with_factory(|| app::root(&api)).complete();
```

The shell asks the binary what completes, so what it offers is whatever the
embedded document describes — enum values included — and there is nothing on a
user's machine to regenerate when the document moves.

```console
$ COMPLETE=bash toy >> ~/.bashrc
$ toy raw vouchers create --status <TAB>
draft  open  paid
```

`bash`, `elvish`, `fish`, `powershell` and `zsh`. `toy completions <SHELL>`
prints a static script for the same five instead, for a setup that wants a
file — that one is a snapshot of the tree at the moment it ran, and it does go
stale.

## Exit codes and stderr

`typed-openapi` returns an `Outcome` or an error and prints nothing; what
follows is what [`examples/toy/cli`](../examples/toy/cli/) makes of that, and
is worth copying.

| code | when |
|---|---|
| `0` | the request was sent and answered with a 2xx, or it was a dry run |
| `1` | the invocation failed, or the server answered with a non-2xx |
| `2` | clap refused the command line: an unknown flag, a missing required flag, or a value the document's schema rejects |

[`main.rs`](../examples/toy/cli/src/main.rs) prints an error's own message and
then walks the `source` chain, because the cause is where the detail is and an
agent reading stderr cannot ask for it afterwards:

```console
$ toy raw contacts create --json-body notes.txt
toy: notes.txt does not hold JSON: expected ident at line 1 column 2
  caused by: expected ident at line 1 column 2

$ toy raw vouchers get --id 5
toy: transport: io: Connection refused (os error 111)
  caused by: io: Connection refused (os error 111)
```

A 4xx is an outcome rather than a transport error — `Outcome::Sent` carries the
response, status and all — so `toy` prints the body that came with it on stderr
and exits 1. Your adapter has to let that body through: ureq turns a 4xx into an
error and discards the body unless it is built with
`http_status_as_error(false)`, which is why
[`client.rs`](../examples/toy/cli/src/client.rs) does.
