//! The clap tree, and the trip back from `ArgMatches` to a sent request.
//!
//! The tree is two levels: one subcommand per resource the document groups its
//! paths into, and one subcommand under that per operation — `vouchers update`
//! rather than `update-voucher`. Both names are decided while the document is
//! reduced and travel in the reduced model, so nothing here derives them.
//!
//! [`commands`] turns the document into those subcommands and [`dispatch`] runs
//! whichever one the user typed, so an adopter who wants the generated surface
//! and nothing else writes those two calls and renders the [`Outcome`].
//!
//! [`select`] and [`Selection::send`] are the same trip with a seam in the
//! middle, for an adopter who has a check of their own to make — holding the
//! body to a generated type, say — between what the user typed and what goes
//! out. [`dispatch`] is the two of them in order.
//!
//! The flag-to-wire-name translation lives here and nowhere else: below this
//! module the vocabulary is the document's own names, which is why a Rust
//! caller can use the same request builder without ever meeting a flag.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches, Command, ValueHint, builder::PossibleValuesParser};
use http::Uri;
use thiserror::Error;

use crate::model::{
    Body, COMMIT, Document, Effect, FIELD_PART, FILE_PART, Field, Gate, JSON_BODY, Location,
    Operation, Param, RAW_BODY,
};
use crate::names::CommandName;
use crate::plan::{Answers, Plan, PlanError};
use crate::scalar::Scalar;
use crate::transport::{HttpRequest, HttpResponse, SyncClient};
use crate::values::{Part, Payload, Values};

