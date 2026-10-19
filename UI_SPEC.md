# Morphix — UI and Screen Specification

- **Status:** Design baseline. No UI code exists yet.
- **Scope:** V0.1 (MVP) screens, states, copy, and failure presentation.
- **Normative companions:** [MVP.md](MVP.md) §7 (acceptance criteria),
  [ARCHITECTURE.md](ARCHITECTURE.md) §13 (IPC) and §14 (frontend rules),
  [TRANSACTIONS.md](TRANSACTIONS.md) (outcomes and rollback semantics),
  [SECURITY.md](SECURITY.md) (trust boundaries), [PACK_SPEC.md](PACK_SPEC.md) §8
  (validation codes), [TESTING.md](TESTING.md) §4.9 (component tests).
- **Implementation plan:** [PHASES.md](PHASES.md) Phase 8, [TASKS.md](TASKS.md) P8-01…P8-14.

This document fixes what the user sees. Where it and the code disagree, this document is
wrong only if an ADR says so.

---

## 1. Purpose and non-goals

The interface exists to make one thing impossible: a user believing Morphix changed more,
less, or differently than it did. Every screen therefore answers four questions before it
offers an action:

1. What will change? (plan, targets, old → new values)
2. What is this allowed to touch? (capability list)
3. What happened? (per-operation outcome, verification result)
4. If something went wrong, what is the state of my system now? (restored / not confirmed)

**Non-goals for V0.1:** theme browsing from remote sources, pack authoring UI, a settings
marketplace, live previews rendered on the user's desktop, notification badges, and any UI
that writes files or KDE config directly.

---

## 2. Interface rules (non-negotiable)

| # | Rule | Reason |
| --- | --- | --- |
| U-01 | Only `src/api/client.ts` calls `invoke()`. Components receive typed props. | One seam to mock, one place to audit (ARCHITECTURE §14). |
| U-02 | No component constructs a filesystem path or a KDE config key. | A UI that knows `kdeglobals` will eventually write to it. |
| U-03 | Apply is disabled until the consent checkbox is checked **and** the displayed plan hash equals the plan being applied. | AC-20. |
| U-04 | A screen may only say "restored" when `rollback.outcome == "succeeded"`. Any other outcome renders the recovery view. | AC-23, TRANSACTIONS §8. |
| U-05 | Failures are never dismissed automatically, and success is never shown for a failed operation. | PRD §7.5. |
| U-06 | No raw error text from KDE, shell output, file contents, or stack traces is rendered. Codes come from the registry; messages are Morphix's own. | SECURITY.md T-23. |
| U-07 | Every error code in the registry has an entry in the presentation map (§8). A code without an entry is a build failure, not a fallback string. | Phase 8 exit criterion. |
| U-08 | Pack-supplied text is rendered as text. No `dangerouslySetInnerHTML`, no `innerHTML`, no remote asset URLs. | XSS: pack fields are untrusted input. |
| U-09 | Unsupported or unverified environments are shown as such; the UI never presents a workaround as equivalent. | PRD principle 13. |
| U-10 | Nothing polls a mutating command. Reads may poll; writes are one-shot with progress events. | Avoid duplicate mutations. |

---

## 3. Information architecture

Eight routes, fixed (ARCHITECTURE §14):

| Route | Screen | Primary IPC | Purpose |
| --- | --- | --- | --- |
| `/` | Dashboard | `get_system_status`, `list_transactions` | Is this system supported, and what happened recently? |
| `/discover` | Discover | `list_packs` | What can I apply? |
| `/packs/:packId` | Pack Detail | `get_pack_detail` | What exactly does this pack do? |
| `/packs/:packId/preview` | Preview | `create_pack_plan`, `discard_plan`, `apply_plan` | What will change, and do I consent? |
| `/transactions` | Transactions | `list_transactions`, `get_transaction_detail`, `rollback_transaction`, `get_history` | What has Morphix done, and how do I undo it? |
| `/snapshots` | Snapshots | `list_snapshots`, `get_snapshot_detail`, `create_snapshot_restore_plan` | What states can I return to? |
| `/settings` | Settings | `get_app_info`, local preferences | Where is my data, and what are my options? |
| `/diagnostics` | Diagnostics | `get_diagnostics`, `export_diagnostics` | What do I attach to a bug report? |

