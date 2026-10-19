#!/usr/bin/env python3
"""Documentation consistency checks (P0-09, PHASE0.md Stage F).

Runs over the repository Markdown and fails on the errors that would otherwise let the
documents drift away from the build:

1. every internal link resolves to a file that exists;
2. every root document is linked from the README index;
3. ADR headings are contiguous and each one is in the DECISIONS index with the right anchor;
4. acceptance-criteria headings in MVP.md are contiguous;
5. the capability tables in MVP.md, ARCHITECTURE.md and PACK_SPEC.md list the same IDs;
6. commands marked available in CONTRIBUTING.md actually work, and commands marked planned
   are not silently available.

Usage:
    python3 tests/check_docs.py [ROOT]      # check a repository tree
    python3 tests/check_docs.py --self-test # prove the checks fail on a broken copy

The self-test copies the documents to a temporary directory, breaks one link and one
capability table, and asserts both are reported. It never modifies the repository.
"""

from __future__ import annotations

import json
import re
import shutil
import sys
import tempfile
from pathlib import Path

LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
ADR_HEADING_RE = re.compile(r"^## ADR-(\d{3})\b.*$", re.MULTILINE)
ADR_INDEX_RE = re.compile(r"^\|\s*\[ADR-(\d{3})\]\(#([^)]+)\)", re.MULTILINE)
AC_HEADING_RE = re.compile(r"^### AC-(\d{2})\b", re.MULTILINE)
CAPABILITY_ROW_RE = re.compile(r"^\|\s*`([a-z][a-z_]*\.[a-z][a-z_]*)`", re.MULTILINE)

CAPABILITY_TABLES = {
    "MVP.md": r"^## 4\. Capability set",
    "ARCHITECTURE.md": r"^## 6\. Capability and target model",
    "PACK_SPEC.md": r"^## 6\. Capabilities",
}

EXPECTED_CAPABILITY_COUNT = 15


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def markdown_files(root: Path) -> list[Path]:
    return sorted(root.glob("*.md"))


def check_links(root: Path, errors: list[str]) -> None:
    for document in markdown_files(root):
        body = read(document)
        for target in LINK_RE.findall(body):
            if target.startswith(("http://", "https://", "mailto:", "asset:", "#")):
                continue
            relative = target.split("#", 1)[0]
            if not relative:
                continue
            resolved = (document.parent / relative).resolve()
            if not resolved.exists():
                errors.append(f"broken link in {document.name}: {target}")


def check_readme_index(root: Path, errors: list[str]) -> None:
    readme = root / "README.md"
    body = read(readme)
    linked = {
        target.split("#", 1)[0]
        for target in LINK_RE.findall(body)
        if not target.startswith(("http://", "https://", "mailto:", "#"))
    }
    for document in markdown_files(root):
        if document.name == "README.md":
            continue
        if document.name not in linked:
            errors.append(f"document not linked from README: {document.name}")


def github_slug(text: str) -> str:
    lowered = text.strip().lower()
    cleaned = re.sub(r"[^a-z0-9 \-]", "", lowered)
    return cleaned.replace(" ", "-")


def check_adrs(root: Path, errors: list[str]) -> None:
    body = read(root / "DECISIONS.md")
    headings = {
        int(match.group(1)): match.group(0) for match in ADR_HEADING_RE.finditer(body)
    }
    if not headings:
        errors.append("DECISIONS.md has no ADR headings")
        return

    numbers = sorted(headings)
    expected = list(range(1, numbers[-1] + 1))
    if numbers != expected:
        missing = sorted(set(expected) - set(numbers))
        errors.append(f"ADR numbering is not contiguous; missing {missing}")

    index = {int(m.group(1)): m.group(2) for m in ADR_INDEX_RE.finditer(body)}
    for number in numbers:
        if number not in index:
            errors.append(f"ADR-{number:03d} is not listed in the DECISIONS index")
            continue
        expected_anchor = github_slug(headings[number].lstrip("# ").strip())
        if index[number] != expected_anchor:
            errors.append(
                f"ADR-{number:03d} index anchor is #{index[number]}, expected #{expected_anchor}"
            )
    for number in index:
        if number not in headings:
            errors.append(f"ADR-{number:03d} is in the index but has no section")


def check_acceptance_criteria(root: Path, errors: list[str]) -> None:
    numbers = [int(m.group(1)) for m in AC_HEADING_RE.finditer(read(root / "MVP.md"))]
    if not numbers:
        errors.append("MVP.md has no AC- headings")
        return
    expected = list(range(1, max(numbers) + 1))
    if sorted(numbers) != expected:
        missing = sorted(set(expected) - set(numbers))
        errors.append(f"acceptance criteria are not contiguous; missing {missing}")


