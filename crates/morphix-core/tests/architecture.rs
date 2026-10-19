//! Architecture boundary test (P0-05).
//!
//! Enforces the module edges in ARCHITECTURE.md §5, the "must not depend on Tauri or a
//! GUI crate" rule in §3, and the module list in §4. The documents win: if this test
//! fails after a refactor, the refactor is wrong or the document needs an ADR.
//!
//! The scanner is exercised against an inline fixture graph as well as the real tree, so
//! a broken scanner cannot make the suite pass vacuously.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Allowed crate-internal edges, transcribed from ARCHITECTURE.md §5.
///
/// A module may depend on exactly these modules and nothing else in this crate. `lib.rs`
/// is the facade and is deliberately absent: it may re-export from anywhere.
const ALLOWED_EDGES: &[(&str, &[&str])] = &[
    ("errors", &[]),
    ("core", &["errors"]),
    ("platform", &["core", "errors"]),
    ("fs", &["errors"]),
    ("desktop", &["core", "errors", "fs", "platform"]),
    ("packs", &["core", "errors", "fs"]),
    ("permissions", &["core", "errors"]),
    (
        "planning",
        &["core", "desktop", "errors", "fs", "packs", "permissions"],
    ),
    ("snapshots", &["core", "desktop", "errors", "fs", "storage"]),
    (
        "transactions",
        &[
            "core",
            "desktop",
            "errors",
            "fs",
            "planning",
            "snapshots",
            "storage",
        ],
    ),
    ("storage", &["core", "errors"]),
    ("system", &["core", "desktop", "errors", "fs", "platform"]),
    ("logging", &["errors"]),
];

/// Crates that must never appear anywhere in `morphix-core` (ARCHITECTURE.md §3).
const FORBIDDEN_CRATES: &[&str] = &["tauri", "wry", "webkit2gtk", "gtk", "winit", "egui"];

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The repository root, where the design documents live.
fn repo_root() -> PathBuf {
    crate_root().join("..").join("..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Every `.rs` file under `src/`, in a stable order, as `(relative path, contents)`.
fn read_sources() -> Vec<(String, String)> {
    /// Walks `src/`, recording each file as `src/<relative path>` so a violation message
    /// points at a real location.
    fn walk_from(src: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()))
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk_from(src, &path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let relative = path
                    .strip_prefix(src)
                    .unwrap_or_else(|_| panic!("{} is not under {}", path.display(), src.display()))
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((format!("src/{relative}"), read(&path)));
            }
        }
    }

    let src = crate_root().join("src");
    let mut sources = Vec::new();
    walk_from(&src, &src, &mut sources);
    assert!(
        !sources.is_empty(),
        "found no sources under {}; the architecture test would pass vacuously",
        src.display()
    );
    sources
}

/// The module a source file belongs to: the first path component after `src/`.
///
/// `src/lib.rs` is the facade and returns `None`.
fn module_of(relative_path: &str) -> Option<String> {
    let rest = relative_path.strip_prefix("src/")?;
    let first = rest.split('/').next()?;
    let name = first.strip_suffix(".rs").unwrap_or(first);
    (name != "lib").then(|| name.to_owned())
}

/// Strips line comments so a `crate::` mentioned in prose is not read as a dependency.
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every module a source file imports through a `crate::` path, including the brace form
/// `use crate::{a::X, b::Y};`.
fn imported_modules(source: &str) -> BTreeSet<String> {
    const MARKER: &str = "crate::";
    let code = strip_line_comments(source);
    let mut found = BTreeSet::new();
    let mut rest = code.as_str();

    while let Some(index) = rest.find(MARKER) {
        let after = &rest[index + MARKER.len()..];
        if let Some(braces) = after.strip_prefix('{') {
            let group = match braces.split_once('}') {
                Some((group, _)) => group,
                None => braces,
            };
            for item in group.split(',') {
                if let Some(name) = module_name_from_path(item.trim()) {
                    found.insert(name);
                }
            }
        } else if let Some(name) = module_name_from_path(after) {
            found.insert(name);
        }
        rest = after;
    }

    found
}

/// Reads a leading identifier path segment, for example `errors` in `errors::code::ErrorCode`.
fn module_name_from_path(text: &str) -> Option<String> {
    let name: String = text
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Modules declared by `pub mod x;` / `mod x;` in `src/lib.rs`.
fn declared_modules(lib_rs: &str) -> BTreeSet<String> {
    lib_rs
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line
                .strip_prefix("pub mod ")
                .or_else(|| line.strip_prefix("mod "))?;
            let name = rest.strip_suffix(';')?;
            name.chars()
                .all(|c| c.is_alphanumeric() || c == '_')
                .then(|| name.to_owned())
        })
        .collect()
}