**Outcome view** is not a ninth route. An apply result renders in place on
`/packs/:packId/preview` and remains addressable at `/transactions/:transactionId`, so a
refresh cannot lose the result of a mutation.

Navigation: a persistent left rail with the five top-level entries (Dashboard, Discover,
Transactions, Snapshots, Settings) plus Diagnostics in the footer. Pack Detail and Preview
are reached from Discover and are marked as a child of it in the breadcrumb.

---

## 4. Shared components

### 4.1 Capability labels

Plans are grouped by capability, in this fixed order. The ID is data; the label is what the
user reads. Raw IDs appear only in Diagnostics and in copied diagnostics bundles.

| ID | Label | One-line description shown under the label |
| --- | --- | --- |
| `appearance.color_scheme` | Color scheme | Installs a color scheme file and selects it. |
| `appearance.icons` | Icon theme | Installs an icon theme and selects it. |
| `appearance.cursors` | Cursor theme | Installs a cursor theme and selects it. |
| `appearance.plasma_style` | Plasma style | Installs a Plasma style for panels and widgets. |
| `appearance.window_decorations` | Window decorations | Installs a window decoration and selects it. |
| `appearance.wallpaper` | Wallpaper | Installs an image and sets it for each desktop. |
| `settings.kde_global` | Global settings | Changes allowlisted KDE settings. |
| `settings.kwin_effects` | Desktop effects | Turns allowlisted compositor effects on or off. |
| `settings.shortcuts` | Keyboard shortcuts | Changes allowlisted global shortcuts. |
| `appearance.look_and_feel` | Global theme | Applies a packaged theme as the individual changes it contains. |
| `appearance.fonts` | Fonts | Changes the allowlisted font settings. |
| `panel.properties` | Panel properties | Changes a panel's edge, height, alignment, floating and hiding. |
| `panel.widgets` | Panel widgets | Adds, configures or removes widgets on a panel. |
| `panel.create` | Panels | Creates or removes a panel. |
| `settings.desktop_behavior` | Desktop behaviour | Changes allowlisted window behaviour and virtual desktops. |

A group header always shows the label plus the operation count, for example
"Panel widgets · 2 changes". Empty groups are omitted rather than shown as "0 changes".

| Component | Contract |
| --- | --- |
| `StatusBadge` | Four states only: `supported`, `unsupported`, `degraded`, `unknown`. Never a colour without a text label. |
| `CapabilityList` | Renders capability ID + one-sentence description + target summary. Read-only. |
| `PlanView` | Groups operations by capability; each row shows target, old → new, and apply class (`live` / `restart_required`). Totals: assets installed, bytes, config keys, shortcuts, effects. |
| `ConsentDialog` | Lists granted capabilities, the plan hash (short form, copyable), and the sentence in §6.3. Focus-trapped; Escape cancels. |
| `PackCard` | Name, summary, capability count, `bundled`/`local` badge, compatibility badge, and an `invalid` badge when validation failed. |
| `SnapshotCard` | Snapshot short ID, created time, size, resource count, and an unusable marker when the manifest fails its hash check. |
| `RollbackDialog` | Scope summary, then either a plain confirm or the conflict list with the typed `OVERRIDE` phrase (§5.5). |
| `ApplyProgress` | Driven by `morphix://transaction-progress`; shows current operation index over total, current capability, and a log of completed operations. `aria-live="polite"`. |
| `Outcome` | One of four mutually exclusive states (§7). |
| `RecoveryBanner` | Non-dismissable, full-width, top of content. Only appears when `rollback.outcome != "succeeded"`. |
| `ErrorBanner` | Code, message, what was not done, whether anything changed, and the log path. Dismissable only when the transaction is terminal and not recovery-required. |
| `RestartNotice` | Lists operations of class `restart_required` and states that Morphix will not restart Plasma. |
| `EmptyState` | One sentence of what belongs here plus the action that creates it. |
| `OfflineBadge` | Present in Settings and Diagnostics: "Morphix never connects to the network." |

---

## 5. Screen specifications

Each screen lists: entry conditions, data, states, actions, and the tests that encode it.

### 5.1 Dashboard — `/`

**Entry:** app launch, or navigation.

