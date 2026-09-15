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
//! Not every subcommand is asked to run. `--json-body-template` asks what the
//! body of a nested one looks like, and [`select`] answers it from the reduced
//! model as an [`Asked::Template`] — no values read, no request built, no
//! client wanted. It is an arm of an enum rather than a flag left lying about
//! because an arm has to be rendered: a template nobody prints is the thing
//! this exists to replace.
//!
//! The seam also answers the gate on its own: [`Selection::plan`] builds the
//! request and returns what the gate decided, with nothing sent and no client
//! asked for. An adopter whose client costs something to build — a credential a
//! dry run has no use for — asks there, and builds one only for a request the
//! gate is letting through.
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
    Body, COMMIT, Document, Effect, FIELD_PART, FILE_PART, Field, Gate, JSON_BODY,
    JSON_BODY_TEMPLATE, Location, Operation, Param, RAW_BODY, Shape,
};
use crate::names::{CommandName, renamed};
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
    for arg in op.params().iter().filter_map(param_arg) {
        cmd = cmd.arg(arg);
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
    // A parameter with no flag is said here, where a flag would have been. The
    // document declares it and this CLI cannot supply it, and a user reading
    // `--help` is owed both halves of that.
    for param in op.params() {
        if let Shape::Unreachable(why) = param.shape() {
            let _ = write!(
                out,
                "\n\n`{}` has no flag: it is {why}. The request goes out without it.",
                param.name()
            );
        }
    }
    out
}

/// What the user answered at the gate, read off one operation's `ArgMatches`:
/// the write confirmation, and every named gate the operation carries.
///
/// A read carries neither flag, so there is nothing to ask it.
///
/// A flag the command never declared reads as unanswered, which is the closed
/// answer: it holds the request back. That is what makes this safe to point at
/// a command this crate did not build — a verb of your own, or one built before
/// an Overlay named a new gate. Give your own command its flags with [`gates`]
/// and the two stay in step by construction.
#[must_use]
pub fn answers(op: &Operation, matches: &ArgMatches) -> Answers {
    if op.effect() == Effect::Read {
        return Answers::new();
    }
    let mut answered = Answers::new();
    if flag(matches, COMMIT) {
        answered = answered.commit();
    }
    for gate in op.gates() {
        if flag(matches, gate.as_str()) {
            answered = answered.gate(gate.as_str());
        }
    }
    answered
}

/// Did the command line carry this flag?
///
/// `ArgMatches::get_flag` panics on an id the `Command` never declared, and a
/// panic is the wrong answer to "is this gate answered?". A command with no
/// flag for a gate has no answer for it, and no answer is `false` — the reading
/// the gate has everywhere else, and the one that holds the request back.
fn flag(matches: &ArgMatches, id: &str) -> bool {
    matches
        .try_get_one::<bool>(id)
        .ok()
        .flatten()
        .copied()
        .unwrap_or(false)
}

/// Read one operation's arguments out of its `ArgMatches`, under the names the
/// document uses.
pub fn values(op: &Operation, matches: &ArgMatches) -> Result<Values, ArgError> {
    let mut values = Values::new();
    for param in op.params() {
        let Shape::Flag { flag, .. } = param.shape() else {
            continue;
        };
        // `get_many` serves both kinds: a flag given once yields one value, and
        // a repeatable one yields every occurrence, in the order they were
        // typed — which is the order they go on the wire in.
        for raw in strings(matches, flag) {
            values = values.param(param.name(), raw);
        }
    }
    Ok(values.body(payload(op, matches)?))
}

/// What one subcommand produced, carried far enough to render.
///
/// Two of the arms are the gate's two answers — a response that came back, or
/// the request that was not sent. The third is the subcommand that was asked
/// what its body looks like and answered without running. Printing is the
/// adopter's: this crate decides and executes, and hands the result over.
///
/// Every arm has to be rendered somewhere, which is the point of putting the
/// template here rather than leaving it to a flag an adopter may or may not
/// read. A `--json-body-template` nothing prints is the flag this design exists
/// to avoid, and the compiler is what rules it out.
#[derive(Debug)]
pub enum Outcome {
    /// The request went out; this is what came back, status and all. A 4xx is
    /// an outcome, not an error: the body that came with it is what a caller
    /// needs.
    Sent(HttpResponse),
    /// A write without confirmation. Nothing was sent, and this is the exact
    /// request that a confirmed run would have sent.
    DryRun(HttpRequest),
    /// `--json-body-template`: the skeleton of the JSON body, as JSON text with
    /// no trailing newline. Nothing was built and nothing was sent — this
    /// never reached the gate, because there was no request to put to it.
    Template(String),
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
    /// The client refused the request or never got an answer, carrying the
    /// error the client itself returned.
    ///
    /// What the box costs is worth saying out loud: this crate cannot tell
    /// whether the request left, and on a write that is the difference between
    /// an operation that did nothing and one that may have done everything.
    /// Only an adapter can read that out of its own client's error, so the
    /// reading belongs above the seam. A failure nobody has classified is one
    /// that may have arrived, which is the only safe default to hold it at.
    ///
    /// A [`Recorder`](crate::Recorder) does not bend that rule — it is the
    /// other side of it. A scripted failure carries a [`Reach`](crate::Reach)
    /// because the script chose the failure rather than read one, and a
    /// `downcast_ref` here hands that state back with it, so a test drives both
    /// branches of an adopter's retry rule through this variant without the
    /// crate ever classifying a real client's error.
    ///
    /// Reading it is not shut off, though. The box holds `C::Error` exactly as
    /// the client returned it, so `downcast_ref` recovers it — and the type a
    /// catch site names is the error type the adopter's own seam fixes, not
    /// whichever client the call went through, because `&dyn SyncClient<Error
    /// = E>` fixes `E` for every client behind it. A caller who wants no box at
    /// all builds the request with [`Plan`] and sends it through the client's
    /// own `send`. `docs/client.md` shows both.
    #[error("transport: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// One operation the user named, with its arguments read and its gate answered
/// — everything needed to send it, and nothing sent yet.
///
/// This is the seam an adopter puts a check of their own into. It has two
/// exits: [`send`] builds the request, puts it to the gate and carries the
/// verdict out, and [`plan`] stops at the verdict and hands it over. Both
/// consume the selection, so the request is built once and cannot be sent
/// twice.
///
/// [`send`]: Selection::send
/// [`plan`]: Selection::plan
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