/// Something about the arguments the document cannot repair.
#[derive(Debug, Error)]
pub enum ArgError {
    #[error("cannot read {}: {source}", path.display())]
    ReadBody {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{} does not hold JSON: {source}", path.display())]
    ParseBody {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("`--{flag} {raw}` is not `NAME=VALUE`")]
    PartSyntax { flag: &'static str, raw: String },
}

/// One subcommand per group, in document order, holding the operations under
/// it in document order.
#[must_use]
pub fn commands(doc: &Document) -> Vec<Command> {
    let mut groups: Vec<(&CommandName, Vec<&Operation>)> = Vec::new();
    for op in doc {
        match groups.iter_mut().find(|(name, _)| *name == op.group()) {
            Some((_, under)) => under.push(op),
            None => groups.push((op.group(), vec![op])),
        }
    }
    groups
        .into_iter()
        .map(|(name, under)| group(name, under))
        .collect()
}

/// The subcommand for one group: a name the document grouped by, and every
/// operation it grouped under it.
fn group(name: &CommandName, under: Vec<&Operation>) -> Command {
    Command::new(name.as_str().to_owned())
        .about(format!("Operations on {name}"))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommands(under.into_iter().map(command))
}

/// The subcommand for one operation.
#[must_use]
pub fn command(op: &Operation) -> Command {
    let mut cmd = Command::new(op.command().as_str().to_owned());
    if let Some(summary) = op.summary() {
        cmd = cmd.about(summary.to_owned());
    }
    cmd = cmd.long_about(long_about(op));
    for param in op.params() {
        cmd = cmd.arg(param_arg(param));
    }
    cmd = body_args(cmd, op.body());
    if op.effect() == Effect::Write {
        cmd = cmd.arg(
            Arg::new(COMMIT)
                .long(COMMIT)
                .action(ArgAction::SetTrue)
                .help("Send the request. Without it this is a dry run that prints it"),
        );
    }
    gates(cmd, op)
}

/// The gate flags one operation names, added to a command of your own.
///
/// [`command`] puts them on the subcommand it builds, and this is that same
/// door: a verb you write yourself — one that fetches, decides, and then calls a
/// gated operation — adds them with this and spells nothing itself. One
/// definition rather than two is what keeps the two command lines from coming to
/// disagree about one operation, and an Overlay that renames a gate renames the
/// flag on both.
///
/// An operation that names no gate is handed back unchanged, so a caller does
/// not have to ask first:
///
/// ```rust,ignore
/// tree::gates(Command::new("finalize-voucher").arg(id).arg(commit), op)
/// ```
#[must_use]
pub fn gates(cmd: Command, op: &Operation) -> Command {
    op.gates()
        .iter()
        .fold(cmd, |cmd, gate| cmd.arg(gate_arg(gate)))
}

/// One named gate: a flag that has to be typed in addition to `--commit`.
///
/// `required(true)` rather than merely read at the gate, and that is the whole
/// point of naming a hazard: a dry run of the operation that mails a stranger
/// is still a command line somebody had to write `--email` on. The word comes
/// before the request exists, not after it is built.
fn gate_arg(gate: &Gate) -> Arg {
    Arg::new(gate.as_str().to_owned())
        .long(gate.as_str().to_owned())
        .action(ArgAction::SetTrue)
        .required(true)
        .help(format!(
            "Required, and demanded in addition to --commit: this operation \
             stands behind the `{gate}` gate"
        ))
}

/// The long help: what the document says, then the wire request this
/// subcommand stands for, so an agent can see the method and path without
/// opening the document.
fn long_about(op: &Operation) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    if let Some(text) = op.description().or_else(|| op.summary()) {
        out.push_str(text);
        out.push_str("\n\n");
    }
    let _ = write!(
        out,
        "{} {}  (operationId: {})",
        op.method(),
        op.path(),
        op.id()
    );
    if op.effect() == Effect::Write {
        out.push_str("\n\nThis operation writes. Without --commit it is a dry run.");
    }
    if !op.gates().is_empty() {
        let named: Vec<String> = op.gates().iter().map(|gate| format!("--{gate}")).collect();
        let _ = write!(
            out,
            "\n\nNamed gates: {}. Each one is required, and demanded in \
             addition to --commit.",
            named.join(", ")
        );
    }
    out
}

/// What the user answered at the gate, read off one operation's `ArgMatches`:
/// the write confirmation, and every named gate the operation carries.
///
/// A read carries neither flag, so there is nothing to ask it.
///
/// `matches` has to carry the operation's own flags — the subcommand
/// [`command`] built, or a hand-written verb offering the same ones — because a
/// flag clap never heard of is a panic rather than an answer. A verb of your own
/// builds them out of [`Operation::gates`] for exactly that reason: then there
/// is nothing to keep in step.
#[must_use]
pub fn answers(op: &Operation, matches: &ArgMatches) -> Answers {
    if op.effect() == Effect::Read {
        return Answers::new();
    }
    let mut answered = Answers::new();
    if matches.get_flag(COMMIT) {
        answered = answered.commit();
    }
    for gate in op.gates() {
        if matches.get_flag(gate.as_str()) {
            answered = answered.gate(gate.as_str());
        }
    }
    answered
}

/// Read one operation's arguments out of its `ArgMatches`, under the names the
/// document uses.
pub fn values(op: &Operation, matches: &ArgMatches) -> Result<Values, ArgError> {
    let mut values = Values::new();
    for param in op.params() {
        if let Some(raw) = matches.get_one::<String>(param.flag()) {
            values = values.param(param.name(), raw);
        }
    }
    Ok(values.body(payload(op, matches)?))
}

/// What running one operation produced.
///
/// The two arms are the gate's two answers, carried far enough to render: a
/// response that came back, or the request that was not sent. Printing is the
/// adopter's — this crate decides and executes, and hands the result over.
#[derive(Debug)]
pub enum Outcome {
    /// The request went out; this is what came back, status and all. A 4xx is
    /// an outcome, not an error: the body that came with it is what a caller
    /// needs.
    Sent(HttpResponse),
    /// A write without confirmation. Nothing was sent, and this is the exact
    /// request that a confirmed run would have sent.
    DryRun(HttpRequest),
}

/// Why an operation the user named did not run.
///
/// The transport's own error is the source of [`Self::Transport`] rather than
/// a variant of its own, so this enum does not grow a type parameter for the
/// client and an adopter's error type can absorb it whole.
#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("no command given")]
    NoCommand,
    #[error("no operation named `{group} {command}` in the document")]
    Unknown { group: String, command: String },
    #[error(transparent)]
    Arg(#[from] ArgError),
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error("transport: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// One operation the user named, with its arguments read and its gate answered
/// — everything needed to send it, and nothing sent yet.
///
/// This is the seam an adopter puts a check of their own into. `send` consumes
/// it, so the request is built once and cannot be sent twice.
#[derive(Debug)]
pub struct Selection<'d> {
    operation: &'d Operation,
    values: Values,
    answers: Answers,
}

impl<'d> Selection<'d> {
    /// The operation the subcommand named.
    #[must_use]
    pub fn operation(&self) -> &'d Operation {
        self.operation
    }

    /// The arguments, under the document's own names rather than the flags they
    /// arrived as. A body a generated type should vet is
    /// `selection.values().payload()`.
    #[must_use]
    pub fn values(&self) -> &Values {
        &self.values
    }

    /// What the user answered at the gate: the write confirmation, and each
    /// named gate the operation carries. A read answers nothing, because a read
    /// is asked nothing.
    #[must_use]
    pub fn answers(&self) -> &Answers {
        &self.answers
    }

    /// Build the request, put it to the gate, and do what the gate decided.
    pub fn send<C: SyncClient>(self, client: &C, base: &Uri) -> Result<Outcome, DispatchError> {
        match Plan::build(self.operation, base, self.values, &self.answers)? {
            Plan::Send(request) => client
                .send(request)
                .map(Outcome::Sent)
                .map_err(|error| DispatchError::Transport(Box::new(error))),
            Plan::DryRun(request) => Ok(Outcome::DryRun(request)),
        }
    }
}

/// Read the two subcommands the user typed, whichever command the groups were
/// mounted on.
///
/// `matches` belongs to that command: the root itself when the operations are
/// the whole CLI, or the `raw` subcommand when they sit under one. This
/// function reads the group below it and the operation below that, and never
/// looks above it, which is what lets the same tree mount anywhere.
pub fn select<'d>(doc: &'d Document, matches: &ArgMatches) -> Result<Selection<'d>, DispatchError> {
    let (group, under) = matches.subcommand().ok_or(DispatchError::NoCommand)?;
    let (command, args) = under.subcommand().ok_or(DispatchError::NoCommand)?;
    let operation = doc
        .by_command(group, command)
        .ok_or_else(|| DispatchError::Unknown {
            group: group.to_owned(),
            command: command.to_owned(),
        })?;
    Ok(Selection {
        values: values(operation, args)?,
        answers: answers(operation, args),
        operation,
    })
}

