//! The bless step for this adoption: the vendor's document plus the adopter's
//! Overlay in, the `api-generated` crate out.
//!
//! ```text
//! cargo run -p xtask -- bless
//! ```
//!
//! This is the whole of an adopter's generator. Everything it calls ships in
//! `typed-openapi` under the `generate` feature, so the four artefacts under
//! `api-generated/` are reproducible from a published crate rather than from
//! this workspace.

use std::path::Path;
use std::process::ExitCode;

use typed_openapi::generate::Settings;

fn main() -> ExitCode {
    if let Some(task) = std::env::args().nth(1).filter(|task| task != "bless") {
        eprintln!("unknown task `{task}`; the only task is `bless`");
        return ExitCode::FAILURE;
    }
    // The adoption this xtask blesses is the directory holding it.
    let adoption = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let blessed = Settings::new(
        adoption.join("spec/toy.yaml"),
        adoption.join("spec/overlay.yaml"),
    )
    .replace("money", "api_types::Money")
    .write_to(adoption.join("api-generated"));

    match blessed {
        Ok(written) => {
            for path in written {
                println!("blessed {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bless: {error}");
            ExitCode::FAILURE
        }
    }
}