**Data:** `get_system_status` (`refresh: false` on mount, `refresh: true` on **Re-check**);
`list_transactions({ limit: 5 })`.

**States**

| State | Condition | Presentation |
| --- | --- | --- |
| Loading | First status fetch | Skeleton rows; no spinner-only screen. |
| Supported | `status.supported == true` | Green `supported` badge; reasoning collapsed under "Why this is supported". |
| Unsupported | Gate failed (`E_UNSUPPORTED_PLATFORM`) | Amber `unsupported` badge; each failed check as a row: distribution, Plasma version, session type, adapter. Muting of all mutation entry points. |
| Root refusal | `get_system_status` reports root (`E_RUNNING_AS_ROOT`) | Red banner: "Morphix refuses to change settings while running as root." Mutations disabled; browsing still allowed. |
| Degraded | Plasma detected, adapter partially unavailable | `degraded` badge; the unavailable checks are named, and affected capabilities are marked unavailable in Discover. |
| Error | Status call failed | `ErrorBanner` with `get_system_status` failure; **Re-check** retries. |
| Empty history | No transactions | "No changes yet. Anything Morphix applies appears here." |

**Actions:** **Re-check** (status only), open recent transaction, navigate to Discover.

**Rules:** unsupported or root state disables every button that leads to a mutation, and
the disabled control carries a tooltip naming the reason. The support verdict text must
match MVP §1.1 exactly: supported on the validated matrix, not on an unverified range.

**Tests:** supported/unsupported/root/degraded rendering; mutation entry points disabled
with reason; recent list ordering (AC-04, AC-17).

### 5.2 Discover — `/discover`

**Data:** `list_packs`; `get_system_status` for compatibility filtering.

**States**

| State | Presentation |
| --- | --- |
| Loading | Three card skeletons. |
| Empty | "No packs found. Morphix ships four; if none appear, the pack store could not be read." + Diagnostics link. |
| Populated | Cards: name, summary, capability count, source badge (`bundled` / `local`), compatibility badge. |
| Incompatible | Card stays visible and readable; **Preview** is disabled with the reason ("requires Plasma 6.6–6.8, this system is 6.8.0"? no — name the actual mismatch only). |
| Invalid local pack | Card with `invalid` badge and a Details link; never hidden, never silently dropped. |
| Error | `ErrorBanner`; retry. |

**Rules:** `local` and `bundled` are visually distinct but equally prominent; a local pack
is never ranked below a bundled one. No remote, featured, or trending ordering exists.

**Install a local pack:** a secondary action opens the OS directory picker through Rust
(`install_local_pack`); the UI never accepts a typed path. Outcomes:

| Result | Presentation |
| --- | --- |
| Installed | Card appears immediately with the `local` badge, focused and announced. |
| Invalid | Dialog lists field-level errors (§8.2) and states "Nothing was installed." Dialog stays open so the user can pick another directory or close it. |
| Already installed | `E_PACK_ALREADY_INSTALLED` with the installed version and a link to its Pack Detail. |
| Duplicate ID, different version | `E_PACK_ID_CONFLICT` listing both versions; no silent replacement. |

A rejected install is also recorded as a `pack_install_rejected` history event, so the
Activity feed can show it later.

**Tests:** badges, empty state, invalid pack surfaced, incompatible disable reason,
install dialog field errors and "nothing was installed" copy (AC-05, AC-18).

### 5.3 Pack Detail — `/packs/:packId`

**Data:** `get_pack_detail({ pack_id })`.

**Content order (fixed):** description → what will change (capability list) → assets
(name, type, bytes) → compatibility → license and attribution → previews → actions.

**States:** loading; not found (`E_PACK_ID_CONFLICT` must not reach here — use a plain
"pack not found" plus Diagnostics link); invalid pack (field-level errors from §8.2,
read-only); compatible; incompatible; already applied.

**Already applied:** the primary action becomes **View applied plan** → Transactions detail;
**Apply** is disabled with `E_PACK_ALREADY_APPLIED` copy: "This pack is already applied.
Roll it back before applying it again."

**Refusals are shown here, not at apply time:** if a pack requests a capability that has no
working mechanism, the request is listed under "Not supported" with the code and
explanation (§9), and **Preview** is disabled when the pack cannot be applied at all.