/// [`select`], then [`Selection::send`]: the whole generated surface in one
/// call, for an adopter with no check of their own to make.
pub fn dispatch<C: SyncClient>(
    doc: &Document,
    base: &Uri,
    client: &C,
    matches: &ArgMatches,
) -> Result<Outcome, DispatchError> {
    select(doc, matches)?.send(client, base)
}

fn payload(op: &Operation, matches: &ArgMatches) -> Result<Option<Payload>, ArgError> {
    match op.body() {
        Body::None => Ok(None),
        // A missing required body is `Invocation::new`'s to report, so that one
        // place decides what satisfies an operation.
        Body::JsonWhole { .. } => matches
            .get_one::<PathBuf>(JSON_BODY)
            .map(|path| read_json(path).map(Payload::Json))
            .transpose(),
        Body::Opaque { .. } => matches
            .get_one::<PathBuf>(RAW_BODY)
            .map(|path| read_bytes(path).map(Payload::Raw))
            .transpose(),
        Body::Multipart { .. } => parts(matches).map(|parts| parts.map(Payload::Multipart)),
        Body::JsonFields(fields) => Ok(Some(Payload::Json(assembled(fields, matches)?))),
    }
}

/// `--json-body` is the base document; per-field flags are merged over it, so a
/// flag beside a file is an edit rather than a value the CLI silently drops.
fn assembled(fields: &[Field], matches: &ArgMatches) -> Result<serde_json::Value, ArgError> {
    let mut body = match matches.get_one::<PathBuf>(JSON_BODY) {
        Some(path) => read_json(path)?,
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    let Some(object) = body.as_object_mut() else {
        return Ok(body);
    };
    for field in fields {
        let Some(raw) = matches.get_one::<String>(field.flag()) else {
            continue;
        };
        // The flag's value parser has already accepted this, so the same rule
        // is not applied twice with two error shapes; the second call is a
        // conversion, and `Invocation::new` re-checks the whole set anyway.
        if let Ok(value) = field.scalar().parse(raw) {
            object.insert(field.name().to_owned(), value);
        }
    }
    Ok(body)
}

/// `--file NAME=PATH` and `--field NAME=VALUE`, in the order they were given.
fn parts(matches: &ArgMatches) -> Result<Option<Vec<Part>>, ArgError> {
    let mut parts = Vec::new();
    for raw in strings(matches, FIELD_PART) {
        let (name, value) = split_part(FIELD_PART, raw)?;
        parts.push(Part::text(name, value));
    }
    for raw in strings(matches, FILE_PART) {
        let (name, path) = split_part(FILE_PART, raw)?;
        let path = PathBuf::from(path);
        let filename = path
            .file_name()
            .map_or_else(|| name.to_owned(), |f| f.to_string_lossy().into_owned());
        parts.push(Part::file(name, filename, read_bytes(&path)?));
    }
    Ok((!parts.is_empty()).then_some(parts))
}

fn strings<'m>(matches: &'m ArgMatches, id: &str) -> impl Iterator<Item = &'m String> {
    matches.get_many::<String>(id).into_iter().flatten()
}

