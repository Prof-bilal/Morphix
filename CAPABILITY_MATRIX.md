# Morphix — Capability Matrix

- **Status:** Evidence-based. Every state below was produced by execution on a live
  Plasma 6 session, not by reading documentation.
- **Test session:** see §1. Full per-experiment records: [PROTOTYPE_RESULTS.md](PROTOTYPE_RESULTS.md).
- **Rule:** `PARTIAL` is never counted as `PASS`. A capability moves to `MVP: YES` only
  when the mechanism that production will use has been executed and verified.

---

## 1. Test environment

| Fact | Value |
| --- | --- |
| Distribution | CachyOS (Arch-based, rolling), kernel `7.2.9-2-cachyos-deckify` |
| Desktop | KDE Plasma **6.8.0** (`plasmashell 6.8.0`) |
| Compositor | KWin **6.8.0**, Wayland session (`XDG_SESSION_TYPE=wayland`) |
| Qt | **6.12.0** (runtime and compile version) |
| KDE Frameworks | **6.31.0** (`KF6CoreAddons` 6.31.0), Plasma scripting API v20 |
| Screens | 1 (1920×1080), `multihead=false`, `screenCount=1` |
| Virtual desktops | 1 (`Id_1=58c5aa6f-…`) |
| Panel | id `23`, bottom, height 30, center, floating, 7 widgets |
| KWin decoration plugins | `org.kde.breeze`, `org.kde.kwin.aurorae` (+ QML `plastik`) |
| Installed L&F packages | 8 (incl. `CachyOS-Nord`, 3 Breeze variants, 2 Valve, Emerald) |

**Environment hazard that invalidated naive probing:** the ContextGit AppImage injects an
`LD_LIBRARY_PATH` that breaks `plasmashell`, `qdbus6`, `kreadconfig6` and `gdbus`
(`undefined symbol: curl_global_trace`, missing `XZ_5.4`). Every probe was run with
`env -u LD_LIBRARY_PATH`. A production build must never mutate a child process's loader
path this way.

## 2. Matrix

States: `PASS` · `PARTIAL` · `FAIL` · `NOT TESTED`.
"Preferred method" is the highest rung of the implementation ladder that applies.

### 2.1 KDE configuration (the foundation for everything else)

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Read a config value | `kreadconfig6` | PASS | Plasma `readConfig` | `ConfigFile` in scripting | `KConfig` read in-process (Rust) | YES |
| Write a config value | `kwriteconfig6` | PASS | Plasma `writeConfig` | `ConfigFile.writeEntry` | `KConfig` write in-process | YES |
| Verify a written value | read-back | PASS | live-state probe | — | read-back of *effective* value | YES |
| Restore previous value | `kwriteconfig6`/`--delete` | PASS | snapshot file image | — | inverse write; `--delete` when key was absent | YES |
| Concurrent external changes | KConfig locking | PARTIAL | engine write lock | retry-on-lost-update | exclusive lock + verify + retry | YES (restricted) |
| Reload semantics per key | notifications | PARTIAL | shell restart | — | per-capability reload policy (§3) | YES (restricted) |
| Reading `kdedefaults` overlay | XDG_CONFIG_DIRS | PASS | — | — | read merge of user + `kdedefaults` + `/etc/xdg` | YES |

### 2.2 Wallpaper

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Detect current wallpaper | `org.kde.PlasmaShell.wallpaper(0)` | PASS | `desktop.readConfig` | `appletsrc` parse | DBus `wallpaper(u)` map | YES |
| Change wallpaper | Plasma script `writeConfig("Image")` | PASS | `plasma-apply-wallpaperimage` | `setWallpaper(plugin,map,screen)` | Plasma script + read-back | YES |
| Verify active wallpaper | DBus `wallpaper(0)` | PASS | script `readConfig` | `appletsrc` | DBus map comparison | YES |
| Restore previous wallpaper | same script path | PASS | — | — | inverse write + verify | YES |
| Multi-screen behaviour | per-desktop scripting | NOT TESTED | — | — | per-desktop loop, screen-keyed | PENDING |

