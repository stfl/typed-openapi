//! String enum names, checked before typify's two naming passes can panic.
//!
//! The corpus test holds this prediction to typify's actual conversion. Other
//! names are read from its output by `names`.

use std::collections::BTreeMap;

use heck::ToPascalCase;
use schemars::schema::Schema;
use unicode_ident::{is_xid_continue, is_xid_start};

use super::GenerateError;

pub(super) fn check<'a>(
    schemas: impl IntoIterator<Item = (String, &'a Schema)>,
) -> Result<(), GenerateError> {
    for (owner, schema) in schemas {
        let mut pending = vec![schema];
        while let Some(schema) = pending.pop() {
            let Schema::Object(object) = schema else {
                continue;
            };
            if let Some((values, variant)) = object.enum_values.as_ref().and_then(|values| {
                let strings: Vec<_> = values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect();
                collision(&strings)
            }) {
                return Err(GenerateError::EnumVariants {
                    schema: owner,
                    values,
                    variant,
                });
            }
            pending.extend(
                super::cycles::children(schema)
                    .into_iter()
                    .map(|(_, child)| child),
            );
        }
    }
    Ok(())
}

fn collision(values: &[&str]) -> Option<(Vec<String>, String)> {
    variants(values)
        .into_iter()
        .find(|(_, values)| values.len() > 1)
        .map(|(name, values)| (values, name))
}

fn variants(values: &[&str]) -> BTreeMap<String, Vec<String>> {
    let first = variant_names(values, false);
    if first.values().all(|values| values.len() == 1) {
        first
    } else {
        variant_names(values, true)
    }
}

fn variant_names(values: &[&str], escaped: bool) -> BTreeMap<String, Vec<String>> {
    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for value in values {
        let input = if escaped {
            value.replace(|c| c == '_' || !is_xid_continue(c), "X")
        } else {
            (*value).to_owned()
        };
        names
            .entry(sanitize(&input))
            .or_default()
            .push((*value).to_owned());
    }
    names
}

fn sanitize(input: &str) -> String {
    let name = match input {
        "+1" => "plus1".to_owned(),
        "-1" => "minus1".to_owned(),
        _ => input
            .replace('\'', "")
            .replace(|c| !is_xid_continue(c), "-")
            .to_pascal_case(),
    };
    let name = match name.chars().next() {
        None => "X".to_owned(),
        Some(c) if is_xid_start(c) => name,
        Some(_) => format!("X{name}"),
    };
    if syn::parse_str::<syn::Ident>(&name).is_ok() {
        name
    } else {
        format!("{name}_")
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a fixture or typify contract violation must fail the test"
)]
mod tests {
    use super::{collision, variants};
    use crate::generate::guard::caught;

    #[test]
    fn enum_prediction_agrees_with_typify() {
        let corpus = [
            "active", "ACTIVE", "a-b", "a_b", "+", "-", "+1", "-1", "1", "x1", "", "_", "Self",
            "self", "hello's", "hellos", "é", "É", "a💡b", "aXb", "a b", "a.b", "ß", "中",
            "a\u{301}", "💡", "X", "x", "123", "X123",
        ];
        for values in [
            &["hello's", "hellos", "helloXs"][..],
            &["hello's", "hellos", "unique"][..],
            &["active", "ACTIVE", "third"][..],
            &["active", "unique", "different"][..],
        ] {
            agrees_with_typify(values);
        }
        for (at, left) in corpus.iter().enumerate() {
            for right in corpus.iter().skip(at + 1) {
                agrees_with_typify(&[left, right]);
            }
        }
    }
    fn agrees_with_typify(values: &[&str]) {
        let schema = serde_json::from_value(serde_json::json!({"type":"string", "enum":values}))
            .expect("schema");
        let actual = caught(|| {
            let mut space = typify::TypeSpace::default();
            let id = space
                .add_type_with_name(&schema, Some("Choice".to_owned()))
                .expect("enum converts or panics");
            let ty = space.get_type(&id).expect("the inserted type");
            let typify::TypeDetails::Enum(held) = ty.details() else {
                panic!("a string enum")
            };
            let names: std::collections::BTreeSet<_> =
                held.variants().map(|(name, _)| name.to_owned()).collect();
            let _ = space.to_stream();
            names
        });
        assert_eq!(collision(values).is_some(), actual.is_err(), "{values:?}");
        if let Ok(names) = actual {
            assert_eq!(
                variants(values)
                    .into_keys()
                    .collect::<std::collections::BTreeSet<_>>(),
                names,
                "{values:?}"
            );
        }
    }
}
