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
import re
import subprocess
import sys
from pathlib import Path

LEVELS = ["Major changes", "Minor changes", "Patch changes"]

SECTION_RE = re.compile(r"^## (?P<version>\d+\.\d+\.\d+(?:[-+][\w.]+)?)(?:\s.*)?$")
SUBSECTION_RE = re.compile(r"^### (?P<level>.+?)\s*$")
TAG_RE = re.compile(r"^(?P<crate>[a-z][a-z0-9-]*)-v(?P<version>\d+\.\d+\.\d+(?:[-+][\w.]+)?)$")
DEPS_RE = re.compile(r"^- Updated dependencies:\s*(?P<deps>.+?)\s*$")


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
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--crate",
        action="append",
        metavar="NAME@VERSION",
        help="override the released set instead of reading git tags at HEAD",
    )
    args = parser.parse_args()

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
        section = extract_section(changelog.read_text(), version)
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

    print("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