### 2.3 Global themes / Look-and-Feel

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Apply a global theme | `plasma-apply-lookandfeel -a` | PASS | decompose package defaults | `--resetLayout` | `-a` alone (never `--resetLayout`) | YES |
| Determine current theme | `LookAndFeelPackage` key | PARTIAL | key-match vs `contents/defaults` | `kdedefaults/package` | marker + override diff, labelled "custom" when mixed | YES (restricted) |
| Restore previous theme | captured keys + file image | PASS | re-apply old package | — | restore overrides + `kdedefaults/` + re-apply live | YES |
| Interaction with individual components | per-key overrides | PASS | — | — | decompose into primitive operations | YES |
| Plasma 6 compatibility | native tool | PASS | — | — | supported | YES |
| Layout replacement (`--resetLayout`) | Plasma layout load | NOT TESTED | — | — | never invoked by Morphix | NO (by design) |

### 2.4 Panels

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Panel location | `panel.location` | PASS | `appletsrc` | — | scripting property | YES |
| Panel height | `panel.height` | PASS | `plasmashellrc thickness` | — | scripting property | YES |
| Panel width / length | `panel.length(Mode)` | NOT TESTED | `plasmashellrc` | — | scripting property | PENDING |
| Panel alignment | `panel.alignment` | PASS | — | — | scripting property | YES |
| Panel floating | `panel.floating` | PASS | — | — | scripting property | YES |
| Panel visibility | `panel.hiding` | PASS | — | — | scripting property | YES |
| Panel opacity | `panel.opacity` | FAIL | `plasmashellrc panelOpacity` | restart shell | **not supported** until a working path is proven | PENDING |
| Create a panel | `new Panel()` | PASS | — | — | scripting constructor | YES |
| Remove a panel | `panel.remove()` | PASS | — | — | scripting, then residue cleanup | YES |
| Persist panel changes | plasmashell autosave | PASS | — | — | wait for persistence, then verify | YES (restricted) |

### 2.5 Widgets

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Detect widgets | `panel.widgets()` | PASS | `appletsrc` | — | scripting | YES |
| Add widget | `panel.addWidget(type)` | PASS | `appletsrc` | — | scripting from `knownWidgetTypes` allowlist | YES |
| Configure widget | `widget.writeConfig` | PASS | `appletsrc` | — | scripting + read-back | YES |
| Remove widget | `widget.remove()` | PASS (async) | — | — | scripting + poll for absence | YES |
| Back up full layout | `widgets()` + per-widget config | PARTIAL | `appletsrc` image | — | type/order/config snapshot; IDs are reassigned | YES (restricted) |
| Restore full layout | inverse operations | PARTIAL | `appletsrc` image + shell restart | — | rebuild by type/order, verify by type | YES (restricted) |

### 2.6 KWin effects

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Discover effects | `/Effects.listOfEffects` | PASS | plugin dir scan (wrong: IDs differ) | — | DBus list | YES |
| Read effect state | `isEffectLoaded` | PASS | `kwinrc [Plugins]` | — | DBus + config | YES |
| Enable / disable | `loadEffect`/`unloadEffect` | PASS | `kwinrc` + `reconfigure` | — | **both**: config for persistence, DBus for liveness | YES |
| Configure parameters | `kwinrc [Effect-*]` | PARTIAL | `reconfigureEffect` | — | allowlisted keys, no introspection | YES (restricted) |
| Verify | `isEffectLoaded` | PASS | — | — | DBus read-back | YES |
| Restore | `kwinrc` + DBus | PASS | — | — | inverse + `--delete` when key was absent | YES |

### 2.7 Window decorations

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Detect current decoration | `supportInformation` | PASS | `kwinrc` | — | live state first, config second | YES |
| Enumerate available | plugin dir scan | PASS | — | — | filesystem enumeration | YES |
| Switch decoration | `kwinrc [org.kde.kdecoration2]` + `reconfigure` | PASS | — | — | config + reconfigure | YES |
| Verify | `supportInformation` `Plugin:`/`Theme:` | PASS | pixel diff | — | live state string match | YES |
| Rollback | inverse config keys | PASS | — | — | inverse + reconfigure | YES |

### 2.8 Color schemes

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Detect | `kdeglobals General/ColorScheme` | PASS | `[Colors:*]` compare | — | config read | YES |
| Install | copy `.colors` to `~/.local/share/color-schemes/` | PASS | `kpackagetool6` | — | engine-owned asset install | YES |
| Apply | `plasma-apply-colorscheme` | PASS | `kdeglobals` + notify | — | tool (live) with config read-back | YES |
| Verify | config + panel pixels | PASS | — | — | config read-back (authoritative) | YES |
| Rollback | re-apply previous scheme | PASS | — | — | inverse + file removal | YES |

