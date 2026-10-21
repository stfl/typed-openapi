//! What a model blob opens with, and what reading one checks again.
//!
//! A blob that another build wrote is refused by name before a byte of it is
//! decoded, and a blob that decodes is held to the rules a load keeps, because
//! a binary starts from the blob and nothing else has read the document.
//!
//! The blobs here are made two ways. A damaged one is a valid blob cut short or
//! given a header by hand, so these tests also pin the header's format, which
//! never changes. An invalid reduction is a valid one edited as JSON and
//! written again: `Document` serialises to a value, deserialises from one, and
//! checks nothing of its own in between, so the edit reaches `to_blob` and the
//! check under test is the one `from_blob` makes. The fixtures are this crate's
//! own (`tests/fixtures/`), as in every other test file.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that cannot build or edit its fixture should fail loudly and name it"
)]

use serde_json::{Value, json};
use typed_openapi::{Document, DocumentError, Loading, Operation, Shape, Unsupported};

const TOY: &str = include_str!("fixtures/toy.yaml");
const CORRECTIONS: &str = include_str!("fixtures/corrections.yaml");
const CLI: &str = include_str!("fixtures/cli.yaml");

/// The model blob of the toy as typed-openapi 0.0.2 wrote it, which had no
/// header: it starts with the length of the server's URL.
const V0_0_2: &[u8] = include_bytes!("fixtures/model-0.0.2.postcard");

/// The eight bytes every stamped blob opens with. A test of the format writes
/// them out, because a change to the constant would otherwise change the test
/// with it.
const MAGIC: &[u8; 8] = b"\x89TOAMDL\n";

fn document() -> Document {
    Document::load(TOY, &[CORRECTIONS, CLI]).expect("the vendor's document plus the Overlays")
}

/// A header as the format fixes it: the magic, the layout as four little-endian
/// bytes, and the version as four little-endian bytes of length and its text.
fn header(layout: u32, version: &str) -> Vec<u8> {
    let length = u32::try_from(version.len()).expect("a version is short");
    [
        MAGIC.as_slice(),
        &layout.to_le_bytes(),
        &length.to_le_bytes(),
        version.as_bytes(),
    ]
    .concat()
}

/// `body` under a header of this build's layout.
fn stamped(body: &[u8]) -> Vec<u8> {
    [
        header(Document::MODEL_LAYOUT, env!("CARGO_PKG_VERSION")).as_slice(),
        body,
    ]
    .concat()
}

fn refusal(blob: &[u8]) -> DocumentError {
    Document::from_blob(blob).expect_err("the blob is refused")
}

#[test]
fn a_blob_opens_with_its_stamp_and_then_the_encoding() {
    let blob = document().to_blob().expect("the reduction encodes");
    let version = env!("CARGO_PKG_VERSION");
    let body = blob
        .strip_prefix(header(Document::MODEL_LAYOUT, version).as_slice())
        .expect("magic, layout number, length of the version, version");
    assert!(!body.is_empty(), "the encoding follows the stamp");
}

#[test]
fn a_blob_reads_back_as_the_reduction_it_was_written_from() {
    let doc = document();
    let blob = doc.to_blob().expect("the reduction encodes");
    assert_eq!(Document::from_blob(&blob).expect("and decodes"), doc);
}