**Tests:** capability list completeness, override values shown, license/attribution present,
screenshots escaped, applied state (AC-06, AC-07, AC-26).

### 5.4 Preview — `/packs/:packId/preview`

**Data:** `create_pack_plan({ pack_id })` on mount; `discard_plan({ plan_id })` when leaving
without applying; `apply_plan({ plan_id, plan_hash, consent })`.

**Layout:** plan header (pack, capability count, warnings) → capability groups → asset
totals → restart notice → consent → actions. The plan hash is always visible before the
consent control, not after it.

**States**

| State | Presentation |
| --- | --- |
| Planning | "Building plan…" with no actions enabled. |
| Ready | Plan rendered; **Apply** enabled only after consent (§6). |
| Restart required | `RestartNotice` listing each affected operation and the sentence "Morphix will not restart Plasma for you." Apply remains allowed (AC-31). |
| Stale | Any precondition change: `E_PLAN_STALE` → full-width banner "The system changed after this plan was built. Nothing was written." **Apply** disabled; **Build a new plan** replaces it. |
| Expired | `E_PLAN_EXPIRED` → same treatment, copy "This plan expired. Nothing was written." |
| Refused | `E_CAPABILITY_DENIED` / `E_UNSUPPORTED_RESOURCE` → the reason, the exact resource, and "No changes were made." Apply never enabled. |
| Collision | `E_TARGET_EXISTS` → lists the colliding targets and who owns them. |
| Insufficient space | `E_INSUFFICIENT_SPACE` → required vs available bytes. |
| Applying | `ApplyProgress`; navigation away is allowed but the banner persists app-wide. |
| Result | `Outcome` (§7). |

**Rules:** Preview performs no writes. Leaving the screen discards the plan; there is no
background apply. The Apply button's disabled reason is always visible as text, never
tooltip-only.

**Tests:** grouped plan, old → new values, totals, restart notice, stale/expired disabling,
zero-write assertion (AC-09, AC-10, AC-20, AC-21, AC-31).

### 5.5 Transactions — `/transactions`

**Data:** `list_transactions({ filter, limit, offset })`, `get_transaction_detail`,
`rollback_transaction`.

**List columns:** kind (apply / rollback / restore / recovery), pack, started, finished,
outcome, operation count. Filter chips: all, applied, rolled back, failed, recovery
required. Pagination is explicit (page size 25) with a total count.

**Detail:** per-operation rows — capability, target, old → new summary, state, verified
mark. Failed rows show their code. The rollback outcome block mirrors §7, including the
snapshot path when recovery was needed.

**Actions**

| Action | Preconditions | Dialog content |
| --- | --- | --- |
| Roll back | Transaction outcome is `applied` | Scope summary (assets removed, values restored, shortcuts restored), then confirm. |
| Roll back with override | `E_ROLLBACK_CONFLICT` returned | Conflict list with later transactions and timestamps; typed confirmation phrase `OVERRIDE` required; the override is recorded in history. |
| View snapshot | Snapshot referenced by the transaction | Navigates to `/snapshots/:snapshotId`. |
| Export this transaction | Always | Writes a diagnostics bundle through `export_diagnostics`. |

**Rules:** rollback is its own recorded transaction and shows its own `Outcome`. A
conflict preview never mutates. Rollback of a non-committed transaction returns
`E_TRANSACTION_NOT_ROLLBACKABLE` and offers the snapshot instead.

**Activity feed (second view on the same screen).** `get_history` returns the append-only
event log; it is not a list of transactions and must not be merged into one. Events:
`transaction_started`, `transaction_committed`, `transaction_failed`,
`transaction_rolled_back`, `rollback_succeeded`, `rollback_failed`, `snapshot_created`,
`snapshot_restored`, `recovery_completed`, `recovery_required`, `pack_installed`,
`pack_install_rejected`, `conflict_override`.

Presentation rules:

- Events are never edited or removed; a correction appears as a later event, and the UI
  says so once at the top of the feed rather than repeating it per row.
- `recovery_required` rows are visually the most prominent entry type in the feed and link
  to the affected transaction and its snapshot.
- `conflict_override` rows name the overridden transactions; an override is never silent.
- The feed filters by event type and pack, with pagination (page size 50).