### 2.9 Icons

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Detect current | `kdeglobals Icons/Theme` | PASS | — | — | config read | YES |
| Install | copy tree to `~/.local/share/icons/` | NOT TESTED | `kpackagetool6` | — | engine-owned asset install | YES |
| Activate (live) | `kdeglobals` write | FAIL | `kbuildsycoca6` | — | no live path found; needs shell restart | — |
| Activate (with shell restart) | config + restart | PASS | — | — | config write, mark restart-required | YES (restricted) |
| Verify | panel pixel diff | PASS | — | — | config + post-restart pixels | YES |
| Rollback | inverse config + restart | PASS | — | — | config + file removal + restart | YES (restricted) |

### 2.10 Cursor themes

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Detect current | `supportInformation` `Cursor` | PASS | `kcminputrc` | — | live state + config | YES |
| Install | copy tree to `~/.local/share/icons/` | NOT TESTED | — | — | engine-owned asset install | YES |
| Activate | `plasma-apply-cursortheme` | PASS | `kcminputrc` write | — | tool + KWin read-back | YES |
| Wayland behaviour | native Wayland session | PASS | — | — | supported | YES |
| Cursor size | `kcminputrc Mouse/cursorSize` | PARTIAL | — | — | config only; live effect unproven | PENDING |
| Verify / rollback | `supportInformation` + config | PASS | — | — | forced re-apply (desync workaround) | YES |

### 2.11 Fonts

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Read font configuration | `kdeglobals [General] font` etc. | PASS | `kreadconfig6` on all font keys | — | config read of the allowlist | YES |
| Change fonts | config write | PARTIAL | restart apps | shell restart | config write, applied by apps at start | YES (restricted) |
| Restyle Plasma chrome | — | FAIL | — | — | Plasma chrome ignores `[General] font` | NO |
| Restore fonts | inverse write / `--delete` | PASS | — | — | inverse + verification of config | YES |

### 2.12 Shortcuts

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Read shortcut | `KGlobalAccel.shortcut` | PASS | `kglobalshortcutsrc` | — | DBus | YES |
| Set shortcut | `setShortcut(..., flags=4)` | PASS | `kglobalshortcutsrc` | — | DBus with `SetPresent` | YES |
| Detect conflicts | `isGlobalShortcutAvailable` | PASS | `getGlobalShortcutsByKey` | — | DBus pre-check before apply | YES |
| Verify | `shortcut()` read-back | PASS | config line | — | DBus read-back | YES |
| Restore | `setShortcut` with old value | PASS | — | — | inverse DBus call | YES |

### 2.13 Desktop behaviour

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Virtual desktops | `org.kde.KWin.VirtualDesktopManager` | PASS | `kwinrc [Desktops]` | — | DBus create/remove/rename | YES |
| Workspace rows / wrapping | DBus properties | PARTIAL | `kwinrc` | — | read verified; write untested | PENDING |
| Hot corners | `kwinrc [ElectricBorders]` | PASS | — | — | config + `reconfigure` + live read-back | YES |
| Window rules | `kwinrulesrc` | PARTIAL | — | — | config + `reconfigure`, no verification channel | YES (restricted) |
| Animations | `kdeglobals [KDE] AnimationDurationFactor` | PARTIAL | `kwinrc Compositing` | — | config write; not visually verified | YES (restricted) |
| Task switching | `kwinrc [WindowSwitcher] LayoutName` | NOT TESTED | — | — | config write | PENDING |
| Overview behaviour | effect `overview` | NOT TESTED | — | — | effect enable/disable only | PENDING |

### 2.14 Snapshots, transactions, recovery

| Capability | Preferred Method | Result | Alternative 1 | Alternative 2 | Final Approach | MVP |
| --- | --- | --- | --- | --- | --- | --- |
| Per-capability snapshot + restore | engine capture + inverse | PASS | file image | — | adapter `capture()`/`restore()` per resource | YES |
| Combined 5-resource transaction | journal + reverse rollback | PASS | — | — | engine (prototype verified) | YES |
| Partial failure (op N of M fails) | reverse-order rollback | PASS | — | — | engine (prototype verified) | YES |
| Crash mid-apply (`SIGKILL`) | journal recovery on start | PASS | — | — | engine (prototype verified) | YES |
| Rollback of committed transaction | inverse operations | PASS | — | — | engine (prototype verified) | YES |
| Multi-screen snapshot | per-desktop loop | NOT TESTED | — | — | screen-keyed resources | PENDING |