/// Top-level modules listed in the ARCHITECTURE.md §4 tree.
fn documented_modules(architecture: &str) -> BTreeSet<String> {
    let section = architecture
        .split_once("## 4. Core module tree")
        .expect("ARCHITECTURE.md has a §4 module tree")
        .1;
    section
        .lines()
        .take_while(|line| !line.trim_start().starts_with("Module size discipline"))
        .filter_map(|line| {
            let entry = line
                .strip_prefix("├── ")
                .or_else(|| line.strip_prefix("└── "))?;
            // Entries are followed by an aligned `# comment`; the name is what precedes it.
            let entry = entry.split('#').next().unwrap_or(entry).trim();
            let name = entry.trim_end_matches('/');
            let name = name.strip_suffix(".rs").unwrap_or(name);
            (name != "lib").then(|| name.to_owned())
        })
        .collect()
}

fn allowed_edges() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    ALLOWED_EDGES
        .iter()
        .map(|(module, allowed)| (*module, allowed.iter().copied().collect()))
        .collect()
}

/// Reports each edge that the matrix does not allow.
fn violations(
    sources: &[(String, String)],
    allowed: &BTreeMap<&str, BTreeSet<&str>>,
    modules: &BTreeSet<String>,
) -> Vec<String> {
    let mut violations = Vec::new();
    for (path, contents) in sources {
        let Some(owner) = module_of(path) else {
            continue; // lib.rs is the facade: it may re-export from anywhere
        };
        let permitted = allowed.get(owner.as_str());
        for imported in imported_modules(contents) {
            if !modules.contains(&imported) {
                continue; // a crate-external path, an std path, or a typo caught elsewhere
            }
            if imported == owner {
                continue;
            }
            let is_permitted = permitted.is_some_and(|set| set.contains(imported.as_str()));
            if !is_permitted {
                violations.push(format!("{owner} -> {imported} (in {path})"));
            }
        }
    }
    violations.sort();
    violations
}

#[test]
fn module_list_matches_the_documented_tree() {
    let lib_rs = read(&crate_root().join("src/lib.rs"));
    let declared = declared_modules(&lib_rs);
    let architecture = read(&repo_root().join("ARCHITECTURE.md"));
    let documented = documented_modules(&architecture);

    assert_eq!(
        declared, documented,
        "the modules declared in src/lib.rs must match the ARCHITECTURE.md §4 tree"
    );

    let sources = read_sources();
    let on_disk: BTreeSet<String> = sources
        .iter()
        .filter_map(|(path, _)| module_of(path))
        .collect();
    assert_eq!(
        on_disk, declared,
        "every declared module must exist under src/, and nothing undeclared may exist"
    );

    let matrix: BTreeSet<&str> = ALLOWED_EDGES.iter().map(|(module, _)| *module).collect();
    let declared_refs: BTreeSet<&str> = declared.iter().map(String::as_str).collect();
    assert_eq!(
        matrix, declared_refs,
        "every module needs an entry in ALLOWED_EDGES (ARCHITECTURE.md §5)"
    );
}

