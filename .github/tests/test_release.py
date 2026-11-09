"""Run the release workflow's decision step against local Git history."""

import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]


def decision_script():
    workflow = (ROOT / ".github/workflows/release.yml").read_text()
    step = workflow.split("      - id: read\n", 1)[1]
    block = step.split("        run: |\n", 1)[1]
    lines = []
    for line in block.splitlines():
        if line.strip() and not line.startswith("          "):
            break
        lines.append(line)
    return textwrap.dedent("\n".join(lines))


class ReleaseDecision(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "Release test")
        self.git("config", "user.email", "release@example.invalid")
        self.git("init", "--bare", "-q", str(self.root / "origin.git"))
        self.git("remote", "add", "origin", str(self.root / "origin.git"))
        crate = self.repo / "typed-openapi"
        (crate / "src").mkdir(parents=True)
        (crate / "src/lib.rs").write_text("")
        (crate / "Cargo.toml").write_text(
            '[package]\nname = "typed-openapi"\nversion.workspace = true\n'
        )
        (self.repo / "CHANGELOG.md").write_text("## 0.0.2\n\nRelease notes.\n")
        self.before = self.commit("0.0.1", "before bump")
        self.bump = self.commit("0.0.2", "version bump")
        self.successor = self.commit("0.0.2", "unchanged successor")

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.repo, check=True, capture_output=True, text=True
        ).stdout.strip()

    def commit(self, version, message):
        (self.repo / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["typed-openapi"]\n'
            f'[workspace.package]\nversion = "{version}"\n'
        )
        self.git("add", ".")
        self.git("commit", "-q", "--allow-empty", "-m", message)
        self.git("push", "-q", "origin", "HEAD:refs/heads/main")
        return self.git("rev-parse", "HEAD")

    def decide(self, commit, before, event="push", ref="refs/heads/main"):
        self.git("checkout", "-q", "--detach", commit)
        output = self.root / "output"
        output.write_text("")
        result = subprocess.run(
            ["bash", "-c", decision_script()],
            cwd=self.repo,
            env={
                **os.environ,
                "GITHUB_EVENT_NAME": event,
                "GITHUB_REF": ref,
                "GITHUB_SHA": commit,
                "BEFORE": before,
                "GITHUB_OUTPUT": str(output),
            },
            capture_output=True,
            text=True,
        )
        return result, output.read_text()

    def assert_mode(self, mode, commit, before, **event):
        result, output = self.decide(commit, before, **event)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"mode={mode}\n", output)
        self.assertIn("version=0.0.2\n", output)
        self.assertIn("tag=v0.0.2\n", output)

    def test_unchanged_successor_cannot_take_over_failed_release(self):
        # Neither failure point has made a tag. Registry state is not an input
        # to selection, so both must leave only the original event releasable.
        for failure in ("before publish", "after publish, before tag"):
            with self.subTest(failure=failure):
                self.assert_mode("release", self.bump, self.before)
                self.assert_mode("rehearse", self.successor, self.bump)
                self.assert_mode("release", self.bump, self.before)

    def test_original_bump_selects_release(self):
        self.assert_mode("release", self.bump, self.before)

    def test_push_compares_event_before_not_immediate_parent(self):
        self.assert_mode("release", self.successor, self.before)

    def test_retry_keeps_original_version_after_another_bump(self):
        self.commit("0.0.3", "later bump")
        self.assert_mode("release", self.bump, self.before)

    def test_shallow_checkout_fetches_missing_comparison_commit(self):
        clone = self.root / "shallow"
        self.git(
            "clone",
            "-q",
            "--depth=1",
            "--branch=main",
            (self.root / "origin.git").as_uri(),
            str(clone),
        )
        self.repo = clone
        self.assert_mode("release", self.successor, self.before)

    def test_dispatch_rehearses_without_comparison_history(self):
        for ref in ("refs/heads/main", "refs/heads/feature/test"):
            with self.subTest(ref=ref):
                self.assert_mode(
                    "rehearse", self.bump, "", event="workflow_dispatch", ref=ref
                )

    def test_push_to_another_ref_rehearses(self):
        self.assert_mode("rehearse", self.bump, self.before, ref="refs/heads/other")

    def test_existing_tag_rehearses_even_on_original_bump(self):
        self.git("tag", "v0.0.2", self.bump)
        self.git("push", "-q", "origin", "refs/tags/v0.0.2")
        self.assert_mode("rehearse", self.bump, self.before)

    def test_missing_comparison_commit_fails_without_outputs(self):
        for before in ("", "0" * 40, "f" * 40):
            with self.subTest(before=before):
                result, output = self.decide(self.bump, before)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, "")

    def test_unreadable_before_manifest_fails_without_outputs(self):
        for manifest in (
            None,
            "not toml",
            "[workspace]\n",
            '[workspace.package]\nversion = "bad"',
        ):
            with self.subTest(manifest=manifest):
                self.git("checkout", "-q", "--detach", self.before)
                path = self.repo / "Cargo.toml"
                if manifest is None:
                    path.unlink()
                else:
                    path.write_text(manifest)
                self.git("add", ".")
                self.git("commit", "-q", "-m", "unreadable before manifest")
                before = self.git("rev-parse", "HEAD")
                result, output = self.decide(self.bump, before)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, "")

    def test_unavailable_tag_lookup_fails_without_outputs(self):
        self.git("remote", "set-url", "origin", str(self.root / "absent.git"))
        result, output = self.decide(self.bump, self.before)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output, "")

    def test_release_without_changelog_section_fails_without_outputs(self):
        self.git("checkout", "-q", "--detach", self.bump)
        (self.repo / "CHANGELOG.md").write_text("## Unreleased\n")
        self.git("add", ".")
        self.git("commit", "-q", "-m", "missing release notes")
        result, output = self.decide(self.git("rev-parse", "HEAD"), self.before)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output, "")


if __name__ == "__main__":
    unittest.main()
