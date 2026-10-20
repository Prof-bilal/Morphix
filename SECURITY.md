# Morphix — Security Model and Threat Model

- **Status:** Design baseline, threat-model validated. Mechanisms are confirmed on a live
  Plasma 6.8 session; see [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md) and
  [PROTOTYPE_RESULTS.md](PROTOTYPE_RESULTS.md).
- **Related:** [ARCHITECTURE.md](ARCHITECTURE.md), [PACK_SPEC.md](PACK_SPEC.md),
  [TRANSACTIONS.md](TRANSACTIONS.md), [TESTING.md](TESTING.md).

Security is a product feature, not a hardening phase. A customization platform that can
rewrite a user's desktop is only acceptable if it is conservative about what it accepts,
explicit about what it changes, and honest about what it cannot protect against.

---

## 1. Security objectives

| ID | Objective |
| --- | --- |
| SO-1 | A pack can never cause execution of pack-provided code or commands |
| SO-2 | A pack can never write outside the engine's fixed target map |
| SO-3 | No mutation occurs without a verified snapshot and a durable journal |
| SO-4 | No change is reported as restored unless restoration was verified |
| SO-5 | Morphix never runs as root and never escalates privileges |
| SO-6 | The app works offline and never exfiltrates user data |
| SO-7 | Hostile input (TOML, paths, files, DBus responses) cannot crash or corrupt the app |
| SO-8 | Every refusal and failure is explainable with a stable error code |
| SO-9 | Pack content never becomes evaluable script text in a Plasma script context |
| SO-10 | The engine never restarts the desktop shell or logs the user out |
| SO-11 | A successful apply is never reported when a config write was lost to a concurrent writer |

## 2. Non-goals and explicit limitations

These are documented so the product does not claim protection it cannot provide:

- **A hostile process with the user's UID is out of scope.** It can already edit
  `~/.config` directly, attach to the session bus, or replace the Morphix binary. Morphix
  protects users from *packs and applications*, not from their own account.
- **No OS-level sandbox in V0.1.** There is no seccomp, Landlock, or container boundary;
  safety comes from a closed operation set, fixed targets, and validation. Future hardening
  may add Landlock for defense in depth (post-MVP).
- **No authenticity without signing.** Checksums detect corruption and accidental
  mismatch, not a determined attacker who can rewrite both assets and checksums. V0.1's
  trust model is "the user chose this pack directory"; a signed registry is post-MVP.
- **No protection against a compromised KDE session or DBus daemon** that lies about
  state. Verification catches inconsistencies but cannot prove a daemon is honest.
- **Plasma's scripting interface is not a sandbox.** Testing confirmed that
  `evaluateScript` exposes `ConfigFile`, which can create, edit and delete arbitrary
  user config files. Morphix mitigates this by never interpolating pack text into scripts
  and by generating only fixed templates; it cannot make the interface itself safe.
- **No protection against a concurrently edited config file being silently overwritten.**
  Twenty parallel `kwriteconfig6` writers lost 14 of 20 keys in testing. Morphix detects
  its own lost writes and retries, but cannot serialize writers outside its lock.
- **No protection against physical access, kernel compromise, or malware already running.**
- **No backup of the whole system.** Snapshots cover only resources Morphix changes.
- **No archive handling in V0.1.** Packs are installed from directories, not zip/tar
  files, so archive extraction attacks are not in the V0.1 attack surface. Archive support
  would require a dedicated ADR and extraction hardening.

## 3. Assets to protect

1. User configuration: `~/.config/kdeglobals`, `plasmarc`, `kwinrc`,
   `kglobalshortcutsrc`, `kcminputrc`, `plasmashellrc`,
   `plasma-org.kde.plasma.desktop-appletsrc`, `kwinrulesrc`, the `~/.config/kdedefaults/`
   overlay, and any file reached by the fixed target map.
2. User data: installed theme/icon/cursor/wallpaper directories.
3. Reversibility: snapshots, journal, and history.
4. Integrity of the Morphix pack store and application files.
5. User privacy: no telemetry, no network, no file contents in logs.
6. Session stability: no destructive restarts, no lockout, no unbootable state.

## 4. Trust boundaries