#[test]
fn dependency_edges_match_the_architecture() {
    let sources = read_sources();
    let modules: BTreeSet<String> = sources
        .iter()
        .filter_map(|(path, _)| module_of(path))
        .collect();
    let found = violations(&sources, &allowed_edges(), &modules);
    assert!(
        found.is_empty(),
        "forbidden module dependencies (ARCHITECTURE.md §5):\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn core_never_depends_on_a_gui_or_shell_crate() {
    for (path, contents) in read_sources() {
        let code = strip_line_comments(&contents);
        for forbidden in FORBIDDEN_CRATES {
            let direct = format!("use {forbidden}");
            let requirement = format!("{forbidden}::");
            assert!(
                !code.contains(&direct) && !code.contains(&requirement),
                "{path} references the forbidden crate {forbidden} (ARCHITECTURE.md §3)"
            );
        }
    }

    let manifest = read(&crate_root().join("Cargo.toml"));
    for forbidden in FORBIDDEN_CRATES {
        assert!(
            !manifest.contains(&format!("{forbidden} =")),
            "morphix-core must not depend on {forbidden} (ARCHITECTURE.md §3)"
        );
    }
}

/// The scanner must report a forbidden edge in a fixture graph. Without this, a scanner
/// that silently finds nothing would keep the suite green forever.
#[test]
fn a_forbidden_fixture_import_is_reported() {
    let fixture = vec![
        (
            "src/fs/mod.rs".to_owned(),
            "// fs may not depend on packs\nuse crate::packs::validator;\n".to_owned(),
        ),
        (
            "src/packs/mod.rs".to_owned(),
            "// a permitted edge, in the brace form\nuse crate::{core::ids::PackId, errors::ErrorCode};\n".to_owned(),
        ),
        (
            "src/core/mod.rs".to_owned(),
            "// prose about crate::fs::atomic must not count as a dependency\n".to_owned(),
        ),
        (
            "src/lib.rs".to_owned(),
            "pub mod core;\npub mod errors;\npub mod fs;\npub mod packs;\n".to_owned(),
        ),
    ];
    let modules: BTreeSet<String> = fixture
        .iter()
        .filter_map(|(path, _)| module_of(path))
        .collect();
    assert_eq!(
        modules,
        BTreeSet::from(["core".to_owned(), "fs".to_owned(), "packs".to_owned()]),
        "the fixture graph must resolve to the expected modules"
    );

    let found = violations(&fixture, &allowed_edges(), &modules);
    assert_eq!(
        found,
        vec!["fs -> packs (in src/fs/mod.rs)".to_owned()],
        "the scanner must report exactly the forbidden edge"
    );

    // The brace form must be understood, not skipped.
    assert_eq!(
        imported_modules("use crate::{core::ids::PackId, errors::ErrorCode};"),
        BTreeSet::from(["core".to_owned(), "errors".to_owned()])
    );
}

/// Every `.rs` file under `dir`, as `(display path, contents)`.
fn read_rs_files(root: &Path, display_prefix: &str) -> Vec<(String, String)> {
    fn walk(dir: &Path, root: &Path, display_prefix: &str, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()))
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, root, display_prefix, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or_else(|_| {
                        panic!("{} is not under {}", path.display(), root.display())
                    })
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((format!("{display_prefix}/{relative}"), read(&path)));
            }
        }
    }

    let mut files = Vec::new();
    walk(root, root, display_prefix, &mut files);
    files
}

/// SECURITY.md §7: `std::process::Command` may appear only in `platform::probe`.
///
/// This is the check that keeps the allowlist from eroding: a future task that reaches for
/// `Command` in the desktop adapter fails here before it can ship.
#[test]
fn only_the_command_runner_spawns_a_process() {
    let sources = read_sources();
    let mut runner_present = false;

    for (path, contents) in &sources {
        let code = strip_line_comments(contents);
        let spawns = code.contains("process::Command") || code.contains("Command::new(");
        if path == "src/platform/probe.rs" {
            runner_present = spawns;
            continue;
        }
        assert!(
            !spawns,
            "{path} spawns a process; only src/platform/probe.rs may (SECURITY.md §7)"
        );
    }

    assert!(
        runner_present,
        "src/platform/probe.rs must contain the CommandRunner's Command::new call"
    );
}

/// The Tauri shell runs with the webview's privileges, so it must stay free of process
/// spawning and of the banned escalation executables (SECURITY.md §7, T-07).
#[test]
fn the_shell_and_core_never_reference_escalation_tools() {
    let mut targets = vec![(repo_root().join("src-tauri/src"), "src-tauri/src")];
    targets.push((crate_root().join("src"), "src"));

    let mut scanned = 0;
    for (root, prefix) in targets {
        for (path, contents) in read_rs_files(&root, prefix) {
            scanned += 1;
            let code = strip_line_comments(&contents);
            // The runner is the single allowed spawn site; the first test pins that.
            if path != "src/platform/probe.rs" {
                for banned in ["Command::new(", "process::Command"] {
                    assert!(
                        !code.contains(banned),
                        "{path} spawns a process; only src/platform/probe.rs may (SECURITY.md §7)"
                    );
                }
            }
            for banned in ["\"sudo\"", "\"pkexec\"", "sh -c", "bash -c"] {
                assert!(
                    !code.contains(banned),
                    "{path} references the banned escalation form {banned} (SECURITY.md T-07)"
                );
            }
        }
    }

    assert!(
        scanned > 0,
        "no Rust files were scanned; the test would pass vacuously"
    );
}