/// `NAME=REST`, splitting at the first `=` so a value may contain more.
fn split_part<'r>(flag: &'static str, raw: &'r str) -> Result<(&'r str, &'r str), ArgError> {
    match raw.split_once('=') {
        Some((name, rest)) if !name.is_empty() => Ok((name, rest)),
        Some(_) | None => Err(ArgError::PartSyntax {
            flag,
            raw: raw.to_owned(),
        }),
    }
}

fn param_arg(param: &Param) -> Arg {
    // A document that describes nothing still knows where the value goes, and
    // a help page with an empty line beside a flag helps nobody.
    let described = param.description().map_or_else(
        || {
            Some(format!(
                "The `{}` {} parameter",
                param.name(),
                match param.location() {
                    Location::Path => "path",
                    Location::Query => "query",
                    Location::Header => "header",
                }
            ))
        },
        |text| Some(text.to_owned()),
    );
    value_arg(
        param.flag(),
        param.scalar(),
        help_line(
            described.as_deref(),
            param.scalar(),
            wire(param.renamed(), param.name()),
        ),
        param.required(),
    )
}

/// A flag that had to move aside says which wire name it carries.
fn wire(renamed: bool, name: &str) -> Option<String> {
    renamed.then(|| format!("sends `{name}`"))
}

fn body_args(cmd: Command, body: &Body) -> Command {
    match body {
        Body::None => cmd,
        Body::JsonFields(fields) => json_field_args(cmd, fields),
        Body::JsonWhole { required } => cmd.arg(file_arg(JSON_BODY, *required).help(
            "JSON body read from a file; `-` is stdin. This body is nested, so it has \
             no per-field flags",
        )),
        Body::Multipart { names, required } => multipart_args(cmd, names, *required),
        Body::Opaque {
            media_type,
            required,
        } => cmd.arg(file_arg(RAW_BODY, *required).help(format!(
            "Request body read from a file, sent verbatim as `{media_type}`; `-` is stdin. \
             This CLI does not assemble that media type"
        ))),
    }
}

/// One flag per scalar property, and `--json-body` as the base document they
/// are merged over.
fn json_field_args(cmd: Command, fields: &[Field]) -> Command {
    let mut cmd = cmd;
    for field in fields {
        let mut arg = value_arg(
            field.flag(),
            field.scalar(),
            help_line(
                field.description(),
                field.scalar(),
                wire(field.renamed(), field.name()),
            ),
            false,
        );
        if field.required() {
            arg = arg.required_unless_present(JSON_BODY);
        }
        cmd = cmd.arg(arg);
    }
    cmd.arg(file_arg(JSON_BODY, false).help(
        "JSON body read from a file; `-` is stdin. It is the base document: \
         the per-field flags above are merged over it, so a flag wins over \
         the same key in the file",
    ))
}