**Tests:** list/detail, rollback dialog, conflict + typed override, refusal copy,
history survives restart (AC-14, AC-15, AC-17).

### 5.6 Snapshots — `/snapshots`

**Data:** `list_snapshots`, `get_snapshot_detail`, `create_snapshot_restore_plan`.

**List:** snapshot ID (short), created, size, transaction it belongs to, resource count.
**Detail:** resource list (path or config key, with the captured value summary), and the
manifest self-hash status.

**Restore flow:** **Restore this snapshot** → `create_snapshot_restore_plan` → **the same
Preview component** → consent → apply → `Outcome`. Restoring is never a separate code path.

**States:** empty ("No snapshots yet. Each change creates one."); corrupt snapshot
(`E_SNAPSHOT_CORRUPT` / `E_SNAPSHOT_UNREADABLE`) → row marked unusable, restore disabled,
snapshot path shown for manual recovery; drift warning when current state differs from the
snapshot (informational, not blocking).

**Tests:** list/detail, restore plan preview, corrupt snapshot refusal, drift warning
(AC-16).

### 5.7 Settings — `/settings`

**Content:** data locations (config, snapshots, logs) with copy-path buttons; log path from
`get_app_info`; theme preference (system / light / dark); **Clear history** (history and
list metadata only — never snapshots or pack assets); link to Diagnostics; the
`OfflineBadge` statement.

**Rules:** no setting here changes Plasma. Any control that would is absent, not disabled.

**Tests:** paths rendered, copy buttons, theme preference, clear-history confirmation
(P8-11).

### 5.8 Diagnostics — `/diagnostics`

**Data:** `get_diagnostics`, `export_diagnostics`.

**Content:** app version and build, distro/kernel/Plasma/KF6/Qt versions, session type,
adapter probe result with each check's status, database health (`quick_check`), log path,
transaction counters (applied / rolled back / failed / recovery required).

**Export:** file dialog via the Rust side (the UI never receives a path to write). After
export: the path, a copy button, and the line "This bundle contains versions, codes and
counters. It does not contain your files, wallpapers, or theme contents."

**Tests:** counters, probe table, export success and failure (`E_IO_PERMISSION`), no file
contents in the payload (P8-10, SECURITY T-23).

---

## 6. Consent

### 6.1 Gate

**Apply** is enabled only when all hold:

1. Plan state is `ready` (not planning, stale, expired, or refused).
2. The consent checkbox is checked in this session for this plan hash.
3. `displayedPlanHash === plan.plan_hash`.
4. The system gate still reports `supported` (a reappeared root/unsupported condition
   disables it again).

### 6.2 Content

The dialog lists, in this order: capabilities granted (with descriptions), number of
operations, assets installed (count and bytes), config keys changed, shortcuts changed,
warnings, and restart requirements. The plan hash is shown in short form with a copy button
and the tooltip "Identifies this exact plan. If it changes, this dialog must be reopened."

### 6.3 Required sentence

> Applying will change the settings listed above. Morphix saves their current state first,
> so this can be undone from History. Nothing outside this list is modified.

### 6.4 Forbidden

Pre-checked consent, `Enter` as the default action inside the dialog, a "don't ask again"
option, and applying from a keyboard shortcut.

---

## 7. Outcome presentation

Four mutually exclusive results, each with a distinct heading, colour token, icon, and
`aria-live` announcement. The heading is the contract; the body must not contradict it.

| # | Condition | Heading | Required body | Colour token |
| --- | --- | --- | --- | --- |
| O-1 | Committed and verified | **Changes applied** | "N operations verified. Your previous settings are saved as a snapshot." | success |
| O-2 | Failed, `rollback.outcome == "succeeded"` | **Changes were not applied** | "Morphix undid the N completed operations and verified your previous settings are back." | warning |
| O-3 | Failed, `rollback.outcome == "not_needed"` | **Nothing was changed** | "The failure happened before any change was written." | warning |
| O-4 | Failed, `rollback.outcome == "failed"` | **Recovery required** | "Morphix could not confirm that your previous settings were restored." + affected resources + snapshot path + retry + Diagnostics link | danger, non-dismissable |

Rules:

- O-4 may not be dismissed, auto-hidden, or navigated away from into a "success" state. It
  persists as an app-wide banner until recovery reports a verified restore.