    /// Build the request and put it to the gate, with nothing sent.
    ///
    /// The gate decides before a client exists, and that is what this is for: a
    /// dry run is reached with no socket and no credential, so a user who has
    /// configured neither can still ask what a write would send. [`send`] wants
    /// a client in hand to carry the verdict out; this hands the verdict over.
    ///
    /// A [`Plan::Send`] goes out through the client's own `send`, where the
    /// error that comes back is the client's own type: nothing is boxed, so
    /// there is nothing to downcast. `docs/client.md` shows both routes.
    ///
    /// [`send`]: Selection::send
    pub fn plan(self, base: &Uri) -> Result<Plan, DispatchError> {
        Ok(Plan::build(
            self.operation,
            base,
            self.values,
            &self.answers,
        )?)
    }

    /// Build the request, put it to the gate, and do what the gate decided.
    ///
    /// [`plan`] is the first half on its own, for a caller who has to know the
    /// verdict before there is a client to hand over.
    ///
    /// [`plan`]: Selection::plan
    pub fn send<C: SyncClient>(self, client: &C, base: &Uri) -> Result<Outcome, DispatchError> {
        match self.plan(base)? {
            Plan::Send(request) => client
                .send(request)
                .map(Outcome::Sent)
                .map_err(|error| DispatchError::Transport(Box::new(error))),
            Plan::DryRun(request) => Ok(Outcome::DryRun(request)),
        }
    }
}

/// What one subcommand was asked for.
///
/// Two questions reach a subcommand and only one of them is an operation to
/// run, so only one of them produces a [`Selection`] — the value that exists in
/// order to be sent. Asking what a body looks like produces the text and
/// nothing else: no values are read, no file is opened, no request is built and
/// there is no exit here that takes a client.
///
/// That is what keeps [`Plan::decide`] the only place deciding whether a
/// request goes out. This route does not reach a second decision; it reaches no
/// request at all.
///
/// [`Plan::decide`]: crate::Plan::decide
#[derive(Debug)]
pub enum Asked<'d> {
    /// Run the operation: the arguments are read and the gate is answered.
    Run(Selection<'d>),
    /// `--json-body-template`: the skeleton of the JSON body, borrowed from the
    /// reduced model that carries it.
    Template(&'d str),
}

/// Read the two subcommands the user typed, whichever command the groups were
/// mounted on.
///
/// `matches` belongs to that command: the root itself when the operations are
/// the whole CLI, or the `raw` subcommand when they sit under one. This
/// function reads the group below it and the operation below that, and never
/// looks above it, which is what lets the same tree mount anywhere.
pub fn select<'d>(doc: &'d Document, matches: &ArgMatches) -> Result<Asked<'d>, DispatchError> {
    let (group, under) = matches.subcommand().ok_or(DispatchError::NoCommand)?;
    let (command, args) = under.subcommand().ok_or(DispatchError::NoCommand)?;
    let operation = doc
        .by_command(group, command)
        .ok_or_else(|| DispatchError::Unknown {
            group: group.to_owned(),
            command: command.to_owned(),
        })?;
    // Before the arguments are read, because reading them opens whatever file
    // `--json-body` names: a user asking what a body looks like has no such
    // file yet, which is the whole reason they are asking.
    if let Some(template) = wanted_template(operation, args) {
        return Ok(Asked::Template(template));
    }
    Ok(Asked::Run(Selection {
        values: values(operation, args)?,
        answers: answers(operation, args),
        operation,
    }))
}

