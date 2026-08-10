from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from scripts.conventional_commits import (
    commit_exists,
    git_subjects,
    newest_release_tag_reachable_from,
    push_range_log_args,
    valid_subject,
)

ZERO_SHA = "0" * 40


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(repo), *args],
        text=True,
        env={
            **os.environ,
            "GIT_AUTHOR_NAME": "test",
            "GIT_AUTHOR_EMAIL": "test@example.com",
            "GIT_COMMITTER_NAME": "test",
            "GIT_COMMITTER_EMAIL": "test@example.com",
        },
    ).strip()


def commit(repo: Path, subject: str) -> str:
    (repo / "file.txt").write_text(subject, encoding="utf-8")
    git(repo, "add", "file.txt")
    git(repo, "commit", "-m", subject)
    return git(repo, "rev-parse", "HEAD")


class ValidSubjectTests(unittest.TestCase):
    def test_accepts_conventional_subjects(self) -> None:
        self.assertTrue(valid_subject("feat: add a thing"))
        self.assertTrue(valid_subject("fix(update): install selected channel"))
        self.assertTrue(valid_subject("feat!: break a thing"))

    def test_rejects_unconventional_subjects(self) -> None:
        self.assertFalse(valid_subject("add a thing"))
        self.assertFalse(valid_subject("wip: add a thing"))
        self.assertFalse(valid_subject("feat:"))


class PushRangeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        git(self.repo, "init", "--initial-branch=main")
        self.cwd = Path.cwd()
        os.chdir(self.repo)
        self.addCleanup(os.chdir, self.cwd)

        self.first = commit(self.repo, "feat: first")
        git(self.repo, "tag", "v1.0.0")
        self.second = commit(self.repo, "feat: second")
        self.third = commit(self.repo, "fix: third")

    def test_keeps_range_when_both_endpoints_exist(self) -> None:
        rev_range = f"{self.first}..{self.third}"
        self.assertEqual(push_range_log_args(rev_range), [rev_range])
        self.assertEqual(
            git_subjects(push_range_log_args(rev_range)),
            ["fix: third", "feat: second"],
        )

    def test_falls_back_to_newest_tag_when_before_is_missing(self) -> None:
        missing = "1" * 40
        self.assertEqual(
            push_range_log_args(f"{missing}..{self.third}"),
            [f"v1.0.0..{self.third}"],
        )

    def test_falls_back_to_newest_tag_for_zero_sha(self) -> None:
        self.assertEqual(
            push_range_log_args(f"{ZERO_SHA}..{self.third}"),
            [f"v1.0.0..{self.third}"],
        )

    def test_fallback_ignores_preview_tags(self) -> None:
        git(self.repo, "tag", "preview-2026-01-01-abcdef123456")
        self.assertEqual(
            push_range_log_args(f"{ZERO_SHA}..{self.third}"),
            [f"v1.0.0..{self.third}"],
        )

    def test_falls_back_to_tip_when_no_release_tag_is_reachable(self) -> None:
        git(self.repo, "tag", "--delete", "v1.0.0")
        git(self.repo, "tag", "preview-2026-01-01-abcdef123456")
        self.assertEqual(
            push_range_log_args(f"{ZERO_SHA}..{self.third}"),
            ["--max-count=1", self.third],
        )
        self.assertEqual(
            git_subjects(push_range_log_args(f"{ZERO_SHA}..{self.third}")),
            ["fix: third"],
        )

    def test_rejects_an_after_commit_that_is_not_present(self) -> None:
        missing = "2" * 40
        with self.assertRaises(SystemExit):
            push_range_log_args(f"{self.first}..{missing}")

    def test_commit_and_tag_lookups(self) -> None:
        self.assertTrue(commit_exists(self.third))
        self.assertFalse(commit_exists(ZERO_SHA))
        self.assertEqual(newest_release_tag_reachable_from(self.third), "v1.0.0")


if __name__ == "__main__":
    unittest.main()
