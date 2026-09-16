//! Two words, two owners: what happens when a document wants one of them.
//!
//! A command line spends some of its flags on carrying a request's data and two
//! of them on a person saying yes — the confirmation, and each gate an
//! operation names. The document owns the first set. This crate and its adopter
//! own the second.
//!
//! When a vendor's schema declares a property called `enshrine` or `commit`,
//! the two sets collide. Moving the document's property aside would be the
//! quiet fix and it is the wrong one: the property name is what goes on the
//! wire, the vendor chose it, and a confirmation that can be answered by a flag
//! somebody typed for an unrelated reason is not a confirmation. So the
//! document is refused, and each word moves on the side that owns it.
//!
//! Run it with:
//!
//! ```console
//! $ cargo run --features document --example clashing-words
//! ```

use typed_openapi::{Document, Loading, tree};

/// One operation that wants both words: a property spelled like the gate it
/// stands behind, and another spelled like the confirmation.
const CLASHING: &str = r#"
openapi: 3.0.3
info: { title: ledger, version: "1" }
servers: [{ url: "https://ledger.example" }]
paths:
  /vouchers/{id}/enshrine:
    post:
      operationId: enshrineVoucher
      parameters:
        - { name: id, in: path, required: true, schema: { type: integer } }
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                enshrine: { type: string }
                commit: { type: string }
                note: { type: string }
      responses: { "200": { description: OK } }
"#;

/// The adopter's correction layer, naming the hazard this operation stands
/// behind. `GATE_MOVED` is the same layer with the gate spelled out of the
/// document's way.
const GATE: &str = r#"
overlay: 1.1.0
info: { title: cli, version: "1" }
actions:
  - target: $.paths['/vouchers/{id}/enshrine'].post
    update:
      x-cli-gates: [enshrine]
"#;

const GATE_MOVED: &str = r#"
overlay: 1.1.0
info: { title: cli, version: "1" }
actions:
  - target: $.paths['/vouchers/{id}/enshrine'].post
    update:
      x-cli-gates: [gate-enshrine]
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    heading("1. The gate and a property both want `--enshrine`");
    match Document::load(CLASHING, &[GATE]) {
        Ok(_) => println!("loaded, which it should not have"),
        Err(refusal) => println!("{refusal}"),
    }

    heading("2. The gate has moved; the confirmation still wants `--commit`");
    match Document::load(CLASHING, &[GATE_MOVED]) {
        Ok(_) => println!("loaded, which it should not have"),
        Err(refusal) => println!("{refusal}"),
    }

    heading("3. Both words moved, each on the side that owns it");
    let doc = Document::load_with(CLASHING, &[GATE_MOVED], &Loading::new().commit("yes")?)?;
    let op = doc
        .get("enshrineVoucher")
        .ok_or("the document describes it")?;

    println!("the confirmation is `--{}`", op.commit());
    for gate in op.gates() {
        println!("the gate is `--{gate}`");
    }
    println!();
    println!("and the document's own names kept their flags:");
    for arg in tree::command(op).get_arguments() {
        if let Some(long) = arg.get_long() {
            println!("  --{long}");
        }
    }

    Ok(())
}

fn heading(title: &str) {
    println!("\n=== {title} ===");
}
