//! Encoding a `multipart/form-data` body (RFC 7578).
//!
//! Pure: parts in, bytes and a `Content-Type` out. The boundary is derived from
//! the content rather than drawn at random, so the same parts always encode to
//! the same bytes — which is what makes a dry run worth reading and a recorded
//! test worth asserting on.

use crate::values::Part;

/// The bytes and the `Content-Type` header value they must be sent under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

const STEM: &str = "----typed-openapi-boundary";

/// Encode `parts` under a boundary that appears in none of them.
#[must_use]
pub fn encode(parts: &[Part]) -> Encoded {
    let boundary = boundary(parts);
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        bytes.extend_from_slice(content_disposition(part.name(), part.filename()).as_bytes());
        if part.filename().is_some() {
            bytes.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
        }
        bytes.extend_from_slice(b"\r\n");
        bytes.extend_from_slice(part.bytes());
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Encoded {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        bytes,
    }
}

/// A header line whose value cannot break out of the header: `"` and the two
/// line-ending bytes are the only characters that could, and RFC 7578 §5.1
/// sanctions percent-encoding them.
fn content_disposition(name: &str, filename: Option<&str>) -> String {
    use std::fmt::Write as _;

    let mut line = format!("Content-Disposition: form-data; name=\"{}\"", quoted(name));
    if let Some(filename) = filename {
        let _ = write!(line, "; filename=\"{}\"", quoted(filename));
    }
    line.push_str("\r\n");
    line
}

fn quoted(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '"' => "%22".to_owned(),
            '\r' => "%0D".to_owned(),
            '\n' => "%0A".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

/// The first `STEM-<n>` that occurs in no part. Deterministic, and it
/// terminates: each candidate that collides rules out at least one occurrence
/// in a finite body.
fn boundary(parts: &[Part]) -> String {
    (0..u32::MAX)
        .map(|n| format!("{STEM}-{n}"))
        .find(|candidate| {
            !parts
                .iter()
                .any(|part| contains(part.bytes(), candidate.as_bytes()))
        })
        .unwrap_or_else(|| STEM.to_owned())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_part_carries_its_filename_and_a_type() {
        let encoded = encode(&[Part::file("file", "doc.bin", b"PDF".to_vec())]);
        let text = String::from_utf8_lossy(&encoded.bytes).into_owned();
        assert!(
            text.contains("Content-Disposition: form-data; name=\"file\"; filename=\"doc.bin\""),
            "{text}"
        );
        assert!(
            text.contains("Content-Type: application/octet-stream"),
            "{text}"
        );
        assert!(text.ends_with("--\r\n"), "{text}");
    }

    #[test]
    fn a_text_part_carries_neither() {
        let encoded = encode(&[Part::text("kind", "invoice")]);
        let text = String::from_utf8_lossy(&encoded.bytes).into_owned();
        assert!(!text.contains("filename"), "{text}");
        assert!(!text.contains("Content-Type:"), "{text}");
        assert!(text.contains("\r\n\r\ninvoice\r\n"), "{text}");
    }

    #[test]
    fn the_boundary_moves_aside_for_content_that_contains_it() {
        let colliding = format!("{STEM}-0").into_bytes();
        let encoded = encode(&[Part::file("file", "f", colliding)]);
        assert!(
            encoded.content_type.ends_with(&format!("{STEM}-1")),
            "{}",
            encoded.content_type
        );
    }

    #[test]
    fn the_same_parts_encode_to_the_same_bytes() {
        let parts = [
            Part::text("a", "1"),
            Part::file("f", "x.bin", vec![0, 1, 2]),
        ];
        assert_eq!(encode(&parts), encode(&parts));
    }

    #[test]
    fn a_quote_in_a_name_cannot_break_out_of_the_header() {
        let encoded = encode(&[Part::file("f", "a\"b\r\nX: y", Vec::new())]);
        let text = String::from_utf8_lossy(&encoded.bytes).into_owned();
        assert!(text.contains("filename=\"a%22b%0D%0AX: y\""), "{text}");
    }
}