```
  UNTRUSTED                 VALIDATED                    TRUSTED (user-owned)
+-------------+     +----------------------+     +-----------------------------+
| pack files  | --> | packs::parser        | --> | ChangePlan (typed, closed)  |
| pack.toml   |     | packs::validator     |     | Snapshot / Journal / DB     |
| assets      |     | integrity::verify    |     | Transaction engine          |
+-------------+     +----------------------+     +--------------+--------------+
                                                                  |
  SEMI-TRUSTED                                                    v
+-------------+     +----------------------+     +-----------------------------+
| UI / webview| --> | Tauri commands       | --> | KDE adapter -> files & DBus |
| (own content|     | (narrow DTOs, IDs)   |     | (only module that mutates   |
|  only)      |     +----------------------+     |  KDE state)                 |
+-------------+                                  +-----------------------------+
                                                                  ^
  USER-CONTROLLED FILESYSTEM / SESSION BUS ----------------------+
```

| Boundary | Who handles it | Rule |
| --- | --- | --- |
| Pack → engine | `packs::parser`, `packs::validator` | Nothing downstream sees raw TOML or pack paths |
| UI → engine | `src-tauri/commands` | DTOs with IDs/enums; no mutation paths from UI |
| Engine → filesystem | `fs::Sandbox`, KDE target map | Writes only to resolved allowlisted targets |
| Engine → DBus | `desktop::kde` | Only documented KDE interfaces; replies are parsed defensively |
| Engine → Plasma script | `desktop::kde::scripting` | Fixed Rust-generated templates; pack values only as closed-set enums or engine-derived paths; never `ConfigFile` |
| Engine → KDE CLI tools | `CommandRunner` | Fixed argv, value from a closed set, no shell, 5 s timeout |
| Engine → SQLite | `storage` | Parameterized SQL only; no pack text in SQL identifiers |
| Snapshot → engine | `snapshots::manifest` | Manifest hash verified; object hashes verified on read |

## 5. Adversary model

| Adversary | Capability | In scope? |
| --- | --- | --- |
| Malicious pack author | Full control of pack files, manifest, checksums, names | Yes |
| Careless pack author | Wrong paths, huge files, incompatible claims, broken assets | Yes |
| Malicious local web content | Cannot be loaded; UI has no remote content | Yes (deny) |
| Malicious process at same UID | Full user access | No (documented) |
| Malicious KDE client | Can write config files directly | Partially (conflict detection) |
| Network attacker | No network traffic exists | Out of scope by construction |
| Supply-chain attacker in Rust/npm deps | Compromised dependency | Partially (lockfiles, review, audits) |

## 6. Threat table

Severity uses impact on a typical user: **Critical** = code execution or unrecoverable
data loss; **High** = unintended desktop modification or loss of reversal; **Medium** =
denial of service or privacy exposure; **Low** = cosmetic or informational.