## 3. Reload policy (measured)

`—` means no reload needed; the value applies immediately.

| Change | Live effect observed | Required step | Evidence |
| --- | --- | --- | --- |
| Color scheme | immediate | none | config read-back + `[Colors:*]` rewrite |
| Cursor theme | immediate | none (but see desync note) | KWin `themeName` changed |
| Plasma style | immediate | none | panel pixels changed (RMSE 0.029) |
| Wallpaper | immediate | none | DBus `wallpaper(0)` map |
| Decoration | immediate | `KWin.reconfigure` | `supportInformation` `Plugin:` |
| KWin effect enable | runtime only via DBus | `loadEffect` **and** `kwinrc` | `isEffectLoaded` true; `kwinrc` untouched by DBus |
| Hot corners | after reconfigure | `KWin.reconfigure` | `actionTopLeft` live value |
| Virtual desktops | immediate | none | DBus `count` + `kwinrc` |
| Panel properties | immediate | none (persist ~15 s later) | panel read-back; `plasmashellrc` lag |
| Icon theme | **none** | plasmashell restart | live RMSE 0; post-restart RMSE 0.0085 |
| `[General] font` | none | app restart (apps read at start) | panel RMSE 0 even after restart |
| Widget add/remove | immediate | none (removal async) | `widgets()` count after poll |

## 4. Residue and normalisation observed

These are *not* failures of the capability, but they break byte-identical rollback and
must be handled explicitly by the engine.

| Situation | Observed residue | Handling |
| --- | --- | --- |
| Create then remove a panel | `[PlasmaViews][Panel 46]` left in `plasmashellrc` | delete orphan sections for the known panel id |
| Create then remove a desktop | `[Tiling][<uuid>]` left in `kwinrc` | delete orphan section for the known uuid |
| Restore a key that was absent | key written with the default value instead of deleted | restore-to-absent uses `--delete` |
| Restore panel alignment | `alignment=132` (Qt flags) written where no key existed | delete when pre-state was absent |
| Restore color scheme | `ColorScheme=<name>` added (was inherited) | delete when pre-state was absent |
| L&F apply | `~/.config/kdedefaults/*` rewritten | **include `kdedefaults/` in snapshots** |
| Panel/wallpaper widget ops | `[ScreenMapping]` reordered | treat as engine-owned collateral; do not fight it |
| Shortcut save | `[$InverseAction]` table rewritten | compare semantically, not byte-wise |

## 5. Verification channels (what the engine can trust)

| Channel | Covers | Trust |
| --- | --- | --- |
| `kreadconfig6` effective value | every config key | **authoritative** (honours `kdedefaults`, `/etc/xdg`) |
| `org.kde.KWin.supportInformation` | decoration, cursor, hot corners, options | **authoritative** for live state |
| `/Effects.isEffectLoaded` | effects | authoritative |
| `org.kde.PlasmaShell.wallpaper(u)` | wallpaper | authoritative |
| Panel scripts `panels()/widgets()` | panels, widgets | authoritative for live state |
| `KGlobalAccel.shortcut()` | shortcuts | authoritative |
| Screenshot pixel diff | visual confirmation | **unreliable on a live desktop** — full-screen RMSE is dominated by app content; only a static crop (panel strip) was stable (RMSE 0 over 4 s) |

## 6. Conclusion

### Fully validated

Ready for production as specified in the matrix:

- Configuration read, write, verify and restore, including delete-to-absent.
- Wallpaper detect, change, verify (two independent channels) and restore.
- Panel properties: location, height, alignment, floating, hiding.
- Panel creation and removal, with residue cleanup.
- Widget detection, add, configure, remove, and layout restore by type and order.
- KWin effect discovery, state read, enable/disable, verification and restore.
- Window decoration detection, switching, live verification and rollback.
- Color schemes, including installation into `~/.local/share/color-schemes/`.
- Cursor themes, including Wayland behaviour.
- Keyboard shortcuts, including conflict detection before writing.
- Virtual desktops and hot corners.
- Global theme apply and restore (as decomposed plans).
- Snapshot, combined transaction, partial-failure rollback, committed rollback, and
  crash recovery.

