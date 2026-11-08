//! The day-one example reports HTTP failures through its process result.

#![expect(clippy::expect_used, reason = "fixture failures name the broken setup")]

mod support;

use std::process::Command;

use support::Server;

#[test]
fn root_exits_with_failure_and_reports_a_non_success_status_on_stderr() {
    for (status, code, stdout, stderr) in [
        (404, 1, "", "toy: 404 Not Found {}\n"),
        (500, 1, "", "toy: 500 Internal Server Error {}\n"),
        (303, 1, "", "toy: 303 See Other {}\n"),
        (200, 0, "{}\n", ""),
    ] {
        let server = Server::new(status);
        // The example uses the document's server. A local proxy supplies its
        // response without binding that fixed port or changing the document.
        let output = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "run",
                "--offline",
                "--quiet",
                "--example",
                "root",
                "--",
                "vouchers",
                "get",
                "--id",
                "5",
            ])
            .env("ALL_PROXY", &server.url)
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .env_remove("COMPLETE")
            .output()
            .expect("the root example runs");
        assert_eq!(
            output.status.code(),
            Some(code),
            "{status}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), stdout, "{status}");
        assert_eq!(String::from_utf8_lossy(&output.stderr), stderr, "{status}");
    }
}