- No outcome shows a percentage as its headline; progress is per operation, not time-based.
- Partial success is never described as success: an apply whose verification failed is O-2
  or O-4 depending on rollback, never O-1.
- The body always states how many operations ran, because "0 operations" and "12 operations"
  are different user situations.

---

## 8. Error presentation

### 8.1 Structure

Every failure surface renders: code (monospace, copyable), one-sentence message, what was
not done, whether anything changed, affected resources (when any), and the log path. The
message comes from `src/errors/messages.ts`, keyed by code — not from the engine's prose.

### 8.2 Mapping — validation codes (PACK_SPEC §8)

Presented as field-level errors wherever a pack is validated: Pack Detail (read-only),
the local install dialog, and any plan attempt that re-validates.

| Group | Codes | Presentation |
| --- | --- | --- |
| Syntax and size | `E_MANIFEST_SYNTAX`, `E_MANIFEST_ENCODING`, `E_MANIFEST_TOO_LARGE`, `E_CONFIG_TOO_LARGE`, `E_UNKNOWN_FIELD`, `E_MISSING_FIELD`, `E_TYPE_MISMATCH`, `E_VALUE_INVALID`, `E_SCHEMA_VERSION_UNSUPPORTED` | Under the offending field name: `field.path` + message + expected shape. Never a dialog alone; the user must see *where*. |
| Identity | `E_PACK_ID_CONFLICT`, `E_PACK_ALREADY_INSTALLED`, `E_PACK_ALREADY_APPLIED` | Row-level on the pack card and a banner when installation was attempted. |
| Assets | `E_ASSET_MISSING`, `E_ASSET_TYPE_UNSUPPORTED`, `E_UNREFERENCED_FILE`, `E_CHECKSUM_MISMATCH`, `E_CHECKSUM_MALFORMED`, `E_CHECKSUM_INCOMPLETE` | Asset table row marked unusable with the reason. |
| Paths | `E_PATH_TRAVERSAL`, `E_SYMLINK_REJECTED`, `E_SYMLINK_TARGET` | Security wording: "This pack tries to write outside its own directory. Nothing was installed." |
| Capabilities | `E_CAPABILITY_UNKNOWN`, `E_CAPABILITY_UNDECLARED`, `E_CAPABILITY_UNUSED`, `E_CAPABILITY_DENIED`, `E_CONFIG_KEY_NOT_ALLOWED`, `E_EFFECT_NOT_ALLOWED`, `E_SHORTCUT_NOT_ALLOWED`, `E_CONFIG_TARGET_UNSUPPORTED`, `E_UNSUPPORTED_RESOURCE` | "Requests not supported" section on Pack Detail, with the specific key/effect/shortcut. |
| Compatibility | `E_INCOMPATIBLE_PACK`, `E_COMPATIBILITY_INVALID`, `E_DEPENDENCY_INVALID`, `E_DEPENDENCY_CYCLE` | Compatibility block: required range, detected version, verdict. |
| Integrity | `E_REQUIRED_FILE_MISSING`, `E_SNAPSHOT_CORRUPT`, `E_SNAPSHOT_UNREADABLE` | Resource row + snapshot path when one exists. |

### 8.3 Mapping — runtime and mutation codes (ARCHITECTURE §12.1)

