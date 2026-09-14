//! `toy finalize-voucher` — the one hand-written verb.
//!
//! It chains three calls the document describes separately: fetch the voucher,
//! enshrine it if it is still open, render it. The decision about *which* calls
//! is [`plan`], a pure function over the voucher that needs no client, no
//! runtime and no fixture; [`run`] only executes what it returns.
//!
//! The two operations the chain depends on are asserted at compile time. If a
//! vendor revision drops `enshrineVoucher`, this file stops compiling and names
//! it — the CLI does not discover it at run time against a customer's ledger.

use api::{Api, Call, Voucher, VoucherStatus};
use typed_openapi::Plan;
use typed_openapi::{SyncClient, render};

use crate::app::Error;
use crate::output::Output;

const _: () = assert!(api::documented(
    "enshrineVoucher",
    "POST",
    "/vouchers/{id}/enshrine"
));
const _: () = assert!(api::documented(
    "renderVoucher",
    "GET",
    "/vouchers/{id}/render"
));

/// One call the chain makes after the voucher has been fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Finalize. Irreversible, which is why it happens at most once and only
    /// from the one state that permits it.
    Enshrine { id: i64 },
    /// Rewrite the stored PDF. A GET, and a write — the document says so with
    /// `x-cli-writes`.
    Render { id: i64 },
}

/// What finalizing this voucher means, given what the server says it is.
///
/// - `open` — not yet finalized: enshrine it, then render.
/// - `draft`, `paid` — enshrining is not available from either state, so the
///   chain is the render alone.
/// - no id — the server has never stored this voucher; there is nothing to
///   finalize and the chain is empty.
#[must_use]
pub fn plan(voucher: &Voucher) -> Vec<Step> {
    let Some(id) = voucher.id else {
        return Vec::new();
    };
    // No `_` arm: a status the vendor adds is a compile error here, where
    // someone has to decide whether it may be enshrined.
    let mut steps = match voucher.status {
        VoucherStatus::Open => vec![Step::Enshrine { id }],
        VoucherStatus::Draft | VoucherStatus::Paid => Vec::new(),
    };
    steps.push(Step::Render { id });
    steps
}

/// The typed wrapper one step stands for. The only match on [`Step`], so the
/// dry run and the committed run cannot drift apart.
pub fn call(api: &Api, step: Step) -> Result<Call<'_, Voucher>, api::Error> {
    match step {
        Step::Enshrine { id } => api.enshrine_voucher(id),
        Step::Render { id } => api.render_voucher(id),
    }
}

/// What the chain's writes came to, once the gate has had its say.
#[derive(Debug)]
enum Wrote {
    /// Nothing was sent: the requests printed above are a dry run.
    Nothing,
    /// Every write ran, and this is what the last one answered.
    Everything(Voucher),
}

/// Fetch, decide, then either print the writes or make them.
pub fn run<C: SyncClient>(api: &Api, client: &C, id: i64, commit: bool) -> Result<Output, Error> {
    let fetch = api.get_voucher(id)?;
    let mut printed = vec![render(&fetch.request()?)];
    // The read runs in both modes: the plan is a function of what the server
    // says the voucher is, so there is nothing to print without it.
    let voucher = fetch.send(client)?;
    let steps = plan(&voucher);
    if steps.is_empty() {
        return Ok(Output::note(
            printed.concat(),
            format!("voucher {id} has no server id; there is nothing to finalize\n"),
        ));
    }

    let mut wrote = Wrote::Nothing;
    for step in steps {
        let step = call(api, step)?;
        // The same gate the `raw` path uses, asked once per request, so the
        // chain cannot disagree with a single operation about what a dry run is.
        let decided = Plan::decide(step.effect(), commit, step.request()?);
        printed.push(render(decided.request()));
        if let Plan::Send(_) = decided {
            wrote = Wrote::Everything(step.send(client)?);
        }
    }

    let printed = printed.join("\n");
    Ok(match wrote {
        Wrote::Nothing => Output::note(
            printed,
            "dry run: the fetch above ran, the writes did not. \
             Add --commit to send them.\n",
        ),
        Wrote::Everything(last) => Output::value(&printed, &last),
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot build its fixture should fail loudly and name it"
)]
mod tests {
    use super::*;

    fn voucher(id: Option<i64>, status: VoucherStatus) -> Voucher {
        Voucher {
            id,
            total: "12.50".parse().expect("a valid amount"),
            currency: "EUR".to_owned(),
            status,
            internal_ref: None,
        }
    }

    #[test]
    fn an_open_voucher_is_enshrined_then_rendered() {
        assert_eq!(
            plan(&voucher(Some(5), VoucherStatus::Open)),
            [Step::Enshrine { id: 5 }, Step::Render { id: 5 }]
        );
    }

    #[test]
    fn a_draft_is_only_rendered() {
        assert_eq!(
            plan(&voucher(Some(5), VoucherStatus::Draft)),
            [Step::Render { id: 5 }]
        );
    }

    #[test]
    fn a_paid_voucher_is_not_enshrined_twice() {
        assert_eq!(
            plan(&voucher(Some(7), VoucherStatus::Paid)),
            [Step::Render { id: 7 }]
        );
    }

    #[test]
    fn a_voucher_the_server_never_stored_has_nothing_to_finalize() {
        assert_eq!(plan(&voucher(None, VoucherStatus::Open)), []);
    }
}
