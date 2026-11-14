//! The generated named calls and the real raw command meet the same encoder.
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "fixtures fail loudly"
)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use api::{Api, OperationId, SubmitEmptyFormBody, SubmitFormBody};
use cli::app;
use http::StatusCode;
use serde_json::{Value, json};
use typed_openapi::tree::{self, Asked};
use typed_openapi::{Answers, HttpRequest, Part, Plan, Recorder, render};

const GOLDEN: &[u8] =
    b"choice=1&digits=42&kind=Toy&ref%5Bid%5D=A%2BB+%25%26%3D%C3%A4&text=+%7E*._-%0A";
const EMPTY_GOLDEN: &[u8] = b"a%2B%25%26%3D+%C3%A4%5B%5D=";
static FILE: AtomicUsize = AtomicUsize::new(0);

fn valid() -> Value {
    json!({"choice":"1", "digits":"42", "kind":"Toy", "ref[id]":"A+B %&=ä", "text":" ~*._-\n"})
}

fn file(bytes: &[u8]) -> PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "structured-form-{}-{}",
        std::process::id(),
        FILE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, bytes).expect("input file");
    path
}

fn line(api: &Api, id: OperationId, args: &[String]) -> Vec<String> {
    let op = api.operation(id);
    ["toy", "raw", op.group().as_str(), op.command().as_str()]
        .into_iter()
        .map(str::to_owned)
        .chain(args.iter().cloned())
        .collect()
}

fn raw_plan(api: &Api, id: OperationId, body: &Value, commit: bool) -> Plan {
    let path = file(body.to_string().as_bytes());
    let mut args = vec![
        "--json-body".to_owned(),
        path.to_string_lossy().into_owned(),
    ];
    if id == OperationId::SubmitForm {
        args.push("--toy-form".to_owned());
    }
    if commit {
        args.push("--commit".to_owned());
    }
    let matches = app::root(api)
        .try_get_matches_from(line(api, id, &args))
        .expect("raw flags");
    let (_, raw) = matches.subcommand().expect("raw subcommand");
    let Asked::Run(selected) = tree::select(api.document(), raw).expect("selection") else {
        panic!("run")
    };
    selected.plan(api.base()).expect("plan")
}

fn equal(a: &HttpRequest, b: &HttpRequest) {
    assert_eq!(a.method(), b.method());
    assert_eq!(a.uri(), b.uri());
    assert_eq!(a.headers(), b.headers());
    assert_eq!(a.body(), b.body());
}

fn run_raw(
    id: OperationId,
    args: &[String],
    recorder: &Recorder,
) -> Result<cli::output::Output, String> {
    let api = Api::new().expect("api");
    let matches = app::root(&api)
        .try_get_matches_from(line(&api, id, args))
        .map_err(|e| e.to_string())?;
    app::run(api, recorder, &matches).map_err(|e| e.to_string())
}

fn invalid_bodies() -> Vec<Value> {
    let mut bodies = vec![json!(null), json!([]), json!("x")];
    for key in ["choice", "digits", "kind", "ref[id]", "text"] {
        let mut body = valid();
        body.as_object_mut().expect("object").remove(key);
        bodies.push(body);
        for wrong in [json!(null), json!(false), json!(5), json!({}), json!([])] {
            let mut body = valid();
            body[key] = wrong;
            bodies.push(body);
        }
    }
    for (key, value) in [
        ("extra", "x"),
        ("choice", "2"),
        ("digits", "x2"),
        ("digits", "1"),
        ("digits", "1234"),
        ("kind", "toy"),
    ] {
        let mut body = valid();
        body[key] = json!(value);
        bodies.push(body);
    }
    bodies
}

#[test]
fn structured_form_raw_typed_validation() {
    let api = Api::new().expect("api");
    for body in invalid_bodies() {
        let recorder = Recorder::new().strict();
        let path = file(body.to_string().as_bytes());
        let args = vec![
            "--json-body".to_owned(),
            path.to_string_lossy().into_owned(),
            "--toy-form".to_owned(),
            "--commit".to_owned(),
        ];
        assert!(
            run_raw(OperationId::SubmitForm, &args, &recorder).is_err(),
            "raw: {body}"
        );
        assert!(recorder.take().is_empty());
        assert!(
            OperationId::SubmitForm.check_body(&body).is_err(),
            "generated validator: {body}"
        );
        match serde_json::from_value::<SubmitFormBody>(body.clone()) {
            Err(_) => {}
            Ok(typed) => assert!(api.submit_form(&typed).is_err(), "typed call: {body}"),
        }
    }
    for args in [
        vec!["--raw-body", "ignored"],
        vec!["--choice", "1"],
        vec!["--field", "choice=1"],
    ] {
        let args: Vec<_> = args.into_iter().map(str::to_owned).collect();
        let recorder = Recorder::new().strict();
        assert!(run_raw(OperationId::SubmitForm, &args, &recorder).is_err());
        assert!(recorder.take().is_empty());
    }
    let typed: SubmitFormBody = serde_json::from_value(valid()).expect("named request");
    OperationId::SubmitForm
        .check_body(&valid())
        .expect("generated validator");
    assert_eq!(
        api.submit_form(&typed)
            .expect("call")
            .request()
            .expect("request")
            .body(),
        GOLDEN
    );
}