def section(body: str, heading_pattern: str, document: str, errors: list[str]) -> str:
    heading = re.search(heading_pattern, body, re.MULTILINE)
    if heading is None:
        errors.append(f"{document} does not contain the heading {heading_pattern!r}")
        return ""
    rest = body[heading.end():]
    next_heading = re.search(r"^## ", rest, re.MULTILINE)
    return rest[: next_heading.start()] if next_heading else rest


def check_capabilities(root: Path, errors: list[str]) -> None:
    tables: dict[str, list[str]] = {}
    for document, heading in CAPABILITY_TABLES.items():
        body = read(root / document)
        tables[document] = sorted(
            set(CAPABILITY_ROW_RE.findall(section(body, heading, document, errors)))
        )

    reference_name = "MVP.md"
    reference = tables[reference_name]
    if len(reference) != EXPECTED_CAPABILITY_COUNT:
        errors.append(
            f"{reference_name} lists {len(reference)} capabilities, expected "
            f"{EXPECTED_CAPABILITY_COUNT}"
        )
    for document, capabilities in tables.items():
        if document == reference_name:
            continue
        if capabilities != reference:
            only_here = sorted(set(capabilities) - set(reference))
            missing = sorted(set(reference) - set(capabilities))
            errors.append(
                f"{document} capability set differs from {reference_name}: "
                f"extra {only_here}, missing {missing}"
            )


NPM_RUN_RE = re.compile(r"npm run ([A-Za-z0-9:_-]+)")
PYTHON_RE = re.compile(r"python3 (\S+\.py)")


def check_contributing_commands(root: Path, errors: list[str]) -> None:
    body = read(root / "CONTRIBUTING.md")
    package_json = root / "package.json"
    scripts: dict[str, str] = {}
    if package_json.exists():
        scripts = json.loads(read(package_json)).get("scripts", {})

    for line in body.splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 3:
            continue
        command, status = cells[1], cells[2]
        if command.startswith("---") or status.startswith("---"):
            continue
        if "kde_integration" in command:
            # VM-gated command; its availability is a Phase 6 concern, not a docs check.
            continue

        available = status.startswith("yes")
        planned = status.startswith("planned")
        if not (available or planned):
            continue

        checks: list[tuple[str, bool]] = []
        npm_match = NPM_RUN_RE.search(command)
        if npm_match:
            script = npm_match.group(1)
            checks.append((f"npm script {script!r}", script in scripts))
        python_match = PYTHON_RE.search(command)
        if python_match:
            target = python_match.group(1)
            checks.append((target, (root / target).exists()))

        for label, exists in checks:
            if available and not exists:
                errors.append(f"CONTRIBUTING marks {label} available but it is missing")
            if planned and exists:
                errors.append(
                    f"CONTRIBUTING marks {label} planned but it already exists"
                )


def run_checks(root: Path) -> list[str]:
    errors: list[str] = []
    check_links(root, errors)
    check_readme_index(root, errors)
    check_adrs(root, errors)
    check_acceptance_criteria(root, errors)
    check_capabilities(root, errors)
    check_contributing_commands(root, errors)
    return errors


def self_test() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory(prefix="morphix-docs-") as temporary:
        scratch = Path(temporary)
        for document in markdown_files(repo_root):
            shutil.copy(document, scratch / document.name)
        if (repo_root / "package.json").exists():
            shutil.copy(repo_root / "package.json", scratch / "package.json")

        readme = scratch / "README.md"
        readme.write_text(
            read(readme) + "\n[Broken](does-not-exist.md)\n", encoding="utf-8"
        )

        mvp = scratch / "MVP.md"
        mvp_body = read(mvp)
        anchor = "| `appearance.color_scheme`"
        if anchor not in mvp_body:
            print("self-test FAILED: could not find the capability table anchor in MVP.md")
            return 1
        mvp.write_text(
            mvp_body.replace(
                anchor,
                "| `appearance.bogus` | Not a real capability | nowhere |\n" + anchor,
                1,
            ),
            encoding="utf-8",
        )

        errors = run_checks(scratch)
        expected = ["does-not-exist.md", "appearance.bogus"]
        missing = [needle for needle in expected if not any(needle in e for e in errors)]
        if missing:
            print("self-test FAILED: checks did not report " + ", ".join(missing))
            for error in errors:
                print(f"  reported: {error}")
            return 1

        print("self-test passed: broken link and capability mismatch were both detected")
        return 0


def main(argv: list[str]) -> int:
    if len(argv) > 1 and argv[1] == "--self-test":
        return self_test()

    root = Path(argv[1]).resolve() if len(argv) > 1 else Path(__file__).resolve().parent.parent
    if not (root / "README.md").exists():
        print(f"not a Morphix documentation root: {root}")
        return 2

    errors = run_checks(root)
    if errors:
        print(f"documentation checks failed ({len(errors)}):")
        for error in errors:
            print(f"  - {error}")
        return 1

    print("documentation checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