| ID | Threat | Sev | Mitigation | Residual risk / test |
| --- | --- | --- | --- | --- |
| T-01 | Pack ships a shell script or binary and gets it executed | Critical | No execution path exists; executable bits rejected; extension and file-type allowlists per asset kind; no archive extraction; `CommandRunner` never accepts pack input | None known. Tests: security fixtures, `tests/architecture.rs` forbids process APIs outside `CommandRunner` |
| T-02 | Command injection through a version probe or adapter call | High | No shell; fixed argv arrays; absolute binary paths; no pack-controlled arguments; 2 s timeout; strict output parsing; DBus calls use typed arguments | Phase 6 spikes confirm probe/adapter behavior (PRD A-2, A-4) |
| T-03 | Path traversal (`../../.ssh/authorized_keys`) in an asset path | Critical | Paths must be relative, normalized, and under allowed pack subdirectories; `..` and absolute paths rejected before filesystem access; pack root canonicalized | Tests: traversal fixtures |
| T-04 | Symlink inside pack points outside (to `/etc` or `~/.ssh`) | Critical | Symlinks and hardlinks rejected anywhere in the pack during validation; directories walked with `symlink_metadata` and never followed; asset copy uses no-follow semantics | TOCTOU window between validation and copy closed by copying from an opened directory handle or re-checking at copy time (prototype A-8) |
| T-05 | Target path is a symlink (`~/.config/kdeglobals -> ~/dotfiles/...`) | High | Refuse to write through symlinked targets (`E_SYMLINK_TARGET`); no-follow open of the final component | Dotfiles users must de-symlink or use a future opt-in; documented |
| T-06 | Overwrite of unrelated user files or themes | High | Fixed target map; engine-derived installed names; ownership markers; `E_TARGET_EXISTS` when a path is not owned by this pack; no whole-directory deletes outside owned trees | Tests: ownership/collision fixtures |
| T-07 | Privilege escalation, silent `sudo`, root execution | Critical | Refuse to run when `geteuid() == 0`; no setuid, no polkit, no privileged helper, no system paths; `sudo` string and process APIs banned by CI checks | None known |
| T-08 | Malicious archive (zip-slip, bombs) | High | V0.1 accepts directories only, not archives; no extraction code. Future archive support requires ADR + hardened extractor | None in V0.1 |
| T-09 | Hostile TOML: exponential parse, huge strings, deep nesting | Medium | 256 KiB manifest cap before parsing; TOML parser limits; string/array/description limits; parse errors are typed; no panics (`unwrap` denied in CI) | Fuzz corpus in Phase 2 |
| T-10 | Oversized assets or file-count exhaustion | Medium | Total 200 MiB, per-file 50 MiB, 2,000 files, depth 8, preview 5 MiB each; checks precede reads; streaming copy with size accounting | Disk-full during copy tested |
| T-11 | Unsafe URLs / remote content in packs | High | No asset URL fields; `homepage` is `https://` and display-only, opened only on explicit click; no remote content in the webview; CSP blocks network loads | Tests: URL fixtures rejected |
| T-12 | XSS via pack name/description/preview text in the webview | High | Pack text rendered as text, not HTML; no `dangerouslySetInnerHTML`; strict CSP; previews restricted to PNG/JPEG/WebP served via Tauri asset protocol; SVG previews rejected (SVG can carry script) | React tests + CSP config |
| T-13 | Pack spoofing / typosquatting of a known pack | Medium | Reverse-DNS IDs; reserved prefix rules; catalog displays source (bundled/local), license, hash, and applied state; local packs are labeled; signing post-MVP | No authenticity in V0.1; documented |
| T-14 | Checksum manifest manipulation or downgrade | Medium | Checksums pinned at install with source recorded (`declared` vs `generated_local`); plan binds pack content hash; mismatches refuse install/apply; no auto-update | Local unverified packs clearly labeled |
| T-15 | Dependency manipulation between packs | High | Dependencies validated, cycles rejected, dependency must be installed and applied; no auto-install; no version resolution that downloads anything; plan lists dependency effects | Tests: dependency fixtures |
| T-16 | Race: two applies, or UI double-click | High | Exclusive OS file lock + DB `BEGIN IMMEDIATE`; second apply gets `E_BUSY`; idempotency keys in the journal; apply button disabled after first click | Concurrency tests |
| T-17 | Race: KDE app writes config between plan and apply | High | Precondition hashes re-checked inside the lock; `E_PLAN_STALE` with no writes; drift re-check at verification | Cannot fully eliminate concurrent writers; reported honestly |
| T-18 | TOCTOU on symlink swap between check and write | High | Re-validate ancestors immediately before write; no-follow open where available; write to temp in the same directory and `rename` (which does not follow the destination symlink) | Prototype A-8 confirms no-follow behavior on supported filesystems |
| T-19 | Corrupted or truncated snapshot | High | Content-addressed objects; object hash verified on read; manifest hash verified; snapshot cannot be marked complete until every object reads back; `E_SNAPSHOT_CORRUPT` blocks apply/restore | Corruption tests |
| T-20 | Incomplete rollback reported as success | Critical | Reversal is verified per resource; success requires verification; otherwise `rollback_failed` + recovery report; no optimistic messaging | Rollback failure tests |
| T-21 | Crash mid-apply leaves partial state | Critical | Journal-before-action; startup recovery reverses unknown/applied operations; commit is atomic; `recovery_required` blocks further mutation | Crash-at-every-boundary tests |
| T-22 | Malicious or malformed DBus replies | Medium | Typed argument marshalling; timeouts; no arbitrary method names; DBus errors mapped to `E_ADAPTER_UNAVAILABLE`; never parse script output as code | Adapter fake tests |
| T-23 | Log leakage of user content | Medium | Logs record paths, keys, hashes, and outcomes; never file contents or asset bytes; log file mode 0600; diagnostics export requires explicit user action | Log redaction tests |
| T-24 | Tauri IPC exposure to remote/injected content | High | Only local bundled UI; no remote URL loading; Tauri capabilities restrict the command set; CSP; no shell plugin; no fs plugin exposed to JS | Config review in Phase 7 |
| T-25 | SQL injection from pack fields | High | Parameterized statements only; pack strings never concatenated into SQL; JSON columns validated on read | Storage tests with quote/`;` fixtures |
| T-26 | Privacy: telemetry or network exfiltration | High | No HTTP client dependency in the core or app; no analytics; diagnostics are local files exported manually | Dependency audit; offline test |
| T-27 | Root-owned or group-writable binary used for probes | Low | Probe binaries must be regular files owned by root and not group/world writable, else probe is skipped | Probe tests |
| T-28 | Denial of service by huge KDE config files | Medium | Config file read cap (16 MiB); refusal with `E_CONFIG_TOO_LARGE`; no unbounded memory growth | Limits tests |
| T-29 | Pack data injected into a generated Plasma script (wallpaper path, panel value, widget type) | Critical | Scripts are fixed Rust templates; only closed-set enum identifiers and engine-derived absolute paths are emitted, JSON-escaped, validated for quotes/newlines/backslashes; pack never supplies script text | Script snapshot tests; a fixture pack with `";print(1);//` in every string field must produce byte-identical script templates |
| T-30 | Use of the scripting `ConfigFile` object to escape the target map | Critical | Templates never construct `ConfigFile`; an architecture test greps generated script sources and fails on the identifier; read-back templates use only `desktops()/panels()/widgets()/readConfig` | Static check + script golden tests |
| T-31 | Silent lost config write from a concurrent KDE writer (14/20 writes lost in testing) | High | Write under the engine lock, read back the effective value, bounded retry, then `E_CONFIG_WRITE_RACE`; never report success on a lost write | Concurrency test with 20 parallel external writers |
| T-32 | Engine-triggered shell restart leaves the user with no panel | Critical | The engine never invokes `systemctl` or `plasmashell --replace`. Observed: repeated restarts hit systemd's rate limit and left the session without a shell | Architecture test forbidding `systemctl`/`plasmashell` argv; manual restart documented as user action |
| T-33 | Orphaned `[PlasmaViews][Panel N]` / `[Tiling][uuid]` sections diverge from the recorded state | Medium | Cleanup for objects the transaction created; verify absence; `E_RESIDUE_CLEANUP_FAILED` otherwise | Panel/desktop create-remove tests assert absence |
| T-34 | Effect/decoration id drift between Plasma versions silently matching nothing | Medium | Derive available effect ids from `/Effects.listOfEffects` at runtime, intersect with the allowlist, refuse unknown names; never copy Plasma 5 `kwin4_effect_*` ids | Versioned allowlist tests; 6.8 fixture proves `contrast` is absent |
| T-35 | Local-theme package declaring a component that is not installed (e.g. `widgetStyle=Windows`) | Low | Validate each declared component before planning; report `Unavailable` instead of letting KDE fall back silently | L&F decomposition tests against a synthetic package |

