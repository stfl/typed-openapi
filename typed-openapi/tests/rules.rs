//! Every rule the document states about a value is enforced in one place and
//! rendered in one place.
//!
//! `Scalar::parse` is the one check and `Scalar::note` the one rendering, so a
//! rule that reaches `--help` is a rule that refuses a value, and a refusal
//! says the rule the help line advertised. Each case is the input of a probe of
//! SEV-249 (`A3b/scalars` and `A7/docs/rules.yaml`). No case reads the archive.
//! A case that refuses a document or a value asserts the variant beside the
//! words.

#![expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]

use std::error::Error;

use clap::error::ErrorKind;
use clap::{ArgMatches, Command};
use typed_openapi::request::ValueError;
use typed_openapi::scalar::ScalarError;
use typed_openapi::tree::{self, Asked, DispatchError};
use typed_openapi::{Document, Invocation, LoadError, Plan, PlanError, Values};

/// A document with one server and whatever the case puts under `paths`.
fn document(paths: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n{paths}"
    )
}

/// One read with the parameters the case lists, one per line.
fn read(parameters: &[&str]) -> Document {
    let mut paths =
        String::from("  /entries:\n    get:\n      operationId: listEntries\n      parameters:\n");
    for parameter in parameters {
        paths.push_str("        - ");
        paths.push_str(parameter);
        paths.push('\n');
    }
    paths.push_str("      responses: { \"200\": { description: OK } }\n");
    Document::load(&document(&paths), &[]).expect("the document loads")
}

/// The message of the cause under `error`: an error describes its own layer, and
/// the value and the rule are one level down.
fn cause(error: &dyn Error) -> String {
    error
        .source()
        .expect("the layer has a cause under it")
        .to_string()
}

/// Why `op` refuses `values`.
fn refusal(doc: &Document, values: Values) -> ValueError {
    let op = doc.get("listEntries").expect("the operation");
    Invocation::new(op, values).expect_err("the document refuses the values")
}

fn root(doc: &Document) -> Command {
    Command::new("p")
        .subcommand_required(true)
        .subcommands(tree::commands(doc))
}

/// What clap makes of an operator's command line.
fn line(doc: &Document, args: &[&str]) -> Result<ArgMatches, clap::Error> {
    root(doc).try_get_matches_from(std::iter::once("p").chain(args.iter().copied()))
}