| Code | Screen | Presentation and next action |
| --- | --- | --- |
| `E_UNSUPPORTED_PLATFORM` | Dashboard, Preview | Gate failure with per-check reasons; mutations disabled. |
| `E_RUNNING_AS_ROOT` | Dashboard, Preview | Root banner; mutations disabled; explanation that running as root is refused by design. |
| `E_PLAN_STALE` | Preview | "The system changed after this plan was built. Nothing was written." → build a new plan. |
| `E_PLAN_EXPIRED` | Preview | "This plan expired. Nothing was written." → rebuild. |
| `E_CONSENT_REQUIRED` | Preview | Consent gate; cannot be reached through the UI, so this indicates a bug: show it with a Diagnostics link. |
| `E_BUSY` | Any mutation | "Another Morphix window is making changes." Retry after it finishes. |
| `E_INSUFFICIENT_SPACE` | Preview | Required vs available bytes; no override button. |
| `E_TARGET_EXISTS` | Preview | Collision list with owner; no override. |
| `E_SYMLINK_TARGET` | Preview | "A destination is a symlink. Morphix will not write through it." |
| `E_KEY_IMMUTABLE` | Preview | Key is locked by KDE; listed under "Cannot be changed". |
| `E_APPLY_FAILED` | Outcome | Always rendered with the rollback outcome; the code alone never becomes the headline. |
| `E_VERIFY_FAILED` | Outcome | Post-state did not match the plan; rollback runs. |
| `E_ROLLBACK_CONFLICT` | Transactions | Conflict list, later transactions, typed override. |
| `E_ROLLBACK_FAILED` | Outcome (O-4) | Recovery required: affected resources, snapshot path, retry. |
| `E_TRANSACTION_NOT_ROLLBACKABLE` | Transactions | "Only a committed change can be rolled back." Offer snapshot restore. |
| `E_RECOVERY_REQUIRED` | App-wide banner | Blocks new mutations until recovery completes; links to the affected transaction. |
| `E_ADAPTER_UNAVAILABLE` | Dashboard, Preview | Capability marked unavailable; no retry loop. |
| `E_SCRIPT_FAILED` | Outcome | Plasma rejected a generated script; includes the failing step name, never the script text. |
| `E_CONFIG_WRITE_RACE` | Outcome | "Another program kept overwriting this setting." → retry path; rollback already applied. |
| `E_PERSIST_TIMEOUT` | Outcome | "Plasma did not save the change in time." Verification is incomplete → O-2/O-4 path. |
| `E_RESIDUE_CLEANUP_FAILED` | Outcome | Names the leftover section; manual cleanup instructions; never silently ignored. |
| `E_RESTART_REQUIRED` | Preview | Informational notice, not an error banner (§5.4). |
| `E_PANEL_OPACITY_UNSUPPORTED` | Pack Detail | Under "Not supported" (§9). |
| `E_CURSOR_SIZE_UNSUPPORTED` | Pack Detail | Under "Not supported" (§9). |
| `E_IO`, `E_IO_PERMISSION` | Any | Target path + permission state; suggests Diagnostics. |
| `E_DB_CORRUPT`, `E_DB_NEWER_THAN_APP` | App-wide | Blocks mutations; offers export + reset of history (never snapshots). |
| `E_INTERNAL` | Any | "Morphix hit an internal error and stopped." Includes code and log path; never claims nothing changed unless the report proves it. |

**Unmapped code:** renders code, message from the engine, and the log path, with **no**
claim about the system state. A test asserts the map covers every registry code, so this
path exists only until the registry changes.

---

## 9. Refusals

Three capabilities have no working mechanism on the validated matrix. The UI must present
them as unavailable by design, not as failures and not as approximations.

| Capability | Code | Where it appears | Copy |
| --- | --- | --- | --- |
| Panel opacity | `E_PANEL_OPACITY_UNSUPPORTED` | Pack Detail "Not supported"; Preview warning | "Morphix cannot set panel opacity on Plasma 6.8. This part of the pack is not applied; nothing is approximated." |
| Cursor size | `E_CURSOR_SIZE_UNSUPPORTED` | Same | "Morphix can install and select a cursor theme, but cannot change the cursor size. The size in this pack is ignored." |
| Live icon switching | `E_RESTART_REQUIRED` class | Plan entry apply class | "This change takes effect after a Plasma restart. Morphix will not restart it for you." |

If a pack consists only of refused capabilities, **Preview** is disabled and the reason is
the refusal, not "invalid pack".

---

## 10. Restart and reload policy

- Every plan entry carries an apply class: `live`, `restart_required`, `manual_step`.
- `restart_required` entries are grouped in a `RestartNotice` **before** consent, with the
  exact sentence that Morphix will not restart Plasma and the command the user can run.
- The Outcome view for a plan containing `restart_required` entries says "Applied. N changes
  are waiting for a Plasma restart." and does not claim they are visible yet.
- `manual_step` entries (if any remain) list the user action verbatim; the UI never implies
  Morphix performed it.

---

## 11. Progress and long operations

- Events: `morphix://transaction-progress`, `morphix://transaction-finished`,
  `morphix://recovery-required`.
- Progress renders operation index / total plus a completed-operations log. No estimated
  time remaining.
