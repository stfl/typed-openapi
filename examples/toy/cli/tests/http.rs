//! The copyable adapters and day-one CLI against a local HTTP server.

#![expect(clippy::expect_used, reason = "fixture failures name the broken setup")]

mod support;

use cli::client::Ureq;
use support::Server;
#[cfg(feature = "reqwest-client")]
use typed_openapi::AsyncClient as _;
use typed_openapi::tree::{self, Outcome};

#[test]
fn ureq_reports_each_redirect_as_the_writes_answer() {
    let api = api::Api::new().expect("the embedded document loads");
    let matches = clap::Command::new("toy")
        .subcommands(tree::commands(api.document()))
        .get_matches_from([
            "toy",
            "vouchers",
            "create",
            "--total",
            "1.00",
            "--currency",
            "EUR",
            "--status",
            "draft",
            "--commit",
        ]);
    let client = Ureq::new();
    // Start with the probe's 303, then cover the rest of the status class.
    for status in std::iter::once(303).chain((300..400).filter(|s| *s != 303)) {
        let server = Server::new(status);
        let outcome = tree::dispatch(
            api.document(),
            &server.url.parse().expect("a loopback URI"),
            &client,
            &matches,
        )
        .expect("HTTP status codes are answers");
        assert!(
            matches!(&outcome, Outcome::Sent(_)),
            "{status}: an answer was sent"
        );
        let Outcome::Sent(response) = outcome else {
            unreachable!()
        };
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(
            response
                .headers()
                .get("location")
                .expect("Location survives"),
            &format!("{}/elsewhere", server.url)
        );
        assert_eq!(
            response.body().as_slice(),
            if status == 304 { &b""[..] } else { &b"{}"[..] },
            "{status}"
        );
    }
}

#[cfg(feature = "reqwest-client")]
#[test]
fn reqwest_reports_each_redirect_as_the_writes_answer() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the adapter runtime");
    runtime.block_on(async {
        let client = cli::client::Reqwest::new().expect("the adapter builds");
        for status in std::iter::once(303).chain((300..400).filter(|s| *s != 303)) {
            let server = Server::new(status);
            let request = http::Request::post(format!("{}/vouchers", server.url))
                .header("content-type", "application/json")
                .body(br#"{"total":"1.00","currency":"EUR","status":"draft"}"#.to_vec())
                .expect("the probe's write");
            let response = client
                .send(request)
                .await
                .expect("HTTP status codes are answers");
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(
                response
                    .headers()
                    .get("location")
                    .expect("Location survives"),
                &format!("{}/elsewhere", server.url)
            );
            assert_eq!(
                response.body().as_slice(),
                if status == 304 { &b""[..] } else { &b"{}"[..] },
                "{status}"
            );
        }
    });
}