/// The page `--help` prints for the command the operator names.
fn help(doc: &Document, command: &[&str]) -> String {
    let mut args = command.to_vec();
    args.push("--help");
    let page = line(doc, &args).expect_err("--help stops the parse");
    assert_eq!(page.kind(), ErrorKind::DisplayHelp);
    // clap wraps the page to the terminal, so a sentence arrives in two pieces
    // unless every run of whitespace is one space.
    page.to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Everything an operator's command line goes through before a request is sent.
fn planned(doc: &Document, args: &[&str]) -> Result<Plan, DispatchError> {
    let matches = line(doc, args).expect("clap accepts the line");
    match tree::select(doc, &matches)? {
        Asked::Run(selection) => selection.plan(doc.base()),
        Asked::Template(_) => panic!("this command line runs the operation"),
    }
}

/// A flag given once for each of `values`.
fn repeated<'a>(flag: &'a str, values: &[&'a str]) -> Vec<&'a str> {
    values.iter().flat_map(|value| [flag, *value]).collect()
}

/// The query of the request that an operator's command line makes.
fn query(doc: &Document, args: &[&str]) -> Option<String> {
    let plan = planned(doc, args).expect("the command line is accepted");
    plan.request().uri().query().map(ToOwned::to_owned)
}

/// Why `args` make no request, the cause the operator reads under the head.
fn stopped(doc: &Document, args: &[&str]) -> (ValueError, String) {
    let Err(DispatchError::Plan(PlanError::Value(error))) = planned(doc, args) else {
        panic!("{args:?}: the command line is refused for a value");
    };
    let reason = cause(&error);
    (error, reason)
}

/// `probes/A3b/scalars` (`multiple`): `multipleOf: 0.01` is how an amount in cents
/// is written, and a binary float refuses 159 of 1000 of them with a false reason
/// (`0.07 / 0.01` is `7.000000000000001`). Divisibility is decided on the digits
/// of the value and of the step, so the amounts a person writes are accepted on
/// both roads and are sent as they were checked.
#[test]
fn an_amount_in_cents_is_a_multiple_of_a_cent() {
    let doc = read(&["{ name: amount, in: query, schema: { type: number, multipleOf: 0.01 } }"]);
    let op = doc.get("listEntries").expect("the operation");
    for raw in ["0.07", "0.29", "19.99"] {
        let request = Invocation::new(op, Values::new().param("amount", raw))
            .unwrap_or_else(|error| panic!("{raw}: {error}"))
            .request(doc.base())
            .expect("a request");
        assert_eq!(
            request.uri().query(),
            Some(format!("amount={raw}").as_str())
        );
    }

    let refused = refusal(&doc, Values::new().param("amount", "0.005"));
    assert!(
        matches!(&refused, ValueError::BadValue { name, source: ScalarError::Rule { .. }, .. }
            if name == "amount"),
        "{refused:?}"
    );
    assert_eq!(cause(&refused), "`0.005` is not a multiple of 0.01");
}

/// The five steps that are refused, on a query parameter and on a property of a
/// flat body: a step of 0 has no multiples, whether it is spelled `0` or `0.0`,
/// and a negative step is no step. A step of `-1` is the one with which the
/// remainder of `i64::MIN` overflows, if the load lets it through.
const STEPS: &[(&str, &str, &str)] = &[
    ("integer", "0", "0"),
    ("integer", "-1", "-1"),
    ("number", "0", "0"),
    ("number", "-0.01", "-0.01"),
    ("number", "0.0", "0"),
];

#[test]
fn a_multiple_of_that_is_not_positive_is_refused_while_the_document_is_reduced() {
    for (kind, step, shown) in STEPS {
        let schema = format!("{{ type: {kind}, multipleOf: {step} }}");
        let paths = format!(
            "  /entries:\n    get:\n      operationId: listEntries\n      parameters:\n        - {{ name: \
             amount, in: query, schema: {schema} }}\n      responses: {{ \"200\": {{ description: OK }} }}\n"
        );
        let error = Document::load(&document(&paths), &[])
            .expect_err("a step that is not positive is no rule");
        assert!(
            matches!(&error, LoadError::Unrunnable { op, name, source: ScalarError::Rule { raw, .. } }
                if op == "listEntries" && name == "amount" && raw == shown),
            "{kind} {step}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "listEntries: `amount` states a rule that cannot run"
        );
        assert_eq!(
            cause(&error),
            format!("`{shown}` is not a step: `multipleOf` has to be more than 0")
        );

        let paths = format!(
            "  /entries:\n    post:\n      operationId: createEntry\n      requestBody:\n        \
             required: true\n        content:\n          application/json:\n            schema:\n              \
             type: object\n              properties:\n                amount: {schema}\n      \
             responses: {{ \"201\": {{ description: Created }} }}\n"
        );
        let error = Document::load(&document(&paths), &[])
            .expect_err("the same step on a body property is refused the same way");
        assert!(
            matches!(&error, LoadError::Unrunnable { op, name, source: ScalarError::Rule { .. } }
                if op == "createEntry" && name == "amount"),
            "{kind} {step}: {error:?}"
        );
    }
}

/// A positive step loads, so the refusal above is about the sign and not about
/// the keyword.
#[test]
fn a_multiple_of_that_is_positive_loads() {
    for schema in [
        "{ type: integer, multipleOf: 1 }",
        "{ type: integer, multipleOf: 5 }",
        "{ type: number, multipleOf: 0.01 }",
        "{ type: number, multipleOf: 5e-324 }",
    ] {
        read(&[&format!("{{ name: amount, in: query, schema: {schema} }}")]);
    }
}

/// The patterns whose meaning the Unicode flag changes. Without the flag `\p{L}`
/// is the text `p{L}` and `\u{41}` is the letter `u` forty-one times, and with it
/// they are a property and a code point. The command line and the generated type
/// run the pattern without the flag, and a vendor's server may run it with, so
/// the two sides could disagree about a name such as `Müller`.
const UNICODE: &[(&str, &str)] = &[
    (r"^\p{L}+$", r"\p{"),
    (r"^\P{L}+$", r"\P{"),
    (r"^\u{41}$", r"\u{"),
    (r"[\p{L}\d]+", r"\p{"),
    (r"^(?:\p{Lu}\p{Ll}*)$", r"\p{"),
    (r"\\\p{L}", r"\p{"),
];

#[test]
fn a_pattern_whose_meaning_depends_on_the_unicode_flag_is_refused_by_name() {
    for (pattern, escape) in UNICODE {
        let paths = format!(
            "  /people:\n    get:\n      operationId: listPeople\n      parameters:\n        - {{ name: \
             surname, in: query, schema: {{ type: string, pattern: '{pattern}' }} }}\n      \
             responses: {{ \"200\": {{ description: OK }} }}\n"
        );
        let error = Document::load(&document(&paths), &[])
            .expect_err("the pattern means one thing with the flag and another without");
        assert!(
            matches!(&error, LoadError::Unrunnable { op, name, source: ScalarError::Rule { raw, .. } }
                if op == "listPeople" && name == "surname" && raw == pattern),
            "{pattern}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "listPeople: `surname` states a rule that cannot run"
        );
        assert_eq!(
            cause(&error),
            format!(
                "`{pattern}` holds `{escape}`, which means one thing with the Unicode flag and \
                 another without it, and a generated type cannot be given the flag"
            ),
            "{pattern}"
        );
    }
}

/// The same pattern on a property of a flat body is refused the same way, and
/// names the operation and the property.
#[test]
fn a_unicode_pattern_on_a_body_property_is_refused_by_name() {
    let paths = "  /people:\n    post:\n      operationId: createPerson\n      requestBody:\n        \
         required: true\n        content:\n          application/json:\n            schema:\n              \
         type: object\n              properties:\n                surname: { type: string, pattern: \
         '^\\p{L}+$' }\n      responses: { \"201\": { description: Created } }\n";
    let error = Document::load(&document(paths), &[]).expect_err("the pattern is refused");
    assert!(
        matches!(&error, LoadError::Unrunnable { op, name, source: ScalarError::Rule { raw, .. } }
            if op == "createPerson" && name == "surname" && raw == r"^\p{L}+$"),
        "{error:?}"
    );
}

/// An `enum` that lists `null` and nothing a flag can send states no value to
/// take. JSON Schema admits no integer under it, and reading it as no rule at all
/// would admit every one, so the document is refused by name on a parameter and on
/// a property of a flat body.
#[test]
fn an_enum_of_null_alone_is_refused_while_the_document_is_reduced() {
    for schema in [
        "{ type: integer, enum: [null] }",
        "{ type: number, nullable: true, enum: [null] }",
        "{ type: boolean, enum: [null, null] }",
        "{ type: string, enum: [null] }",
    ] {
        let paths = format!(
            "  /entries:\n    get:\n      operationId: listEntries\n      parameters:\n        - {{ name: \
             kind, in: query, schema: {schema} }}\n      responses: {{ \"200\": {{ description: OK }} }}\n"
        );
        let error = Document::load(&document(&paths), &[]).expect_err("the enum names no value");
        assert!(
            matches!(&error, LoadError::Unrunnable { op, name, source: ScalarError::Rule { raw, .. } }
                if op == "listEntries" && name == "kind" && raw == "null"),
            "{schema}: {error:?}"
        );
        assert_eq!(
            cause(&error),
            "`null` is all that the `enum` lists, and a flag cannot send it"
        );

        let body = format!("{{ type: object, properties: {{ kind: {schema} }} }}");
        let error = Document::load(&holding(&body, PLAIN, ""), &[])
            .expect_err("the same enum on a body property is refused the same way");
        assert!(
            matches!(&error, LoadError::Unrunnable { op, name, .. }
                if op == "holdsPattern" && name == "kind"),
            "{schema}: {error:?}"
        );

        let list = format!("{{ type: array, items: {schema} }}");
        let paths = format!(
            "  /entries:\n    get:\n      operationId: listEntries\n      parameters:\n        - {{ name: \
             kinds, in: query, schema: {list} }}\n      responses: {{ \"200\": {{ description: OK }} }}\n"
        );
        let error = Document::load(&document(&paths), &[]).expect_err("so are the items of a list");
        assert!(
            matches!(&error, LoadError::Unrunnable { name, .. } if name == "kinds"),
            "{schema}: {error:?}"
        );
    }
}

/// `null` beside a value a flag can send is the way OpenAPI 3.0 says that a value
/// may be absent, and the values the `enum` lists are the rule.
#[test]
fn an_enum_of_null_beside_a_value_is_the_rule_of_that_value() {
    let doc = read(&[
        "{ name: kind, in: query, schema: { type: integer, nullable: true, enum: [null, 7] } }",
    ]);
    let op = doc.get("listEntries").expect("the operation");
    Invocation::new(op, Values::new().param("kind", "7")).expect("7 is listed");
    let refused = refusal(&doc, Values::new().param("kind", "8"));
    assert_eq!(cause(&refused), "`8` is not one of 7");
}

/// A document with one operation, `holdsPattern`, whose request body is `body`
/// and whose success response is `answer`, beside whatever `components` holds.
fn holding(body: &str, answer: &str, components: &str) -> String {
    format!(
        "openapi: 3.0.3\n\
         info: {{ title: t, version: \"1\" }}\n\
         servers: [{{ url: 'http://localhost:9999' }}]\n\
         paths:\n  /people:\n    post:\n      operationId: holdsPattern\n      requestBody:\n        \
         required: true\n        content:\n          application/json:\n            schema: {body}\n      \
         responses:\n        \"200\":\n          description: OK\n          content:\n            \
         application/json:\n              schema: {answer}\n{components}"
    )
}

/// The `pattern` of a name that a vendor's server runs with the Unicode flag.
const NAME: &str = "{ type: string, pattern: '^\\p{L}+$' }";

/// A schema with nothing to refuse.
const PLAIN: &str = "{ type: object, properties: { id: { type: integer } } }";

/// A document, the `operationId` that holds the schema with the pattern in it
/// (`None` where no operation does), and where the schema is as an Overlay names a
/// target.
type Site = (String, Option<&'static str>, String);

/// The places a generated type is made of that no flag is: a response, a nested
/// property, the items of an array, a flat property that comes after a nested one
/// (the body then goes whole through `--json-body`), and a named schema.
fn sites() -> Vec<Site> {
    let at = |place: &str| format!("$.paths['/people'].post.{place}");
    let body = |place: &str| {
        at(&format!(
            "requestBody.content['application/json'].schema{place}"
        ))
    };
    let answer = |place: &str| {
        at(&format!(
            "responses['200'].content['application/json'].schema{place}"
        ))
    };
    let nested = format!(
        "{{ type: object, properties: {{ address: {{ type: object, properties: {{ city: {NAME} }} }} }} }}"
    );
    let items = format!("{{ type: array, items: {NAME} }}");
    let after_a_nested = format!(
        "{{ type: object, properties: {{ address: {{ type: object, properties: {{ line: {{ type: string }} }} }}, surname: {NAME} }} }}"
    );
    let flat = format!("{{ type: object, properties: {{ surname: {NAME} }} }}");
    let named = format!("components:\n  schemas:\n    Surname: {NAME}\n");
    let op = Some("holdsPattern");
    vec![
        (holding(PLAIN, &flat, ""), op, answer(".properties.surname")),
        (
            holding(&nested, PLAIN, ""),
            op,
            body(".properties.address.properties.city"),
        ),
        (holding(&items, PLAIN, ""), op, body(".items")),
        (
            holding(&after_a_nested, PLAIN, ""),
            op,
            body(".properties.surname"),
        ),
        (
            holding(PLAIN, PLAIN, &named),
            None,
            "$.components.schemas.Surname".to_owned(),
        ),
    ]
}

/// `F-44` is about the generated type, which is made of more schemas than make a
/// flag. The pattern is refused in each, naming the operation where one holds the
/// schema, and the schema as an Overlay names a target.
#[test]
fn a_unicode_pattern_is_refused_in_every_schema_a_type_is_made_of() {
    for (document, op, place) in sites() {
        let error = Document::load(&document, &[]).expect_err("the pattern is refused");
        assert!(
            matches!(&error, LoadError::UnrunnablePattern { op: held, at: found, source: ScalarError::Rule { raw, .. } }
                if held.as_deref() == op && *found == place && raw == r"^\p{L}+$"),
            "{place}: {error:?}"
        );
        let head = op.map_or_else(String::new, |op| format!("{op}: "));
        assert_eq!(
            error.to_string(),
            format!("{head}`{place}` states a rule that cannot run")
        );
        assert_eq!(
            cause(&error),
            "`^\\p{L}+$` holds `\\p{`, which means one thing with the Unicode flag and another \
             without it, and a generated type cannot be given the flag"
        );
    }
}

/// A flat property is a flag, so it keeps the refusal that names the property,
/// whatever the response and the other properties hold.
#[test]
fn a_unicode_pattern_that_becomes_a_flag_is_refused_with_its_property() {
    let flat = format!("{{ type: object, properties: {{ surname: {NAME} }} }}");
    let error = Document::load(&holding(&flat, PLAIN, ""), &[]).expect_err("refused");
    assert!(
        matches!(&error, LoadError::Unrunnable { op, name, .. }
            if op == "holdsPattern" && name == "surname"),
        "{error:?}"
    );
}

/// A pattern that means the same with and without the flag is carried wherever a
/// schema states it.
#[test]
fn a_pattern_without_a_flagged_escape_loads_in_every_schema() {
    let ascii = "{ type: string, pattern: '^[A-Za-z]+$' }";
    let body = format!("{{ type: array, items: {ascii} }}");
    let answer = format!(
        "{{ type: object, properties: {{ address: {{ type: object, properties: {{ city: {ascii} }} }} }} }}"
    );
    let components = format!("components:\n  schemas:\n    Surname: {ascii}\n");
    Document::load(&holding(&body, &answer, &components), &[]).expect("the document loads");
}

/// What the refusal must not swallow: an escaped backslash before `p{`, a brace
/// that no escape owns, and the escapes that mean the same with and without the
/// flag.
#[test]
fn a_pattern_that_means_the_same_with_and_without_the_unicode_flag_is_carried() {
    for pattern in [
        r"^\\p{3}$",
        r"^p\{L\}$",
        r"^\\u{2}$",
        r"^A$",
        r"^\x41$",
        r"^\d+$",
        r"^[a-z]+\.\w*$",
        r"^p{L}$",
        r"^\\$",
    ] {
        read(&[&format!(
            "{{ name: surname, in: query, schema: {{ type: string, pattern: '{pattern}' }} }}"
        )]);
    }
}

/// `A7/docs/rules.yaml` is the input. `--status 7` against `enum: [100, 200]` is
/// sent when the enum is no rule, and `--help` lists no value. The enum is read off
/// the numeric schema, so it is enforced on the command line and on the typed
/// call by the one check, and `--help` shows the rule that refuses.
#[test]
fn a_numeric_enum_on_a_flag_is_enforced_and_listed() {
    let doc = read(&[
        "{ name: status, in: query, schema: { type: integer, enum: [100, 200] } }",
        "{ name: ratio, in: query, schema: { type: number, enum: [0.5, 1.5] } }",
    ]);
    let page = help(&doc, &["entries", "list"]);
    assert!(page.contains("(one of 100, 200)"), "{page}");
    assert!(page.contains("(one of 0.5, 1.5)"), "{page}");

    let refused = line(&doc, &["entries", "list", "--status", "7"])
        .expect_err("`7` is not one of the values");
    assert_eq!(refused.kind(), ErrorKind::ValueValidation);
    let words = refused.to_string();
    assert!(words.contains("'7' for '--status <INT>'"), "{words}");
    assert!(words.contains("`7` is not one of 100, 200"), "{words}");

    // The value is the admitted one, however it is spelled, and it is sent as the
    // value.
    assert_eq!(
        query(
            &doc,
            &["entries", "list", "--status", "+200", "--ratio", "5e-1"]
        )
        .as_deref(),
        Some("status=200&ratio=0.5")
    );

    // The typed call reaches the same check.
    let error = refusal(&doc, Values::new().param("status", 7));
    assert!(
        matches!(&error, ValueError::BadValue { name, source: ScalarError::Choice { raw, allowed }, .. }
            if name == "status" && raw == "7" && allowed == &["100", "200"]),
        "{error:?}"
    );
    assert_eq!(cause(&error), "`7` is not one of 100, 200");
    let error = refusal(&doc, Values::new().param("ratio", 1));
    assert_eq!(cause(&error), "`1` is not one of 0.5, 1.5");
}

/// `A7/docs/boolenum.yaml`: `isBooked` is `type: boolean, enum: [true]`.
#[test]
fn a_boolean_enum_on_a_flag_is_enforced_and_listed() {
    let doc = read(&[
        "{ name: isBooked, in: query, schema: { type: boolean, enum: [true] } }",
        "{ name: isOpen, in: query, schema: { type: boolean } }",
    ]);
    let page = help(&doc, &["entries", "list"]);
    assert!(page.contains("(one of true)"), "{page}");

    let refused = line(&doc, &["entries", "list", "--is-booked", "false"])
        .expect_err("`false` is not the value the enum lists");
    assert_eq!(refused.kind(), ErrorKind::ValueValidation);
    assert!(
        refused.to_string().contains("`false` is not one of true"),
        "{refused}"
    );
    assert_eq!(
        query(
            &doc,
            &[
                "entries",
                "list",
                "--is-booked",
                "true",
                "--is-open",
                "false"
            ]
        )
        .as_deref(),
        Some("isBooked=true&isOpen=false")
    );

    let error = refusal(&doc, Values::new().param("isBooked", false));
    assert!(
        matches!(&error, ValueError::BadValue { name, source: ScalarError::Choice { .. }, .. }
            if name == "isBooked"),
        "{error:?}"
    );
    // A boolean with no enum, or with an enum of both values, carries no note.
    let doc =
        read(&["{ name: isOpen, in: query, schema: { type: boolean, enum: [true, false] } }"]);
    assert!(!help(&doc, &["entries", "list"]).contains("one of"));
    assert!(
        Invocation::new(
            doc.get("listEntries").expect("the operation"),
            Values::new().param("isOpen", false)
        )
        .is_ok()
    );
}

/// The list parameters of the tests below: `ids` is `A7/docs/rules.yaml`'s with a
/// `minItems`, and `pair` takes at least two.
fn lists() -> Document {
    read(&[
        "{ name: ids, in: query, schema: { type: array, minItems: 1, maxItems: 2, uniqueItems: true, items: { type: integer } } }",
        "{ name: pair, in: query, schema: { type: array, minItems: 2, items: { type: string } } }",
    ])
}

/// `A7/docs/rules.yaml` is the input: `ids` is `maxItems: 2, uniqueItems: true`,
/// and `--ids 1 --ids 1 --ids 3` is sent when the list has no rule. A list
/// parameter's count and uniqueness are one value with one check, run where every
/// value of the request is run, so the command line and the typed call refuse the
/// same list, and `--help` says the rule that refuses.
#[test]
fn a_list_flag_is_held_to_its_count_and_to_its_uniqueness() {
    let doc = lists();
    let page = help(&doc, &["entries", "list"]);
    assert!(
        page.contains("at least 1 value; at most 2 values; different values"),
        "{page}"
    );
    assert!(page.contains("at least 2 values"), "{page}");

    for (given, words) in [
        (
            &["1", "2", "3"][..],
            "`1, 2, 3` is not a list of at most 2 values",
        ),
        (&["1", "1"], "`1, 1` is not a list of different values"),
        (&["1", "+1"], "`1, +1` is not a list of different values"),
    ] {
        let mut args = vec!["entries", "list"];
        args.extend(repeated("--ids", given));
        let (error, reason) = stopped(&doc, &args);
        assert!(
            matches!(&error, ValueError::BadList { op, name, source: ScalarError::Rule { .. } }
                if op == "listEntries" && name == "ids"),
            "{given:?}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            "listEntries: `ids` was given a list the document refuses"
        );
        assert_eq!(reason, words, "{given:?}");
    }
    let (_, reason) = stopped(&doc, &["entries", "list", "--pair", "a"]);
    assert_eq!(reason, "`a` is not a list of at least 2 values");
}

/// The lists that the rules above admit, and the one nobody gave.
#[test]
fn a_list_that_meets_its_rules_is_sent_and_a_list_not_given_is_not_a_short_one() {
    let doc = lists();
    for given in [&["1"][..], &["1", "2"]] {
        let mut args = vec!["entries", "list"];
        args.extend(repeated("--ids", given));
        assert!(planned(&doc, &args).is_ok(), "{given:?}");
    }
    assert!(planned(&doc, &["entries", "list"]).is_ok());
    assert_eq!(
        query(&doc, &["entries", "list", "--ids", "1", "--ids", "2"]).as_deref(),
        Some("ids=1&ids=2")
    );
}

/// The typed call hands the values to the same check as the command line.
#[test]
fn the_typed_call_is_held_to_the_same_list_rules() {
    let doc = lists();
    let error = refusal(&doc, Values::new().each("ids", [1, 1, 3]));
    assert!(
        matches!(&error, ValueError::BadList { name, .. } if name == "ids"),
        "{error:?}"
    );
    assert_eq!(cause(&error), "`1, 1, 3` is not a list of at most 2 values");
    assert!(
        Invocation::new(
            doc.get("listEntries").expect("the operation"),
            Values::new().each("ids", [1, 2])
        )
        .is_ok()
    );
}

/// A value of a list that its own rule refuses is refused as that value, before
/// the list is asked.
#[test]
fn a_list_of_values_the_rule_refuses_is_refused_for_the_value() {
    let doc = read(&[
        "{ name: ids, in: query, schema: { type: array, maxItems: 1, items: { type: integer, enum: [1, 2] } } }",
    ]);
    let error = refusal(&doc, Values::new().each("ids", [1, 7]));
    assert!(
        matches!(&error, ValueError::BadValue { name, source: ScalarError::Choice { raw, .. }, .. }
            if name == "ids" && raw == "7"),
        "{error:?}"
    );
}

/// A property of a flat body is a value like a parameter. Its flag runs the
/// enum, and its help line shows it.
#[test]
fn a_numeric_enum_on_a_body_property_is_enforced_by_its_flag() {
    let paths = "  /entries:\n    post:\n      operationId: createEntry\n      requestBody:\n        \
         required: true\n        content:\n          application/json:\n            schema:\n              \
         type: object\n              properties:\n                rate: { type: integer, enum: [0, 7, 19] }\n                \
         fixed: { type: boolean, enum: [false] }\n      responses: { \"201\": { description: Created } }\n";
    let doc = Document::load(&document(paths), &[]).expect("the document loads");
    let page = help(&doc, &["entries", "create"]);
    assert!(page.contains("--rate <INT> one of 0, 7, 19"), "{page}");
    assert!(page.contains("--fixed <BOOL> one of false"), "{page}");

    let refused = line(&doc, &["entries", "create", "--rate", "5"]).expect_err("`5` is no rate");
    assert_eq!(refused.kind(), ErrorKind::ValueValidation);
    assert!(
        refused.to_string().contains("`5` is not one of 0, 7, 19"),
        "{refused}"
    );
    let refused = line(&doc, &["entries", "create", "--fixed", "true"]).expect_err("`true`");
    assert!(
        refused.to_string().contains("`true` is not one of false"),
        "{refused}"
    );
    assert!(
        line(
            &doc,
            &["entries", "create", "--rate", "19", "--fixed", "false"]
        )
        .is_ok()
    );
}

/// A skeleton of a body shows the first value the enumeration lists, which a
/// number zero is not: the template is a body the document admits as far as it
/// goes.
#[test]
fn a_template_shows_the_first_value_an_enum_lists() {
    let paths = "  /entries:\n    post:\n      operationId: createEntry\n      requestBody:\n        \
         required: true\n        content:\n          application/json:\n            schema:\n              \
         type: object\n              required: [rate, ratio, booked, label, address]\n              properties:\n                \
         rate: { type: integer, enum: [7, 19] }\n                ratio: { type: number, enum: [1.5, 2.5] }\n                \
         booked: { type: boolean, enum: [true] }\n                label: { type: string }\n                \
         address: { type: object, properties: { street: { type: string } } }\n      \
         responses: { \"201\": { description: Created } }\n";
    let doc = Document::load(&document(paths), &[]).expect("the document loads");
    let matches = line(&doc, &["entries", "create", "--json-body-template"]).expect("clap");
    let Asked::Template(template) = tree::select(&doc, &matches).expect("a template") else {
        panic!("the template is asked for");
    };
    let template: serde_json::Value = serde_json::from_str(template).expect("JSON");
    assert_eq!(
        template,
        serde_json::json!({
            "rate": 7,
            "ratio": 1.5,
            "booked": true,
            "label": "",
            "address": {},
        })
    );
}
