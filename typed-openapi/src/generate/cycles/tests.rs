//! JSON Schema array forms that OpenAPI 3.0 cannot state as tuple items.

use schemars::schema::Schema;
use serde_json::json;

use super::check;
use crate::generate::GenerateError;

// merge_so_array -> merge_items_array and merge_additional_items. Each row
// states both sides separately so equal values cannot erase document identity.
const ARRAYS: [(&str, &str); 5] = [
    (
        r##"{"items":[{"$ref":"#/definitions/A"}]}"##,
        r##"{"items":[{"$ref":"#/definitions/B"}]}"##,
    ),
    (
        r##"{"items":{"$ref":"#/definitions/A"}}"##,
        r##"{"items":[{"$ref":"#/definitions/B"}]}"##,
    ),
    (
        r##"{"items":[],"additionalItems":{"$ref":"#/definitions/A"}}"##,
        r##"{"items":[{"$ref":"#/definitions/B"}]}"##,
    ),
    (
        r##"{"items":[],"additionalItems":{"$ref":"#/definitions/A"}}"##,
        r##"{"items":[],"additionalItems":{"$ref":"#/definitions/B"}}"##,
    ),
    (
        r##"{"maxItems":0,"items":{"$ref":"#/definitions/A"}}"##,
        r##"{"maxItems":0,"items":{"$ref":"#/definitions/B"}}"##,
    ),
];

#[test]
#[expect(clippy::expect_used, reason = "a missing refusal fails the regression")]
fn array_merge_call_sites_refuse_recursive_operand_sets() {
    for (left, right) in ARRAYS {
        let definitions = definitions(left, right);
        let failure = check(&definitions, &[]).expect_err("recursive array merge");
        assert!(
            matches!(&failure, GenerateError::CyclicAllOf { schemas, .. } if schemas == &["A", "B"]),
            "{failure:?}"
        );
    }
}

#[expect(
    clippy::expect_used,
    reason = "literal JSON Schemas of the regression table"
)]
fn definitions(left: &str, right: &str) -> Vec<(String, Schema)> {
    vec![
        (
            "C".to_owned(),
            serde_json::from_value(
                json!({"allOf": [{"$ref":"#/definitions/A"}, {"$ref":"#/definitions/B"}]}),
            )
            .expect("merge"),
        ),
        ("A".to_owned(), serde_json::from_str(left).expect("left")),
        ("B".to_owned(), serde_json::from_str(right).expect("right")),
    ]
}