## 7. Process execution policy

Two classes of child process are permitted, both through `CommandRunner`.

**1. Version probes (detection fallback):**

- `plasmashell --version`
- `kwin_wayland --version`

**2. Appearance activation tools (only where Plasma's own tool performs the live
notification and no DBus equivalent exists):**

- `plasma-apply-colorscheme <scheme>` — `<scheme>` must match an installed scheme name
  and `^[A-Za-z0-9_-]{1,64}$`.
- `plasma-apply-cursortheme <theme>` — same validation, plus the theme must exist in an
  icon directory with a `cursors/` subdirectory.
- `plasma-apply-desktoptheme <style>` — same validation, must exist in an installed
  plasma style directory.

Rules for both classes:

- No shell, ever (`sh -c` is banned).
- No pack-controlled arguments: values come from validated installed-asset sets, and the
  argv is a fixed array of at most three elements.
- Binary resolved from a small fixed list of absolute paths; must be a regular file owned
  by root and not group/world writable.
- 2 s timeout for probes, 5 s for apply tools; stdout capped at 4 KiB; stderr captured only
  for the error code.
- Probe output parsed with a strict version regex; any other output is rejected.
- A tool's success message is **never** treated as verification; the adapter re-reads state.
- Failure is not fatal for probes (falls back to `Unverified`/`Unsupported`); for apply
  tools it is a transaction failure that triggers rollback.
- CI contains an architecture test that fails if `std::process::Command` appears outside
  `platform::probe::CommandRunner`.