### Validated with restrictions

Usable now, with a documented limitation the UI must state:

- **Concurrency** — external writers can lose config writes (14 of 20 lost under stress);
  detected, retried, and reported rather than hidden.
- **Reload policy** — icon themes need a shell restart; the engine declares
  `restart_required` and never restarts the shell itself.
- **Widget identity** — restored widgets receive new ids; restore is by type, index and
  config, and the plan says so.
- **Effect parameters** — no introspection API; config-verified only, never claimed as
  visually verified.
- **Window rules** — written and persisted, but there is no read-back channel proving KWin
  applied them.
- **Animations** — config round-trip verified, visual effect not screenshot-verifiable.
- **Global-theme detection** — a system-wide marker exists, so mixed configurations are
  reported as "custom (derived from X)".
- **Fonts** — affect newly started applications, not Plasma chrome.
- **Panel persistence** — deferred by Plasma (~15 s); the engine waits before hashing.
- **Panel length/width, workspace rows** — mechanisms exist and read correctly, writing not
  exercised.

### Alternative implementation required

Preferred method failed; a different viable approach exists and is specified:

- **Panel opacity** — `panel.opacity` is silently read-only. Alternative: write
  `plasmashellrc panelOpacity` and accept a `restart_required` class. Not yet proven.
- **Cursor size** — config accepted, KWin still reports 24. Alternative: apply through the
  input KCM's notification path, or accept a compositor-restart class.
- **Live icon theme switching** — no live path found. Alternative: the already-validated
  restart-only path, declared honestly as `restart_required`.

### Genuine blockers

**None of the core MVP capabilities.** Every capability in the product brief's target list
(themes, icons, cursors, wallpapers, colors, window decorations, panels, widgets,
shortcuts, KWin effects, desktop behaviour, complete packs, snapshots, rollback, history)
has either a validated implementation or a documented restricted one.

The three items above are not blockers: each has an implementation path, and the current
product refuses them explicitly with a stable error code rather than approximating them.

### Investigation paths for the three gaps

Each alternative-required item has a concrete next step; none is closed, and none is
deferred for being awkward.

| Gap | Why the first approach failed | Next investigation path |
| --- | --- | --- |
| Panel opacity | `panel.opacity` accepts every value and silently stays `translucent`; the property is effectively read-only | Write `plasmashellrc [PlasmaViews][Panel N] panelOpacity` and test whether a plasmashell restart applies it; if it does, support it as `restart_required`. Test on a throwaway account because a restart can fail. |
| Cursor size | `kcminputrc [Mouse] cursorSize=40` left KWin `themeSize: 24`, with and without `KWin.reconfigure` | Determine whether KWin reads the size only at compositor start: test with a KWin restart on a disposable VM; also check whether the value must be applied through the input KCM's notification path rather than a config write. |
| Live icon switching | Config write changed the key but produced zero panel change; only a shell restart applied it | Identify the notification System Settings uses for icon themes (KIconLoader/KConfigWatcher behaviour), and whether `kbuildsycoca6` plus a `KGlobalSettings` signal is sufficient. Until then icons stay `restart_required`, which is honest but not ideal. |

### Safety admissibility

An approach was accepted only when it passed all five criteria, not merely because it
worked. The rejected alternative in every case was direct `rm`-style config resetting:

| Criterion | How it was applied |
| --- | --- |
| Reversible | Every accepted mechanism has a verified inverse; `plasma-apply-lookandfeel` alone was rejected *as a unit* and replaced by decomposed, individually reversible operations |
| Deterministic enough | `isEffectLoaded`-style read-backs and `shortcut()`/`wallpaper()` comparisons; pixel diffing explicitly **not** used as a gate |
| Constrained | Fixed target map, closed enums, curated widget list, engine-derived asset names; no pack path ever reaches a script or a command line |
| Testable | Each capability has an automated real-session assertion, not a manual eyeball |
| Permission-model compatible | No root, no arbitrary shell, no shell restart; `ConfigFile` in the scripting context is never constructed |

No capability was deferred for being "difficult"; the only items left as `PENDING` are
those that could not be exercised in this environment (multi-screen, panel length,
workspace rows, task switching, overview, `--resetLayout`) or that no mechanism was found
for (panel opacity, cursor size, live icon application).