#[test]
fn structured_form_wire_bytes() {
    let api = Api::new().expect("api");
    let mut value = valid();
    let reversed: serde_json::Map<_, _> = value
        .as_object()
        .expect("object")
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let raw = raw_plan(
        &api,
        OperationId::SubmitForm,
        &Value::Object(reversed),
        false,
    );
    let typed: SubmitFormBody = serde_json::from_value(value.clone()).expect("typed");
    let request = api
        .submit_form(&typed)
        .expect("call")
        .request()
        .expect("request");
    equal(raw.request(), &request);
    assert_eq!(request.body(), GOLDEN);
    assert_eq!(
        request.headers()["content-type"],
        "application/x-www-form-urlencoded"
    );
    check_schema_order(&request, &value);
    let empty = json!({"a+%&= ä[]":""});
    let typed: SubmitEmptyFormBody =
        serde_json::from_value(empty.clone()).expect("empty named form");
    let request = api
        .submit_empty_form(&typed)
        .expect("call")
        .request()
        .expect("request");
    equal(
        &request,
        raw_plan(&api, OperationId::SubmitEmptyForm, &empty, false).request(),
    );
    assert_eq!(request.body(), EMPTY_GOLDEN);
    value["text"] = json!("");
    assert_eq!(
        raw_plan(&api, OperationId::SubmitForm, &value, false)
            .request()
            .body(),
        b"choice=1&digits=42&kind=Toy&ref%5Bid%5D=A%2BB+%25%26%3D%C3%A4&text="
    );
}

fn check_schema_order(request: &HttpRequest, value: &Value) {
    // The committed schema lists its fields in reverse key order. A source
    // load with the opposite property order must produce the same wire bytes.
    let mut source = typed_openapi::overlay::parse(api::DOCUMENT).expect("YAML");
    let properties = source["components"]["schemas"]["ToyForm"]["properties"]
        .as_object()
        .expect("properties");
    let reversed = properties
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    source["components"]["schemas"]["ToyForm"]["properties"] = Value::Object(reversed);
    let document =
        typed_openapi::Document::load(&source.to_string(), &[]).expect("reordered source");
    let reordered = typed_openapi::Invocation::new(
        document.get("submitForm").expect("operation"),
        typed_openapi::Values::new().json(value.clone()),
    )
    .expect("valid")
    .request(document.base())
    .expect("request");
    equal(request, &reordered);
}

#[test]
fn structured_form_plan_send_identity() {
    let api = Api::new().expect("api");
    let typed: SubmitFormBody = serde_json::from_value(valid()).expect("typed");
    let call = api.submit_form(&typed).expect("call");
    let recorder = Recorder::new().strict();
    let dry = call.plan(&Answers::new().gate("toy-form")).expect("plan");
    assert!(matches!(dry.plan(), Plan::DryRun(_)));
    let printed = render(dry.request());
    equal(
        dry.request(),
        raw_plan(&api, OperationId::SubmitForm, &valid(), false).request(),
    );
    assert!(dry.send(&recorder).expect("dry run").is_none());
    assert!(recorder.take().is_empty());
    for answers in [Answers::new(), Answers::new().commit()] {
        let held = call.plan(&answers).expect("missing gate holds the call");
        assert!(matches!(held.plan(), Plan::DryRun(_)));
        assert!(held.send(&recorder).expect("held").is_none());
        assert!(recorder.take().is_empty());
    }
    let sent = call
        .plan(&Answers::new().commit().gate("toy-form"))
        .expect("plan");
    assert!(matches!(sent.plan(), Plan::Send(_)));
    let approved = sent.request().clone();
    assert_eq!(printed, render(&approved));
    equal(
        &approved,
        raw_plan(&api, OperationId::SubmitForm, &valid(), true).request(),
    );
    let recorder = Recorder::new()
        .answering_with(
            http::Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(Vec::new())
                .expect("response"),
        )
        .strict();
    assert!(sent.send(&recorder).expect("send").is_some());
    let sent_requests = recorder.take();
    assert_eq!(sent_requests.len(), 1);
    equal(&approved, &sent_requests[0]);
    check_raw_plan_send(&approved, &printed);
}

fn check_raw_plan_send(approved: &HttpRequest, printed: &str) {
    let recorder = Recorder::new().strict();
    let path = file(valid().to_string().as_bytes());
    for commit in [false, true] {
        let mut args = vec![
            "--json-body".to_owned(),
            path.to_string_lossy().into_owned(),
        ];
        if commit {
            args.push("--commit".to_owned());
        }
        assert!(
            run_raw(OperationId::SubmitForm, &args, &recorder).is_err(),
            "raw requires the gate for dry run and send"
        );
        assert!(recorder.take().is_empty());
    }
    let args = vec![
        "--json-body".to_owned(),
        path.to_string_lossy().into_owned(),
        "--toy-form".to_owned(),
    ];
    assert_eq!(
        run_raw(OperationId::SubmitForm, &args, &recorder)
            .expect("raw dry run")
            .stdout,
        printed
    );
    assert!(recorder.take().is_empty());
    let mut args = args;
    args.push("--commit".to_owned());
    let recorder = Recorder::new();
    run_raw(OperationId::SubmitForm, &args, &recorder).expect("raw send");
    let sent_requests = recorder.take();
    assert_eq!(sent_requests.len(), 1);
    equal(approved, &sent_requests[0]);
}