- If events stop arriving for longer than the adapter timeout, the UI polls
  `get_transaction_detail` once and shows "Waiting for Morphix to report progress" — it
  never assumes failure.
- The app-wide banner survives navigation while a transaction is running, because closing a
  route must not look like cancelling.

---

## 12. Accessibility and visual rules

- Every state has a text label; colour is never the only signal (badges, outcomes, rows).
- Focus: dialogs trap focus and restore it on close; the Outcome heading receives focus when
  it replaces the progress view.
- `aria-live="polite"` for progress; `role="alert"` for `RecoveryBanner` and O-4.
- Keyboard: full discover → detail → preview → consent → apply → outcome path without a
  pointer; Escape cancels dialogs; no action is bound to a bare `Enter` in a destructive
  context.
- Contrast: body text ≥ 4.5:1, large text ≥ 3:1, focus ring ≥ 3:1, in both light and dark.
- Transparency: the window and navigation are translucent (default ~88% opacity, never
  below 85%) with a backdrop blur. Contrast is judged against the worst-case composited
  surface, not the unblended token; `prefers-reduced-transparency: reduce` restores opaque
  surfaces. Transparency is decorative and never the only separation between regions.
- Motion: no animated transitions over 150 ms; `prefers-reduced-motion` respected.
- Text: pack names and descriptions truncate with a title attribute rather than expanding
  layout; no text is rendered in the accent colour alone.

---

## 13. Design tokens (contract)

Tokens live in `src/styles/tokens.css` and are the only colour/spacing source. Semantic
names, not colours, are used by components:

```css
--surface, --surface-raised, --surface-sunken
--surface-glass, --surface-glass-raised      /* translucent window surfaces, ≥85% alpha */
--text, --text-muted, --text-inverse
--border, --border-strong
--accent, --accent-text
--success, --warning, --danger, --info        /* state tokens */
--focus-ring
--space-1 … --space-8                          /* 4px base */
--radius-sm, --radius-md, --radius-lg
--font-ui, --font-mono                        /* mono for codes and hashes */
```

Light and dark are defined through `prefers-color-scheme`, with an explicit override in
Settings. A component that hardcodes a hex value is a review failure. Translucency values
live only in `--surface-glass*`; components never set their own alpha or blur.

---

## 14. Traceability

| Screen | Acceptance criteria | UI tasks |
| --- | --- | --- |
| Dashboard | AC-01, AC-02, AC-03, AC-04, AC-17, AC-23 | P8-02 |
| Discover | AC-05, AC-18 | P8-03 |
| Pack Detail | AC-06, AC-07, AC-19, AC-26 | P8-04 |
| Preview | AC-08, AC-09, AC-10, AC-11, AC-12, AC-13, AC-20, AC-21, AC-24, AC-27, AC-28, AC-29, AC-31, AC-32 | P8-05 |
| Outcome | AC-12, AC-13, AC-23 | P8-06 |
| Transactions | AC-14, AC-15, AC-17 | P8-07 |
| Discover (local install) | AC-18 | P8-09 |
| Snapshots | AC-11, AC-16 | P8-08 |
| Settings | AC-25 (local-first statement) | P8-11 |
| Diagnostics | AC-23, T-23 (redaction) | P8-10 |
| All | AC-22 (no path to privilege in the UI), AC-30 (global-theme plans) | P8-13 |

Every row in §8 must have a component test; TESTING §4.9 lists the flows that must exist.

---

## 15. Open questions

| # | Question | Blocks |
| --- | --- | --- |
| Q-1 | Should the Outcome view block navigation until the user acknowledges O-4, or persist as a banner with navigation allowed? (Current spec: banner persists, navigation allowed.) | P8-06 |
| Q-2 | For a `restart_required`-only plan, is Apply allowed before the restart, or deferred until after it? (Current spec: allowed; notice shown.) | P8-05, AC-31 |
| Q-3 | Does the Preview screen keep the plan alive across a window reload (rehydrate by `plan_id`), or rebuild on load? (Current spec: rebuild; discard on leave.) | P8-05 |
| Q-4 | Should the transactions list show recovery-required entries in the failure filter, or a separate filter? | P8-07 |

None of these change a safety property; they change workflow feel. Resolve before Phase 8
starts, in an ADR if the answer is not obvious.
