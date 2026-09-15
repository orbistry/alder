#!/usr/bin/env python3
"""Aggregate sampo per-crate changelogs into one set of release notes.

Runs from the dist release workflow (post-announce) on the alder-cli tag.
Sampo tags every co-released crate at the same commit, so the release set
is exactly the `<crate>-v<version>` tags pointing at HEAD. Each crate's
changelog section for its released version is parsed, and because sampo
writes a multi-crate changeset verbatim into every affected crate's
changelog (sometimes at different bump levels), entries are deduplicated
by content, labeled with the crates they touched, and grouped under the
highest bump level any crate recorded. "Updated dependencies" bullets are
collapsed into one compact section.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

LEVELS = ["Major changes", "Minor changes", "Patch changes"]

SECTION_RE = re.compile(r"^## (?P<version>\d+\.\d+\.\d+(?:[-+][\w.]+)?)(?:\s.*)?$")
SUBSECTION_RE = re.compile(r"^### (?P<level>.+?)\s*$")
TAG_RE = re.compile(r"^(?P<crate>[a-z][a-z0-9-]*)-v(?P<version>\d+\.\d+\.\d+(?:[-+][\w.]+)?)$")
DEPS_RE = re.compile(r"^- Updated dependencies:\s*(?P<deps>.+?)\s*$")
NOTES_START = "<!-- alder:aggregate-release-notes:start -->"
NOTES_END = "<!-- alder:aggregate-release-notes:end -->"


def compose_release_body(body: str, plan: dict, notes: str) -> str:
    """Replace only our notes, never infer boundaries from arbitrary headings."""
    if NOTES_START in notes or NOTES_END in notes:
        raise ValueError("Aggregated changelog contains reserved release-note markers")
    replacement = f"{NOTES_START}\n## Release Notes\n\n{notes.rstrip()}\n{NOTES_END}"
    starts, ends = body.count(NOTES_START), body.count(NOTES_END)
    if starts or ends:
        if starts != 1 or ends != 1 or body.index(NOTES_START) >= body.index(NOTES_END):
            raise ValueError("Ambiguous aggregate-note markers; release left unchanged")
        start = body.index(NOTES_START)
        end = body.index(NOTES_END) + len(NOTES_END)
        return body[:start] + replacement + body[end:]
    original = plan.get("announcement_changelog")
    if not isinstance(original, str) or not original.strip():
        raise ValueError("Original cargo-dist changelog unavailable; release left unchanged")
    section = "## Release Notes\n\n" + original.replace("\r\n", "\n") + "\n\n"
    if body.count(section) != 1:
        raise ValueError("Original cargo-dist notes missing or ambiguous; release left unchanged")
    return body.replace(section, replacement + "\n\n", 1)


def released_crates() -> list[tuple[str, str]]:
    """(crate, version) pairs for every release tag pointing at HEAD."""
    out = subprocess.run(
        ["git", "tag", "--points-at", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    pairs = []
    for line in out.splitlines():
        match = TAG_RE.match(line.strip())
        if match:
            pairs.append((match.group("crate"), match.group("version")))
    return sorted(pairs, key=lambda pair: (pair[0] != "alder-cli", pair[0]))


def extract_section(changelog: str, version: str) -> str | None:
    """The body of the `## <version>` section, without its heading."""
    lines = changelog.splitlines()
    start = None
    for i, line in enumerate(lines):
        match = SECTION_RE.match(line)
        if match and match.group("version") == version:
            start = i + 1
            break
    if start is None:
        return None
    end = len(lines)
    for i in range(start, len(lines)):
        if SECTION_RE.match(lines[i]):
            end = i
            break
    return "\n".join(lines[start:end])


def parse_entries(section: str) -> list[tuple[str, str]]:
    """(level, entry_text) pairs; entries keep their continuation lines."""
    entries = []
    level = None
    entry_lines: list[str] | None = None

    def flush():
        nonlocal entry_lines
        if entry_lines is not None:
            entries.append((level, "\n".join(entry_lines).rstrip()))
            entry_lines = None

    for line in section.splitlines():
        sub = SUBSECTION_RE.match(line)
        if sub:
            flush()
            level = sub.group("level")
            continue
        if level is None:
            continue
        if line.startswith("- "):
            flush()
            entry_lines = [line]
        elif entry_lines is not None:
            entry_lines.append(line)
    flush()
    return entries


def main() -> int:
    # GitHub descriptions and Sampo changelogs are UTF-8, including on Windows
    # runners whose default redirected console encoding may be a legacy codepage.
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser()
    parser.add_argument("--body", type=Path, help="Existing GitHub release JSON containing its body")
    parser.add_argument("--plan", type=Path, help="Original cargo-dist announcement manifest")
    parser.add_argument(
        "--crate",
        action="append",
        metavar="NAME@VERSION",
        help="override the released set instead of reading git tags at HEAD",
    )
    args = parser.parse_args()
    if bool(args.body) != bool(args.plan):
        parser.error("--body and --plan must be supplied together")

    if args.crate:
        releases = [tuple(spec.split("@", 1)) for spec in args.crate]
    else:
        releases = released_crates()
    if not releases:
        print("no release tags point at HEAD", file=sys.stderr)
        return 1

    # entry text -> [crates]; insertion order preserved for stable output
    grouped: dict[str, dict] = {}
    dep_updates: list[tuple[str, str]] = []
    missing: list[str] = []

    for crate, version in releases:
        changelog = Path("crates") / crate / "CHANGELOG.md"
        if not changelog.is_file():
            missing.append(f"{crate} (no changelog)")
            continue
        section = extract_section(changelog.read_text(encoding="utf-8"), version)
        if section is None:
            missing.append(f"{crate} {version} (no changelog section)")
            continue
        for level, entry in parse_entries(section):
            deps = DEPS_RE.match(entry)
            if deps:
                dep_updates.append((crate, deps.group("deps")))
                continue
            if level not in LEVELS:
                level = "Patch changes"
            info = grouped.setdefault(entry, {"crates": [], "level": level})
            if crate not in info["crates"]:
                info["crates"].append(crate)
            # A shared changeset may be minor for one crate and patch for
            # another: report it once, under the highest level.
            if LEVELS.index(level) < LEVELS.index(info["level"]):
                info["level"] = level

    out: list[str] = []
    versions = ", ".join(f"{crate} {version}" for crate, version in releases)
    out.append(f"_Released: {versions}_")

    for level in LEVELS:
        bullets = [
            (entry, info["crates"])
            for entry, info in grouped.items()
            if info["level"] == level
        ]
        if not bullets:
            continue
        out.append("")
        out.append(f"### {level}")
        out.append("")
        for entry, crates in bullets:
            label = ", ".join(f"`{crate}`" for crate in crates)
            out.append(f"- {label} — {entry[2:]}")

    if dep_updates:
        out.append("")
        out.append("### Dependency updates")
        out.append("")
        for crate, deps in dep_updates:
            pretty = ", ".join(dep.strip() for dep in deps.split(","))
            out.append(f"- `{crate}`: {pretty}")

    if missing:
        print(f"warning: skipped {', '.join(missing)}", file=sys.stderr)

    notes = "\n".join(out)
    if args.body:
        try:
            body = json.loads(args.body.read_text(encoding="utf-8"))["body"]
            if not isinstance(body, str):
                raise ValueError("Release body must be text; release left unchanged")
            plan = json.loads(args.plan.read_text(encoding="utf-8"))
            if not isinstance(plan, dict):
                raise ValueError("Release plan must be an object; release left unchanged")
            notes = compose_release_body(body, plan, notes)
        except (ValueError, OSError, KeyError, TypeError) as error:
            print(str(error), file=sys.stderr)
            return 1
    print(notes, end="" if args.body else "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