/// A blob with no stamp is named as one, whatever else the bytes are: an empty
/// file, the blob of 0.0.2, text, and a header that stops early.
#[test]
fn a_blob_with_no_stamp_says_so_and_names_the_layout_this_build_reads() {
    let good = document().to_blob().expect("the reduction encodes");
    let version = env!("CARGO_PKG_VERSION");
    let mut wrong_magic = good.clone();
    *wrong_magic.first_mut().expect("a blob has bytes") = b'X';
    let cases: [(&str, Vec<u8>); 7] = [
        ("an empty file", Vec::new()),
        ("a blob of 0.0.2", V0_0_2.to_vec()),
        ("text", b"not a reduction".to_vec()),
        (
            "a magic one byte short",
            MAGIC.iter().take(7).copied().collect(),
        ),
        ("a magic and nothing else", MAGIC.to_vec()),
        (
            "a version that runs past the end",
            header(Document::MODEL_LAYOUT, version)
                .into_iter()
                .take(12 + 4 + version.len() - 1)
                .collect(),
        ),
        ("another magic", wrong_magic),
    ];
    for (what, blob) in cases {
        let error = refusal(&blob);
        assert!(
            matches!(error, DocumentError::Unstamped { reads } if reads == Document::MODEL_LAYOUT),
            "{what}: {error:?}"
        );
        let message = error.to_string();
        for part in [
            "no layout stamp",
            "older than 0.1.0",
            &format!("layout {}", Document::MODEL_LAYOUT),
            "bless step again",
        ] {
            assert!(message.contains(part), "{what}: {message:?} lacks {part:?}");
        }
    }
}

/// The stamp is read before the encoding, so a stale blob is named whatever its
/// bytes after the stamp are.
#[test]
fn a_blob_of_another_layout_names_both_layouts_and_the_version_that_wrote_it() {
    let other = Document::MODEL_LAYOUT + 1;
    let good = document().to_blob().expect("the reduction encodes");
    let encoding = good
        .strip_prefix(header(Document::MODEL_LAYOUT, env!("CARGO_PKG_VERSION")).as_slice())
        .expect("the blob opens with its stamp");
    let blobs = [
        (
            "an encoding of this build",
            [header(other, "9.9.9").as_slice(), encoding].concat(),
        ),
        (
            "bytes that are no encoding",
            [header(other, "9.9.9").as_slice(), b"\xff\xff\xff"].concat(),
        ),
    ];
    for (what, blob) in blobs {
        let error = refusal(&blob);
        assert!(
            matches!(
                &error,
                DocumentError::Stale { layout, version, reads }
                    if *layout == other && version == "9.9.9" && *reads == Document::MODEL_LAYOUT
            ),
            "{what}: {error:?}"
        );
        let message = error.to_string();
        for part in [
            &format!("layout {other}"),
            "9.9.9",
            &format!("layout {}", Document::MODEL_LAYOUT),
            "bless step again",
        ] {
            assert!(message.contains(part), "{what}: {message:?} lacks {part:?}");
        }
    }
}

/// The version is only a name for the message. A header whose version is not
/// text still says which layout wrote the blob.
#[test]
fn a_version_that_is_not_text_does_not_hide_the_layout() {
    let other = Document::MODEL_LAYOUT + 1;
    let mut blob = [MAGIC.as_slice(), &other.to_le_bytes(), &2_u32.to_le_bytes()].concat();
    blob.extend_from_slice(b"\xff\xfe");
    let error = refusal(&blob);
    assert!(
        matches!(&error, DocumentError::Stale { layout, .. } if *layout == other),
        "{error:?}"
    );
}

#[test]
fn a_blob_of_this_layout_that_does_not_decode_is_damaged_and_says_to_bless_again() {
    let good = document().to_blob().expect("the reduction encodes");
    let cut: Vec<u8> = good.iter().take(good.len() - 1).copied().collect();
    let cases = [
        ("an empty encoding", stamped(b"")),
        ("a stray byte", stamped(b"\xff")),
        ("an encoding cut short", cut),
    ];
    for (what, blob) in cases {
        let error = refusal(&blob);
        assert!(matches!(error, DocumentError::Blob(_)), "{what}: {error:?}");
        let message = error.to_string();
        assert!(message.contains("damaged"), "{what}: {message:?}");
        assert!(message.contains("bless step again"), "{what}: {message:?}");
        assert!(!message.contains("layout stamp"), "{what}: {message:?}");
    }
}