Explicitly **not** spawned: `sudo`, `pkexec`, package managers, `kwriteconfig6`,
`kreadconfig6`, `systemctl`, `plasmashell --replace`, `kwin_wayland --replace`, `bash`
scripts, or any pack-supplied path. Config reads and writes happen in-process.

The restart prohibition is absolute: a shell restart is the one action that can leave a
user without a desktop, and testing showed it can fail under systemd rate limiting.

## 8. Filesystem policy

Writable roots (defense in depth):

```
~/.config                       (specific allowlisted files only)
~/.local/share                  (specific allowlisted subdirectories only)
~/.local/state/morphix
~/.local/share/morphix
```

Primary control is the **fixed target map**, not the root allowlist. A path is writable
only if it is the exact resolved target of an operation kind:

Detection is read-only with one documented exception: the data-directory writability probe
creates a zero-byte file with `create_new` inside `~/.local/share/morphix/`, then removes
it. It never writes to KDE config, the pack store, or anywhere outside the Morphix data
root, and the detection suite snapshots a temporary tree to prove no other write occurs.

| Kind | Target |
| --- | --- |
| Color scheme | `~/.local/share/color-schemes/<slug>-<key>.colors` |
| Icon/cursor theme | `~/.local/share/icons/<slug>-<key>/` |
| Plasma style | `~/.local/share/plasma/desktoptheme/<slug>-<key>/` |
| Window decoration | `~/.local/share/aurorae/themes/<slug>-<key>/` |
| Wallpaper | `~/.local/share/morphix/assets/wallpapers/<pack-id>/<key>.<ext>` |
| Config | `~/.config/kdeglobals`, `~/.config/plasmarc`, `~/.config/kwinrc`, `~/.config/kglobalshortcutsrc`, `~/.config/kcminputrc`, `~/.config/plasmashellrc`, `~/.config/plasma-org.kde.plasma.desktop-appletsrc`, `~/.config/kwinrulesrc` |
| Defaults overlay | `~/.config/kdedefaults/<same basenames>` (snapshotted because theme application rewrites it) |
| Snapshot | `~/.local/share/morphix/snapshots/<id>/` |
| Pack store | `~/.local/share/morphix/packs/<pack-id>/` |

Additional rules:

- `..`, absolute pack paths, NUL bytes, and non-UTF-8 names are rejected.
- The final component must not be a symlink; ancestor directories are checked and
  re-checked immediately before writing.
- Writes are temp-file + fsync + atomic rename; permissions are `0644` files, `0755`
  directories; existing modes are preserved for config files.
- Deleting a directory requires an ownership marker matching the pack, or a snapshot entry
  recording that exact path as formerly present.
- Mount points, `/proc`, `/sys`, `/dev`, `/etc`, and `/usr` are never targets; the path
  validator rejects anything outside the user's home-based roots even if a pack somehow
  produced it.

## 9. DBus policy

- Only the session bus; no system bus access.
- The client is `zbus`'s blocking API over `async-io`; no Tokio and no generic
  "call any method" path. Presence is one typed `org.freedesktop.DBus.NameHasOwner` call
  per allowlisted service behind `KdeBus`.
- Only these interfaces are used: `org.kde.KWin` (reconfigure), `org.kde.kglobalaccel`
  (shortcut sync), `org.kde.KGlobalSettings` (notify), `org.kde.plasmashell` (wallpaper
  script), and `org.freedesktop.DBus` (name presence/ping).
- Calls use typed marshalling with fixed method names; no method name is derived from pack
  input.
- Timeouts (2 s) on every call; failures are explicit `E_ADAPTER_UNAVAILABLE` errors, never
  retried indefinitely.
- DBus replies are parsed defensively; a malformed reply is a typed error, not a panic.
- The wallpaper script is generated only by Rust, from a constant template; the single
  interpolated value is a validated absolute file URL encoded as a JSON string literal
  (ADR-015). Script text is snapshot-tested and cannot contain a newline.

## 10. Capabilities and consent as a security control

- Capabilities are a closed enum in code; the manifest only selects from it.
- The planner grants a capability only if every operation using it validates, and the
  manifest declares it. Unused or undeclared capabilities fail validation.
- The consent payload includes `plan_hash` and the exact acknowledged capability set. The
  engine refuses mismatches (`E_CONSENT_REQUIRED`) and stale plans (`E_PLAN_STALE`).
- Consent is not a sandbox. It is a human-review control; enforcement comes from the
  operation/target model. This is stated plainly in the UI copy and docs.

