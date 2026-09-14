//! What one invocation produced, and the three ways it comes about.
//!
//! Nothing here prints: [`crate::app::run`] hands an [`Output`] back and
//! `main.rs` is the only place that writes to a stream. That is what lets every
//! test in `tests/` drive the real command with a recording client and assert on
//! the bytes.

use http::Response;
use serde::Serialize;

/// What one invocation produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

impl Output {
    /// A request went out and came back. JSON to stdout, anything else as it
    /// arrived, and the status in the exit code.
    #[must_use]
    pub fn answered(response: &Response<Vec<u8>>) -> Self {
        let body = response.body();
        let text = match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(value) => serde_json::to_string_pretty(&value)
                .unwrap_or_else(|_| String::from_utf8_lossy(body).into_owned()),
            Err(_) => String::from_utf8_lossy(body).into_owned(),
        };
        if response.status().is_success() {
            Self {
                stdout: format!("{text}\n"),
                stderr: String::new(),
                success: true,
            }
        } else {
            Self {
                stdout: String::new(),
                stderr: format!("toy: {} {text}\n", response.status()),
                success: false,
            }
        }
    }

    /// Output that stands on its own, with a line on stderr saying what it is —
    /// a dry run, or a chain that found nothing to do.
    #[must_use]
    pub fn note(stdout: String, note: impl Into<String>) -> Self {
        Self {
            stdout,
            stderr: note.into(),
            success: true,
        }
    }

    /// A run that ended on a typed value rather than a raw response.
    #[must_use]
    pub fn value<T: Serialize>(printed: &str, value: &T) -> Self {
        Self {
            stdout: format!("{printed}\n{}\n", json(value)),
            stderr: String::new(),
            success: true,
        }
    }
}

/// Pretty JSON, or an explanation of why there is none.
fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("<unprintable: {e}>"))
}