/// `--file` and `--field`, repeatable. The part names are the document's, but
/// they are advice rather than a constraint: a document that declares none (or
/// declares them wrongly) is common, and refusing would help nobody.
fn multipart_args(cmd: Command, names: &[String], required: bool) -> Command {
    let declared = if names.is_empty() {
        String::new()
    } else {
        format!(" The document declares: {}.", names.join(", "))
    };
    cmd.arg(
        Arg::new(FILE_PART)
            .long(FILE_PART)
            .value_name("NAME=PATH")
            .action(ArgAction::Append)
            .required(required)
            .value_hint(ValueHint::Other)
            .help(format!(
                "One file part of the multipart body, repeatable.{declared}"
            )),
    )
    .arg(
        Arg::new(FIELD_PART)
            .long(FIELD_PART)
            .value_name("NAME=VALUE")
            .action(ArgAction::Append)
            .value_hint(ValueHint::Other)
            .help("One text part of the multipart body, repeatable"),
    )
}

fn value_arg(flag: &str, scalar: &Scalar, help: Option<String>, required: bool) -> Arg {
    let mut arg = Arg::new(flag.to_owned())
        .long(flag.to_owned())
        .value_name(scalar.value_name())
        .required(required)
        // Never `FilePath`: a value flag is not a path, and a dynamic completer
        // that offers the working directory for `--currency` is worse than one
        // that offers nothing.
        .value_hint(ValueHint::Other);
    // The document's rule runs at parse time, so a bad amount reports itself
    // the way a bad enum value does — one error shape for one kind of mistake.
    arg = if let Scalar::Choice(values) = scalar {
        arg.value_parser(PossibleValuesParser::new(values))
    } else {
        let scalar = scalar.clone();
        arg.value_parser(move |raw: &str| {
            scalar
                .parse(raw)
                .map(|_| raw.to_owned())
                .map_err(|e| e.to_string())
        })
    };
    if let Some(help) = help {
        arg = arg.help(help);
    }
    arg
}

fn file_arg(flag: &'static str, required: bool) -> Arg {
    Arg::new(flag)
        .long(flag)
        .value_name("FILE")
        .required(required)
        .value_parser(clap::value_parser!(PathBuf))
        .value_hint(ValueHint::FilePath)
}

/// The description, then whatever the document constrains, then the wire name
/// if the flag had to move aside — each in brackets, none of them invented.
fn help_line(description: Option<&str>, scalar: &Scalar, wire: Option<String>) -> Option<String> {
    let notes: Vec<String> = scalar.note().into_iter().chain(wire).collect();
    match (description, notes.is_empty()) {
        (Some(text), true) => Some(text.to_owned()),
        (Some(text), false) => Some(format!("{text} ({})", notes.join("; "))),
        (None, true) => None,
        (None, false) => Some(notes.join("; ")),
    }
}

fn read_json(path: &Path) -> Result<serde_json::Value, ArgError> {
    let bytes = read_bytes(path)?;
    serde_json::from_slice(&bytes).map_err(|source| ArgError::ParseBody {
        path: path.to_owned(),
        source,
    })
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, ArgError> {
    let read = |source| ArgError::ReadBody {
        path: path.to_owned(),
        source,
    };
    if path == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes).map_err(read)?;
        return Ok(bytes);
    }
    std::fs::read(path).map_err(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_splits_at_the_first_equals_only() {
        assert_eq!(
            split_part(FILE_PART, "file=a=b").ok(),
            Some(("file", "a=b"))
        );
        assert_eq!(split_part(FIELD_PART, "k=").ok(), Some(("k", "")));
        assert!(split_part(FILE_PART, "nope").is_err());
        assert!(split_part(FILE_PART, "=path").is_err());
    }
}