/// Every length a blob can be cut to: a cut inside the stamp leaves no stamp,
/// and a cut after it leaves an encoding that ends too soon.
#[test]
fn a_blob_cut_at_any_length_is_refused_and_never_panics() {
    let good = document().to_blob().expect("the reduction encodes");
    let stamp = header(Document::MODEL_LAYOUT, env!("CARGO_PKG_VERSION")).len();
    for end in 0..good.len() {
        let error = refusal(&good.iter().take(end).copied().collect::<Vec<u8>>());
        let expected = if end < stamp {
            matches!(error, DocumentError::Unstamped { .. })
        } else {
            matches!(error, DocumentError::Blob(_))
        };
        assert!(expected, "cut at {end} of {}: {error:?}", good.len());
    }
}

/// `document()` as JSON, edited by `edit` and written as a blob. What `edit`
/// breaks is not caught on the way in: `Document` deserialises its flags and
/// confirmation words as plain strings, so the check under test is the one
/// `from_blob` makes.
fn blob_edited(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value = serde_json::to_value(document()).expect("the reduction is a value");
    edit(&mut value);
    serde_json::from_value::<Document>(value)
        .expect("the edit keeps the shape of a reduction")
        .to_blob()
        .expect("the edited reduction encodes")
}

/// The operation `id` of an edited reduction.
fn operation<'v>(value: &'v mut Value, id: &str) -> &'v mut Value {
    value
        .get_mut("ops")
        .and_then(Value::as_array_mut)
        .and_then(|ops| ops.iter_mut().find(|op| op["id"] == id))
        .expect("the fixture describes the operation")
}

/// The parameter `name` of an operation of an edited reduction.
fn parameter<'v>(op: &'v mut Value, name: &str) -> &'v mut Value {
    op.get_mut("params")
        .and_then(Value::as_array_mut)
        .and_then(|params| params.iter_mut().find(|param| param["name"] == name))
        .expect("the operation declares the parameter")
}

#[test]
fn an_edit_that_breaks_no_rule_is_read_back_as_it_was_written() {
    let blob = blob_edited(|value| {
        operation(value, "getVoucher")["summary"] = json!("a sentence");
    });
    let doc = Document::from_blob(&blob).expect("nothing a load keeps is broken");
    let op = doc.get("getVoucher").expect("the operation is there");
    assert_eq!(op.summary(), Some("a sentence"));
}

/// `help` is a flag clap declares itself, `json-body` is one every subcommand
/// declares, and the rest are not words a load writes as a flag.
const REFUSED_WORDS: [&str; 5] = ["help", "json-body", "", "!! not a flag !!", "Commit"];

#[test]
fn a_confirmation_word_a_load_refuses_is_refused_on_the_document() {
    for word in REFUSED_WORDS {
        let error = refusal(&blob_edited(|value| value["commit"] = json!(word)));
        assert!(
            matches!(&error, DocumentError::Confirmation { word: found } if found == word),
            "{word:?}: {error:?}"
        );
        assert!(error.to_string().contains("bless step again"), "{error}");
    }
}

/// The word an operation declares is the one clap sees, so this is the check
/// that stops the start-up panic of F-70.
#[test]
fn a_confirmation_word_a_load_refuses_is_refused_on_the_operation_that_declares_it() {
    for word in REFUSED_WORDS {
        let blob = blob_edited(|value| operation(value, "updateVoucher")["commit"] = json!(word));
        let error = refusal(&blob);
        assert!(
            matches!(
                &error,
                DocumentError::OperationConfirmation { op, word: found }
                    if op == "updateVoucher" && found == word
            ),
            "{word:?}: {error:?}"
        );
        assert!(error.to_string().contains("updateVoucher"), "{error}");
    }
}