## 11. Snapshot and journal integrity

- Objects and manifests are SHA-256 addressed; manifests carry their own hash.
- Snapshot objects are written and fsynced before the journal marks the snapshot complete.
- Only `Storage` writes the journal, inside SQLite transactions with `synchronous=FULL`
  (or `NORMAL` in WAL with an explicit checkpoint before apply; the implementation must
  document which and test crash behavior).
- A snapshot cannot be silently edited: editing an object breaks hashes at restore time;
  editing the manifest breaks `manifest_hash`. This is tamper *evidence*, not tamper
  *prevention*.
- Recovery never deletes snapshot objects; it may only create new ones.

## 12. Cryptography

V0.1 uses SHA-256 only, for integrity and content addressing. There is no signing, no
encryption, and no key management. Do not invent cryptographic protocols here; signing is a
future ADR with a defined key distribution and trust-store model.

## 13. Privacy and logging

- No network access, no accounts, no telemetry, no crash upload.
- Logs live in `~/.local/state/morphix/logs/` with mode 0600 and rotation.
- Log fields: timestamp, level, event, transaction id, plan id, pack id, target path,
  config key, error code. Never file contents, never asset bytes, never the user's home
  path beyond what is needed for diagnostics (diagnostics export shows it; the user chooses
  to share). Fields named in `logging::REDACTED_FIELDS` are replaced with `[redacted]`
  before the line is written, so a careless call site cannot leak them.
- Diagnostics export is explicit, writes a single local JSON/ZIP the user selects, and
  includes version, adapter probe, counts, and log excerpts with paths redacted to
  `$HOME` where feasible.

## 14. Resource exhaustion and denial of service

- Manifest and asset limits in [PACK_SPEC.md](PACK_SPEC.md) §3 are enforced before reads.
- Config reads are capped at 16 MiB; exceeding it refuses the operation rather than
  loading the file.
- Snapshot size is bounded by the plan's resources, which are bounded by the allowlist and
  asset limits (no whole-home capture).
- The UI shows a clear error on disk-full during snapshot or apply; apply never starts if
  free space is below the plan's estimated snapshot + install size plus a safety margin
  (default 64 MiB).
- No unbounded retry loops; DBus and I/O retries are bounded (0 or 1 retry).

## 15. Supply chain

- Rust dependencies are pinned by `Cargo.lock`; frontend by `package-lock.json`.
- CI runs `cargo deny` (licenses/advisories) and `npm audit --omit=dev`; findings are
  triaged, not ignored.
- New dependencies require justification in the PR: what it does, why the standard library
  is insufficient, maintenance status, and license compatibility.
- `unsafe` is denied by default (`#![forbid(unsafe_code)]` in `morphix-core`); any exception
  needs an ADR and a safety comment.
- The Tauri capability set exposes only the commands listed in
  [ARCHITECTURE.md](ARCHITECTURE.md) §13. No shell, fs, or http plugins are exposed to the
  webview.

## 16. Security testing and release gates

Required before any release:

1. All security fixtures in [TESTING.md](TESTING.md) §5.4 pass, including traversal,
   symlink, oversized, corrupted snapshot, and malicious manifest cases.
2. Rollback failure tests pass, including crash-at-every-boundary.
3. A manual review confirms `std::process::Command` appears only in the probe module, and
   no `sudo`/`pkexec` strings exist.
4. Tauri CSP and capability configuration reviewed; no remote content load path.
5. No network client dependency in the dependency tree of `morphix-core` or `src-tauri`
   (except transitive build tooling that is not linked into the apply path).
6. Log sample review confirms no file contents.
7. Any accepted residual risk is recorded in [DECISIONS.md](DECISIONS.md) with a rationale.

## 17. Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Use the repository's private
security advisory feature (GitHub Security Advisories) with reproduction steps and affected
versions. Reports are acknowledged, reproduced against a Plasma 6 test VM, and fixed with a
regression test before disclosure. There is no bug bounty and no dedicated security email
in V0.1.

## 18. Future hardening (not V0.1)

- Pack signing with a documented trust store and key rotation.
- Landlock/seccomp confinement of the apply process (defense in depth, not a replacement for
  the operation model).
- A minimal privileged helper, only if a validated need appears: separate process, typed
  narrow API, no shell, no arbitrary paths, explicit polkit action.
- Reproducible builds and signed release artifacts.
- Snapshot encryption at rest for users who store home directories on shared media.
