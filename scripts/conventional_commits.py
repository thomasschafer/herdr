#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path

ALLOWED_TYPES = {
    "feat",
    "fix",
    "perf",
    "docs",
    "ci",
    "test",
    "refactor",
    "chore",
    "release",
}
SUBJECT_RE = re.compile(r"^(?P<kind>[a-z]+)(?:\([^)]+\))?!?:\s+\S")


def git_subjects(log_args: list[str]) -> list[str]:
    output = subprocess.check_output(
        ["git", "log", "--pretty=format:%s", *log_args], text=True
    ).strip()
    return [line.strip() for line in output.splitlines() if line.strip()]


def commit_exists(rev: str) -> bool:
    return (
        subprocess.run(
            ["git", "rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        ).returncode
        == 0
    )


def newest_release_tag_reachable_from(rev: str) -> str | None:
    """Newest `v*` tag reachable from `rev`.

    Release tags are the stable boundary to fall back to. Preview tags are cut
    and re-cut between releases, so they are deliberately excluded.
    """
    result = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0", "--match", "v*", rev],
        capture_output=True,
        text=True,
        check=False,
    )
    tag = result.stdout.strip()
    return tag if result.returncode == 0 and tag else None


def push_range_log_args(rev_range: str) -> list[str]:
    """Turn a push event's `before..after` range into `git log` arguments.

    A force-push or a newly created branch leaves `before` pointing at a commit
    that is not in the repository, which would otherwise abort the whole check.
    Fall back to the newest release tag so every unreleased subject is still
    validated, and to the pushed tip when no release tag is reachable.
    """
    before, separator, after = rev_range.partition("..")
    if not separator:
        return [rev_range]
    if not commit_exists(after):
        raise SystemExit(
            f"cannot validate commit subjects: {after} is not a commit in this repository"
        )
    if commit_exists(before):
        return [rev_range]

    fallback = newest_release_tag_reachable_from(after)
    if fallback:
        print(
            f"note: {before} is not in this repository (force-push or new branch); "
            f"validating {fallback}..{after} instead"
        )
        return [f"{fallback}..{after}"]
    print(
        f"note: {before} is not in this repository (force-push or new branch) and no tag "
        f"was found; validating {after} only"
    )
    return ["--max-count=1", after]


def valid_subject(subject: str) -> bool:
    match = SUBJECT_RE.match(subject)
    return bool(match and match.group("kind") in ALLOWED_TYPES)


def commit_message_subject(path: Path) -> str | None:
    for line in path.read_text(encoding="utf-8").splitlines():
        subject = line.strip()
        if subject and not subject.startswith("#"):
            return subject
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate conventional commit subjects")
    parser.add_argument("subjects", nargs="*")
    parser.add_argument("--range", dest="rev_range")
    parser.add_argument("--message-file")
    args = parser.parse_args()

    subjects = list(args.subjects)
    if args.rev_range:
        subjects.extend(git_subjects(push_range_log_args(args.rev_range)))
    if args.message_file:
        subject = commit_message_subject(Path(args.message_file))
        if subject:
            subjects.append(subject)

    invalid = [subject for subject in subjects if not valid_subject(subject)]
    if invalid:
        print("invalid commit subject(s):")
        for subject in invalid:
            print(f"  {subject}")
        print(
            "commit subjects must use conventional commits because preview notes are generated from them."
        )
        print("example: fix(update): install selected channel")
        print("expected: type(optional-scope): subject")
        print("allowed types: " + ", ".join(sorted(ALLOWED_TYPES)))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