/// What `Loading::commit` accepts, a decode accepts.
#[test]
fn a_confirmation_word_a_load_accepts_is_read_back() {
    let chosen = Loading::new()
        .commit("yes")
        .expect("a word a person can type as a flag");
    let reduced =
        Document::load_with(TOY, &[CORRECTIONS, CLI], &chosen).expect("the fixture loads");
    let blob = reduced.to_blob().expect("the reduction encodes");
    let read = Document::from_blob(&blob).expect("and decodes");
    assert_eq!(read, reduced);
    assert_eq!(read.commit(), "yes");
}

/// A flag is a name a person types, so a decoded flag is held to the spelling
/// every flag a load writes has: kebab-cased, `[a-z0-9-]`, and not empty.
const UNSPELLED: [&str; 5] = ["", "!! not a flag !!", "Id", "has space", "double--dash"];

#[test]
fn a_flag_a_load_could_not_spell_is_refused_on_a_parameter() {
    for flag in UNSPELLED {
        let blob = blob_edited(|value| {
            let id = parameter(operation(value, "getVoucher"), "id");
            id["shape"]["Flag"]["flag"] = json!(flag);
        });
        let error = refusal(&blob);
        assert!(
            matches!(
                &error,
                DocumentError::Flag { op, flag: found } if op == "getVoucher" && found == flag
            ),
            "{flag:?}: {error:?}"
        );
        assert!(error.to_string().contains("getVoucher"), "{error}");
    }
}

#[test]
fn a_flag_a_load_could_not_spell_is_refused_on_a_body_field() {
    for flag in UNSPELLED {
        let blob = blob_edited(|value| {
            let fields = &mut operation(value, "createVoucher")["body"]["JsonFields"];
            fields[0]["flag"] = json!(flag);
        });
        let error = refusal(&blob);
        assert!(
            matches!(
                &error,
                DocumentError::Flag { op, flag: found } if op == "createVoucher" && found == flag
            ),
            "{flag:?}: {error:?}"
        );
    }
}

/// `getVoucher` takes `id` in the path; `ghost` is no parameter of it.
#[test]
fn a_requirement_on_a_parameter_the_operation_does_not_declare_is_refused() {
    let blob = blob_edited(|value| {
        let id = parameter(operation(value, "getVoucher"), "id");
        id["requires"] = json!(["ghost"]);
    });
    let error = refusal(&blob);
    assert!(
        matches!(
            &error,
            DocumentError::RequiresUnknown { op, name, requires }
                if op == "getVoucher" && name == "id" && requires == "ghost"
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("getVoucher"), "{error}");
}

/// A parameter with no flag cannot be given, so a requirement on it is a
/// requirement nothing meets.
#[test]
fn a_requirement_on_a_parameter_without_a_flag_is_refused() {
    let blob = blob_edited(|value| {
        let op = operation(value, "listVouchers");
        let params = op["params"]
            .as_array_mut()
            .expect("the operation has parameters");
        let mut cookie = params.first().cloned().expect("and a first one");
        cookie["name"] = json!("session");
        cookie["required"] = json!(false);
        cookie["shape"] = json!({ "Unreachable": "Cookie" });
        cookie["requires"] = json!([]);
        params.push(cookie);
        let first = params.first_mut().expect("and a first one");
        first["requires"] = json!(["session"]);
    });
    let error = refusal(&blob);
    assert!(
        matches!(
            &error,
            DocumentError::RequiresUnreachable { op, requires, why: Unsupported::Cookie, .. }
                if op == "listVouchers" && requires == "session"
        ),
        "{error:?}"
    );
}

/// The check must not refuse what a load writes: every operation of the
/// fixture, through the same edit path with nothing changed.
#[test]
fn nothing_a_load_writes_is_refused() {
    let blob = blob_edited(|_| {});
    let doc = Document::from_blob(&blob).expect("a load's own reduction passes its own checks");
    assert_eq!(doc, document());
    let flags = doc
        .iter()
        .flat_map(Operation::params)
        .filter(|param| matches!(param.shape(), Shape::Flag { .. }))
        .count();
    assert!(flags > 0, "the fixture has parameters with flags to check");
}