#[test]
fn structured_form_generated_named_call() {
    let api = Api::new().expect("api");
    let typed: SubmitFormBody = serde_json::from_value(valid()).expect("named type");
    let request = api
        .submit_form(&typed)
        .expect("compiled named call")
        .request()
        .expect("request");
    assert_eq!(request.body(), GOLDEN);
    #[cfg(feature = "builder")]
    {
        let built = api
            .submit_form_builder()
            .body(&typed)
            .call()
            .expect("builder")
            .request()
            .expect("request");
        equal(&built, &request);
    }
    let op = api.operation(OperationId::SubmitForm);
    let mut root = app::root(&api);
    let help = root
        .find_subcommand_mut("raw")
        .expect("raw")
        .find_subcommand_mut(op.group().as_str())
        .expect("group")
        .find_subcommand_mut(op.command().as_str())
        .expect("command")
        .render_long_help()
        .to_string();
    assert!(
        help.contains("JSON object")
            && help.contains("application/x-www-form-urlencoded wire bytes"),
        "{help}"
    );
    assert!(!help.contains("--raw-body") && !help.contains("--choice"));
}

#[test]
fn structured_form_legacy_compatibility() {
    let api = Api::new().expect("api");
    let bytes = b"%ff+&x=\0\xff\n";
    let typed = api
        .submit_opaque_form(bytes.to_vec())
        .expect("Vec<u8> interface")
        .request()
        .expect("request");
    assert_eq!(typed.body(), bytes);
    assert_eq!(
        typed.headers()["content-type"],
        "application/x-www-form-urlencoded"
    );
    let path = file(bytes);
    let recorder = Recorder::new();
    run_raw(
        OperationId::SubmitOpaqueForm,
        &[
            "--raw-body".to_owned(),
            path.to_string_lossy().into_owned(),
            "--commit".to_owned(),
        ],
        &recorder,
    )
    .expect("opaque raw");
    let sent = recorder.take();
    assert_eq!(sent.len(), 1);
    equal(&typed, &sent[0]);
    let json = file(br#"{"total":"12.50","currency":"EUR","status":"draft"}"#);
    for args in [
        vec![
            "--total".to_owned(),
            "12.50".to_owned(),
            "--currency".to_owned(),
            "EUR".to_owned(),
            "--status".to_owned(),
            "draft".to_owned(),
        ],
        vec![
            "--json-body".to_owned(),
            json.to_string_lossy().into_owned(),
        ],
    ] {
        let mut args = args;
        args.extend(["--id".to_owned(), "5".to_owned(), "--commit".to_owned()]);
        let recorder = Recorder::new();
        run_raw(OperationId::UpdateVoucher, &args, &recorder).expect("legacy JSON flags");
        let sent = recorder.take();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].headers()["content-type"], "application/json");
        assert_eq!(
            sent[0].body(),
            br#"{"total":"12.50","currency":"EUR","status":"draft"}"#
        );
    }
    legacy_json_and_multipart(&api);
}

fn legacy_json_and_multipart(api: &Api) {
    let request = api
        .upload_document(vec![0, 255, 10])
        .expect("opaque upload")
        .request()
        .expect("request");
    assert_eq!(request.body(), &[0, 255, 10]);
    assert_eq!(request.headers()["content-type"], "application/pdf");
    let directory = file(b"");
    std::fs::remove_file(&directory).expect("reserve directory");
    std::fs::create_dir(&directory).expect("create directory");
    let pdf = directory.join("toy.pdf");
    std::fs::write(&pdf, b"PDF").expect("pdf");
    let request = api
        .upload_document_multipart(vec![
            Part::text("kind", "toy"),
            Part::file("file", "toy.pdf", b"PDF".to_vec()),
        ])
        .expect("parts")
        .request()
        .expect("request");
    assert_eq!(
        request.headers()["content-type"],
        "multipart/form-data; boundary=----typed-openapi-boundary-0"
    );
    assert_eq!(request.body(), b"------typed-openapi-boundary-0\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\ntoy\r\n------typed-openapi-boundary-0\r\nContent-Disposition: form-data; name=\"file\"; filename=\"toy.pdf\"\r\nContent-Type: application/pdf\r\n\r\nPDF\r\n------typed-openapi-boundary-0--\r\n");
    let recorder = Recorder::new();
    run_raw(
        OperationId::UploadDocumentMultipart,
        &[
            "--file".to_owned(),
            format!("file={}", pdf.display()),
            "--field".to_owned(),
            "kind=toy".to_owned(),
            "--commit".to_owned(),
        ],
        &recorder,
    )
    .expect("multipart raw");
    let sent = recorder.take();
    assert_eq!(sent.len(), 1);
    equal(&request, &sent[0]);
}