/// The skeleton of this operation's body, when the user asked for it.
///
/// The answer comes off the reduced model, where a bless step wrote it. Nothing
/// is walked and no schema is read — a shipped binary has no reader compiled
/// into it, which is the rule both command names already follow.
///
/// A body that carries no template grows no flag, so the two cannot disagree:
/// this reads the flag only for the body that has something to print, and
/// [`flag`] reads one the command never declared as absent rather than
/// panicking.
fn wanted_template<'d>(op: &'d Operation, matches: &ArgMatches) -> Option<&'d str> {
    let Body::JsonWhole {
        template: Some(template),
        ..
    } = op.body()
    else {
        return None;
    };
    flag(matches, JSON_BODY_TEMPLATE).then_some(template.as_str())
}

/// [`select`], then [`Selection::send`]: the whole generated surface in one
/// call, for an adopter with no check of their own to make.
pub fn dispatch<C: SyncClient>(
    doc: &Document,
    base: &Uri,
    client: &C,
    matches: &ArgMatches,
) -> Result<Outcome, DispatchError> {
    match select(doc, matches)? {
        Asked::Run(selection) => selection.send(client, base),
        Asked::Template(template) => Ok(Outcome::Template(template.to_owned())),
    }
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

/// The flag one parameter grows — and nothing at all for one this CLI cannot
/// supply, which the subcommand's long help names instead, where a dead flag
/// would otherwise have stood.
fn param_arg(param: &Param) -> Option<Arg> {
    let Shape::Flag {
        flag,
        location,
        scalar,
        join,
        // The kind the document calls this value is not a rule about it, and
        // the help line carries rules: every note beside a flag is something
        // `Scalar::parse` refuses a value for, and a format is something
        // nothing here can refuse a value for.
        format: _,
    } = param.shape()
    else {
        return None;
    };
    // A document that describes nothing still knows where the value goes, and
    // a help page with an empty line beside a flag helps nobody.
    let described = param.description().map_or_else(
        || {
            Some(format!(
                "The `{}` {} parameter",
                param.name(),
                match location {
                    Location::Path => "path",
                    Location::Query => "query",
                    Location::Header => "header",
                }
            ))
        },
        |text| Some(text.to_owned()),
    );
    let notes = join
        .map(|join| join.note().to_owned())
        .into_iter()
        .chain(wire(renamed(flag, param.name()), param.name()));
    let mut arg = value_arg(
        flag,
        scalar,
        help_line(described.as_deref(), scalar, notes),
        param.required(),
    );
    if join.is_some() {
        arg = arg.action(ArgAction::Append);
    }
    Some(arg)
}

/// A flag that had to move aside says which wire name it carries.
fn wire(renamed: bool, name: &str) -> Option<String> {
    renamed.then(|| format!("sends `{name}`"))
}

fn body_args(cmd: Command, body: &Body) -> Command {
    match body {
        Body::None => cmd,
        Body::JsonFields(fields) => json_field_args(cmd, fields),
        Body::JsonWhole { required, template } => {
            let cmd = cmd.arg(file_arg(JSON_BODY, *required).help(
                "JSON body read from a file; `-` is stdin. This body is nested, so it has \
                 no per-field flags",
            ));
            match template {
                None => cmd,
                Some(_) => cmd.arg(template_arg()),
            }
        }
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

/// `--json-body-template`: the shape of the file `--json-body` wants.
///
/// A flag on an operation that does not run the operation is an odd thing, and
/// it earns the place by being the only one a user can redirect: the skeleton
/// goes to stdout as it stands, where a `--help` page would rewrap it and a
/// `long_about` would put a usage block around it. It sits on the subcommand
/// because that is where the question is asked — the user is already typing the
/// operation whose body they cannot spell.
///
/// `exclusive`, which is clap answering two of the three things this flag has to
/// be true of, before any code of this crate's runs. Asking what a body looks
/// like is not running the operation, so the operation's own required flags — a
/// path parameter, a required body, every gate it names — are not demanded for
/// it; and `--commit` beside it is refused rather than quietly ignored, because
/// a command line that confirms a write *and* asks what the write would look
/// like is two commands, and only the user can say which one they meant.
///
/// The third thing — that nothing is sent — is not clap's to promise. It holds
/// because [`select`] answers this from the reduced model and hands back an
/// [`Asked::Template`], which carries no values, builds no request and has no
/// exit that takes a client. [`Plan::decide`] stays the only place that decides
/// whether a request goes out, because this way round there is no request for
/// it to decide about.
///
/// [`Plan::decide`]: crate::Plan::decide
fn template_arg() -> Arg {
    Arg::new(JSON_BODY_TEMPLATE)
        .long(JSON_BODY_TEMPLATE)
        .action(ArgAction::SetTrue)
        .exclusive(true)
        .help(
            "Print a skeleton of the JSON body and stop, for --json-body to be filled \
             in from. Required properties only, with empty values. Nothing is built \
             and nothing is sent, so this takes no other flag",
        )
}

/// The description, then whatever the document constrains, then whatever else
/// the caller has to add — how a repeatable flag is joined, the wire name a flag
/// that moved aside carries — each in brackets, none of them invented.
fn help_line(
    description: Option<&str>,
    scalar: &Scalar,
    extra: impl IntoIterator<Item = String>,
) -> Option<String> {
    let notes: Vec<String> = scalar.note().into_iter().chain(extra).collect();
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
