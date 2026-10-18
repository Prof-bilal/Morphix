# Morphix — Prototype Results

- **Status:** Executed on a live KDE Plasma 6.8 session on 2026-10-18.
- **Companion:** [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md) (summary).
- **Honesty rule:** anything not executed is marked `NOT TESTED`. No result below is
  inferred from documentation.

---

## 1. How to read this document

Each experiment records what was being proven, the environment, the preferred approach
attempted first, the exact commands or APIs, the result, the observed output, the
problems found, the alternatives considered, and what production should use.

### 1.1 The implementation ladder actually followed

Alternatives were investigated in the requested priority order; each capability stopped at
the highest rung that worked.

| Rung | Used for | Outcome |
| --- | --- | --- |
| 1. Official KDE/Plasma API | wallpaper read (`wallpaper(u)`), virtual desktops, effects state, `supportInformation` | adopted |
| 2. DBus interface | shortcuts (`kglobalaccel`), effects (`/Effects`), desktops (`VirtualDesktopManager`) | adopted |
| 3. Plasma scripting | wallpaper write, panel properties, widgets, panel creation | adopted (fixed templates only) |
| 4. KConfig / KDE utilities | theme keys, hot corners, animations, decorations, `plasma-apply-*` | adopted, with read-back |
| 5. Supported command-line utility | `plasma-apply-colorscheme`, `-cursortheme`, `-desktoptheme` | adopted (fixed argv, enum values) |
| 6. Carefully controlled configuration mutation | effect parameters, window rules, font keys | adopted with `StateVerified` only |
| 7. Application-owned compatibility layer | none needed | not reached |

Rung 7 was never required, which is the outcome the brief wanted. Direct destructive
configuration resetting ("`rm -rf ~/.config`") was never used, and no approach was accepted
on the grounds that it merely worked.

## 2. Shared environment

| Fact | Value |
| --- | --- |
| Distribution | CachyOS (Arch-based, rolling), kernel `7.2.9-2-cachyos-deckify` |
| Plasma | `plasmashell 6.8.0`, scripting API `20` |
| KWin | `6.8.0`, Wayland, `Operation Mode: Wayland` |
| Qt | `6.12.0` |
| KDE Frameworks | `6.31.0` (`libKF6CoreAddons.so.6.31.0`); Plasma scripting API `20` |
| Display | 1920×1080, single screen, `screenCount=1`, `multihead=false` |
| Panel | id `23`, bottom, 30 px, center, floating, `hiding=none`, `opacity=translucent` |
| Session bus | `unix:path=/run/user/1000/bus` |

### 2.1 Environment defects encountered (and worked around)

1. The ContextGit AppImage exports an `LD_LIBRARY_PATH` that breaks KDE tooling:
   `plasmashell: /tmp/.mount_…/usr/lib/liblzma.so.5: version 'XZ_5.4' not found`, and
   `gdbus: symbol lookup error: undefined symbol: g_string_free_and_steal`,
   `git-remote-https: undefined symbol: curl_global_trace`.
   All probes ran with `env -u LD_LIBRARY_PATH`.
2. `XDG_CONFIG_DIRS` includes `~/.config/kdedefaults`, so `kreadconfig6` reports *merged*
   values. A key absent from `~/.config/<file>` can still have an effective value.
3. `/etc/xdg/kdeglobals` ships `LookAndFeelPackage=com.valve.vapor.desktop`, i.e. a
   system-wide global-theme marker exists even when the user never chose one.

---

## 3. KDE configuration core

### Goal
Prove that configuration can be read, written, verified, restored, and deleted safely,
and that the reload behaviour is understood.

### Environment

CachyOS config tree under `~/.config`; `XDG_CONFIG_DIRS` includes `~/.config/kdedefaults`; `/etc/xdg/kdeglobals` supplies a system-wide `LookAndFeelPackage`. Tools: `kreadconfig6`, `kwriteconfig6`, Plasma `ConfigFile` (scripting API 20).

### Preferred approach
`kreadconfig6` / `kwriteconfig6` (the KConfig command-line tools used by KDE's own
settings modules), plus the Plasma scripting `ConfigFile` object as an alternative.

### Commands
```bash
kreadconfig6 --file kdeglobals --group Icons --key Theme --default __UNSET__
kwriteconfig6 --file kdeglobals --group Icons --key Theme breeze
kwriteconfig6 --file kdeglobals --group Icons --key Theme --delete
qdbus6 org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.evaluateScript \
  'var c = new ConfigFile("/home/u/.config/kdeglobals", false);
   c.group = "General"; c.deleteEntry("Probe");'
```

### Result
`PASS`

### Evidence
- Read/write/delete round-trip on `kdeglobals`, `kwinrc`, `kcminputrc`, `kwinrulesrc`,
  `kglobalshortcutsrc`, `plasmarc`, `plasmashellrc` all succeeded.
- `ConfigFile` round-trip: `k1=v1 k2=v2` → after `deleteEntry("k2")` →
  `keys=["k1"]`, and reopen returned `k1=v1`.
- Restoring a key that was absent, using `--delete`, produced a byte-identical `kwinrc`
  (`diff -q` clean).

### Problems
1. **Concurrent writers lose updates.** Twenty parallel `kwriteconfig6` calls to the same
   file produced only **6 of 20** keys:
   `[MorphixConcurrency] k14 k16 k17 k18 k4 k6`. Sequential and 5-way parallel writes
   were all preserved, so the failure is a read-modify-write race at higher contention,
   not a malformed file: the file stayed parseable and `General/ColorScheme` remained
   readable.
2. **`ConfigFile.group` must be a string.** `c.group = ["Alpha"]` silently created a group
   named `[A]` (first character); `c.group = "Alpha"` created `[Alpha]`. This cost one
   debugging cycle and polluted `kdeglobals` with `MorphixProbe=x` until cleaned with
   `--delete`.
3. **`ConfigFile` has no `sync()`** and its methods live on the file object, not on
   `configFile.group` (a `ConfigGroup` exposing only array indices). Writing via the
   group object throws `TypeError: Property 'sync' … is not a function`.
4. **`kwriteconfig6` has no `--delete-group`** (`Unknown option 'delete-group'`), so
   removing a whole group requires an in-engine KConfig writer.

### Alternative approaches
1. `kreadconfig6`/`kwriteconfig6` — works, no in-process control of locking.
2. Plasma `ConfigFile` — works, but runs inside plasmashell (wrong process for
   journaling).
3. In-process KConfig parser/writer (planned in `desktop::kde::kconfig`) — needed anyway
   for atomic writes and group deletion.

### Final recommendation
Write configuration in-process with a KConfig-compatible implementation, under the
engine's exclusive mutation lock, then **read back and compare**. Treat a mismatch as a
lost update: retry with a bounded backoff, and after N attempts fail with
`E_CONFIG_WRITE_RACE` rather than reporting success. Never assume that a successful
`kwriteconfig6` exit means the value is present.

---

## 4. Wallpaper

### Goal
Detect, change, verify, and restore the wallpaper with a reliable read-back.

### Environment

One virtual desktop with containment id 43, wallpaper plugin `org.kde.image`, image `/usr/share/wallpapers/Steam Deck Logo Default.jpg`; `/usr/share/plasma/wallpapers/` ships 7 plugins.

### Preferred approach
The dedicated DBus method `org.kde.PlasmaShell.wallpaper(u) -> a{sv}` for reading, and the
Plasma scripting API for writing.

### Commands
```javascript
var d = desktops()[0];
d.currentConfigGroup = ["Wallpaper", "org.kde.image", "General"];
print(d.readConfig("Image", "UNSET"));        // detect
d.wallpaperPlugin = "org.kde.image";
d.writeConfig("Image", "/usr/share/wallpapers/cachyos-wallpapers/north.png");
d.reloadConfig();
```
```bash
qdbus6 org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.wallpaper 0
```

### Result
`PASS`

### Evidence
- Detect: `Image: /usr/share/wallpapers/Steam Deck Logo Default.jpg`.
- After write, DBus read-back: `Image: /usr/share/wallpapers/cachyos-wallpapers/north.png`;
  scripting read-back agreed.
- Restore: DBus read-back returned `Steam Deck Logo Default.jpg`, and
  `plasma-org.kde.plasma.desktop-appletsrc` contained it again.
- The `wallpaper(u)` map also exposes `FillMode`, `Blur`, `Color`, `SlidePaths`, so fill
  mode and blur can be snapshotted and restored as first-class values.

### Problems
1. `d.readConfig("org.kde.image", "Image")` is the **wrong signature**: it takes
   `(key, defaultValue)` and reads from `currentConfigGroup`. The wrong call returned
   the literal string `"Image"` and wrote a stray `Image=Image` key, which had to be
   reverted from the snapshot. The correct sequence is to set `currentConfigGroup` first.
2. The first write attempt used the wrong API shape and polluted `appletsrc`; recovery
   required restoring the file image *and* re-applying the value through the API.

### Alternative approaches
1. `plasma-apply-wallpaperimage <path>` — installs+applies for the current activity;
   no per-screen control, no read-back.
2. `setWallpaper(plugin, map, screen)` DBus — present in the introspection
   (`setWallpaper sa{sv}u`), but building `a{sv}` from Rust is more work than the script
   path for no extra capability.
3. Direct `appletsrc` edit — would not notify plasmashell; deferred to last resort.

### Final recommendation
Use the scripting path with an explicit `currentConfigGroup`, verify with the DBus
`wallpaper(u)` map (independent channel), and snapshot `Image` **plus** the plugin and
fill mode. This removes the V0.1 "wallpaper gate" and the "manual step" fallback.

---

## 5. Global themes (Look-and-Feel)

### Goal
Prove that a complete theme can be applied, its current state determined, its individual
components respected, and the previous state restored — without destroying the desktop.

### Environment

8 installed `Plasma/LookAndFeel` packages; `plasma-apply-lookandfeel --list` reports 6. Qt styles installed: `breeze6`, `UnionWidgetsStyle` only. `/etc/xdg/kdeglobals` sets `LookAndFeelPackage=com.valve.vapor.desktop`, while `~/.config/kdedefaults/package` also names `com.valve.vapor.desktop`.

### Preferred approach
`plasma-apply-lookandfeel -a <package>`, then `LookAndFeelPackage` for detection.

### Commands
```bash
plasma-apply-lookandfeel --list
plasma-apply-lookandfeel -a org.kde.breezedark.desktop
kreadconfig6 --file kdeglobals --group KDE --key LookAndFeelPackage
```

### Result
`PASS` for apply and restore; `PARTIAL` for detection.

### Evidence
- `-a org.kde.breezedark.desktop` exited `0`.
- **The panel was untouched**: same id `23`, same 7 widgets in the same order, same
  geometry. `--resetLayout` is a separate opt-in flag.
- The application did **not** write per-key values. It *deleted* the user's overrides and
  wrote one marker:

```diff
 [General]
-ColorScheme=BreezeDark
 ColorSchemeHash=27363f5706202c1c8261fbda7381ddaab8ad48f3
-[Icons]
-Theme=breeze-dark
 [KDE]
+LookAndFeelPackage=org.kde.breezedark.desktop
-widgetStyle=Breeze
```
  and in `kwinrc` it removed `[org.kde.kdecoration2]`, in `kcminputrc` it removed
  `cursorTheme=capitaine-cursors`.
- It also **rewrote `~/.config/kdedefaults/`**: `package` became
  `org.kde.breezedark.desktop`, `plasmarc name=default`, `ksplashrc` theme changed.
- Detection by key-matching over all 8 installed packages returned **no exact match**;
  the best candidate was `org.kde.breezedark.desktop` at **5/8 keys (62%)**, because
  `plasmarc`, `kcminputrc` and `ksplashrc` did not match.
- `-l/--list` reported 6 packages while 8 directories exist (`com.github.rkstrdee.emerald`
  is installed but not listed).

### Problems
1. `LookAndFeelPackage` is unreliable as "current state": `/etc/xdg/kdeglobals` sets
   `com.valve.vapor.desktop` system-wide, and that marker survives per-key
   customisation. Reading it alone over-reports a theme the user has left.
2. Restoring required three distinct actions: restore config files **and**
   `kdedefaults/`, delete the marker when it was absent, then re-apply live state. My
   first restore missed `kdedefaults/` and left the plasma style reporting `default`
   instead of `Vapor`; it was repaired by rewriting `kdedefaults/plasmarc`,
   `ksplashrc` and `package` to their original byte sizes (19/58/23 bytes) and values.
3. `plasma-apply-lookandfeel` can set values for components that are not installed
   (`widgetStyle=Windows` in the VGUI package; only `breeze6` and `UnionWidgetsStyle`
   styles exist), with silent fallback.

### Alternative approaches
1. `plasma-apply-lookandfeel -a` — works, coarse-grained (one package = many files),
   destructive to overrides.
2. **Decompose** `contents/defaults` into primitive operations — the file is a plain
   KConfig whose section headers encode `(file, group, subgroup)`, e.g.
   `[kdeglobals][General] ColorScheme=BreezeDark`. Each line maps onto a capability
   Morphix already has.
3. `--resetLayout` — never required for a theme change; excluded by policy.

### Final recommendation
Support global themes as a **composed plan**: read the package's `contents/defaults`,
express it as typed per-resource operations, snapshot each, apply through the normal
capability adapters, and roll back per resource. Keep `-a` only as a compatibility path,
and never pass `--resetLayout`. Detect the current theme as: exact `LookAndFeelPackage`
marker **plus** an override diff; if the declared keys are mixed, report "custom
(derived from X)" rather than guessing. ADR-012's blanket exclusion is no longer
justified.

---

## 6. Panels

### Goal
Control location, height, alignment, floating, visibility, widgets, and panel
creation/removal, with verification and restoration.

### Environment

Single panel id 23: bottom edge, 30 px, center, floating, `hiding=none`, `opacity=translucent`, 7 widgets (kickoff, pager, icontasks, marginsseparator, systemtray, digitalclock, showdesktop). `plasmashell` 6.8.0, one screen.

### Preferred approach
The Plasma scripting `Panel` object from `panels()` / `new Panel()`.

### Commands / APIs

```javascript
// property round-trip
var p = panels()[0];
print(p.location + " " + p.height + " " + p.alignment + " " + p.floating + " " + p.hiding);
p.location = "top"; p.height = 44; p.alignment = "left";
p.floating = false; p.hiding = "autohide";

// creation and removal
var np = new Panel();          // returns a panel with a new id
np.location = "top"; np.height = 36;
np.addWidget("org.kde.plasma.digitalclock");
np.remove();

// post-removal residue
// grep '\[PlasmaViews\]\[Panel' ~/.config/plasmashellrc
```

### Result
`PASS` for location, height, alignment, floating, hiding, create, remove, widget add /
configure / remove. `FAIL` for opacity. `NOT TESTED` for length/width.

### Evidence
```javascript
var p = panels()[0];
p.location = "top"; p.height = 44; p.alignment = "left";
p.floating = false; p.hiding = "autohide";
// read-back
location=top height=44 alignment=left floating=false hiding=autohide
// restore
location=bottom height=30 alignment=center floating=true hiding=none
```
- Creation: `new Panel()` produced id `46`, `panels().length` went 1→2, `remove()`
  brought it back to 1 (`ids=[23]`).
- Widgets on the new panel: `addWidget("org.kde.plasma.digitalclock")` → id `47`,
  `panelWidgets=1`; `remove()` was asynchronous (`panelWidgets` still 1 immediately).
- On the real panel, `addWidget` produced id `48`; after `remove()` plus a 2 s poll the
  panel was back to its original 7 widgets and `AppletOrder=24;25;26;27;28;38;39`.
- Panel property persistence goes to `~/.config/plasmashellrc`
  (`[PlasmaViews][Panel 23] floating=1`, `[Defaults] thickness=30`), while `location` and
  the containment live in `plasma-org.kde.plasma.desktop-appletsrc`.

### Problems
1. **`panel.opacity` is read-only in practice.** Setting `"opaque"`, `"translucent"`,
   `"adaptive"`, the capitalised forms, and `0/1/2` all left the value at `translucent`.
   No error is raised. There is no scripting path to panel opacity.
2. **Persistence is deferred.** Immediately after a property change, `appletsrc` was
   unchanged; `plasmashellrc` still held the *old* values for ~15 s, then converged
   (`alignment=132`, `floating=1`, `thickness=30`). A snapshot taken immediately after
   apply would capture the new state, not the pre-state.
3. **Removing a panel leaves residue**: `[PlasmaViews][Panel 46]` and
   `[PlasmaViews][Panel 46][Defaults]` remained in `plasmashellrc` after the panel was
   gone. The containment and applet sections were cleaned up correctly.
4. **Side effects on unrelated keys**: the `[ScreenMapping]` section was rewritten by
   panel operations.
5. `alignment` is stored as Qt alignment flags (`center` → `132`), so a textual diff is
   misleading; compare semantically.

### Alternative approaches
1. Scripting `Panel` object — best coverage, live, reversible.
2. Direct `plasmashellrc`/`appletsrc` edit — persisted but not live, and clashes with
   plasmashell's in-memory state (it rewrites the file later).
3. Shell restart — last resort; see §14 for why it is dangerous.

### Final recommendation
Adopt panels into MVP with the scripting adapter. Require a **quiesce step**: after
mutation, wait for persistence to converge before hashing files, and verify panel state
through the API rather than the file. Treat panel opacity as unsupported until a working
path is proven; when a pack requests it, refuse with `E_PANEL_OPACITY_UNSUPPORTED` rather
than silently ignoring it. Delete orphan `[PlasmaViews][Panel N]` sections for panels the
transaction created.

---

## 7. Widgets

### Goal
Detect, add, configure, remove, and restore widgets; determine whether full layout state
can be backed up.

### Environment

Same panel 23; `knownWidgetTypes` reports 68 installed widget types; `panel.widgetIds` = `24,25,26,27,28,38,39`.

### Preferred approach
`panel.widgets()`, `panel.addWidget(type)`, `widget.readConfig/writeConfig`, `widget.remove()`.

### Commands / APIs

```javascript
var p = panels()[0];
var w = p.addWidget("org.kde.plasma.digitalclock");   // -> new id, index -1
w.currentConfigGroup = ["Configuration", "General"];
print(w.readConfig("showSeconds", "ABSENT"));
w.writeConfig("showSeconds", "true");
print(w.readConfig("showSeconds", "ABSENT"));
w.remove();                                            // asynchronous
print(p.widgets().length);                             // still 8 immediately after
```

### Result
`PASS` for detect/add/configure/remove; `PARTIAL` for full-layout backup and restore.

### Evidence
```
BEFORE widgets=7 ids=24,25,26,27,28,38,39
ADDED id=48 type=org.kde.plasma.digitalclock
cfgGroup=["Configuration","General"] keys=[]
showSeconds before='ABSENT' after='true' restored='false'
widgets before remove=8 / after remove=8 (still 8 immediately)
after poll: panel23 widgets(7)=… (original order, id 48 gone from appletsrc)
```

### Problems
1. **Removal is asynchronous** — verification must poll, not assert once.
2. Writing a config value that did not exist (`showSeconds`) and then "restoring" it wrote
   `false` where the key had been absent. Inert, but not byte-clean.
3. Widget IDs are assigned by plasmashell; a removed widget restored later gets a new id,
   so "restore" is type + order + config, not identity.
4. `configGroups` was empty for a freshly added widget, so configuration groups must be
   created by assignment rather than enumerated.

### Alternative approaches
1. Scripting API — full control, live.
2. `appletsrc` manipulation — would need a shell restart and would fight autosave.
3. `kpackagetool6` for installation of third-party widgets — out of scope: MVP ships no
   widget packages, it only references installed types.

### Final recommendation
Adopt widgets into MVP, constrained to an allowlist derived from `knownWidgetTypes` and a
curated subset, and only "safe" widgets (clock, pager, spacer, separator, show-desktop,
trash, system monitor). Snapshot `(type, index, config)` per widget; restore by rebuilding
in order and verifying types and indices.

---

## 8. KWin effects

### Goal
Discover, read, toggle, configure, verify, and restore compositor effects.

### Environment

KWin 6.8.0 Wayland; `/Effects` reports 57 effect ids; initially loaded: `blur` only. `org.kde.KWin /Effects` interface present on the session bus.

### Preferred approach
The `org.kde.kwin.Effects` DBus interface on `org.kde.KWin /Effects`.

### Commands / APIs

```bash
qdbus6 org.kde.KWin /Effects org.kde.kwin.Effects.listOfEffects
qdbus6 org.kde.KWin /Effects org.kde.kwin.Effects.isEffectLoaded wobblywindows
qdbus6 org.kde.KWin /Effects org.kde.kwin.Effects.loadEffect wobblywindows
kwriteconfig6 --file kwinrc --group Plugins --key wobblywindowsEnabled --type bool true
qdbus6 org.kde.KWin /KWin org.kde.KWin.reconfigure
```

### Result
`PASS` for discover/read/toggle/verify/restore; `PARTIAL` for parameters.

### Evidence
- `listOfEffects` returned 57 unprefixed ids: `blur`, `diminactive`, `wobblywindows`,
  `magiclamp`, `overview`, … **not** the Plasma 5 `kwin4_effect_*` names.
- `isEffectSupported("contrast")` → `false` (no such effect in 6.8).
- `loadEffect("wobblywindows")` → `true`; `isEffectLoaded` → `true`;
  `activeEffects` remained `blur` (so `activeEffects` is not a verification signal).
- **`loadEffect` did not touch `kwinrc`**: no `[Plugins]` section was created.
- `kwriteconfig6 [Plugins] wobblywindowsEnabled=true` + `KWin.reconfigure` left
  `isEffectLoaded=false`.
- The combination — write `kwinrc` **and** call `loadEffect` — yielded
  `loaded=true`; rollback with `kwinrc=false` + `unloadEffect` yielded `loaded=false`,
  and deleting the key restored `kwinrc` to baseline.

### Problems
1. Two mechanisms are needed: config for persistence across sessions, DBus for liveness.
   Neither alone is sufficient.
2. `supportInformation <effect>` returned an empty string for `blur`; there is no
   introspection of effect parameters.
3. Plasma 5 effect ids are wrong on Plasma 6; an allowlist copied from older docs would
   silently match nothing.

### Alternative approaches
1. DBus `loadEffect` only — not persistent.
2. `kwinrc` + `reconfigure` only — not applied.
3. Both — works.

### Final recommendation
Adopt effects with a *both* write strategy, and derive the supported id list from
`listOfEffects` at runtime, intersected with a curated allowlist. Effect parameters stay
allowlisted and are verified by config read-back only; the UI must not claim a visual
parameter change was verified.

---

## 9. Window decorations

### Goal
Detect the current decoration, enumerate alternatives, switch, verify live, and roll back.

### Environment

Decoration plugins installed: `org.kde.breeze`, `org.kde.kwin.aurorae` (+ `.v2`), and the QML `plastik` decoration. `/usr/share/aurorae/themes/` is empty, so no classic Aurorae theme exists on this host.

### Preferred approach
`kwinrc [org.kde.kdecoration2] library/theme` + `KWin.reconfigure`, verified by
`supportInformation`.

### Commands / APIs

```bash
kwriteconfig6 --file kwinrc --group org.kde.kdecoration2 --key library org.kde.kwin.aurorae
kwriteconfig6 --file kwinrc --group org.kde.kdecoration2 --key theme kwin4_decoration_qml_plastik
qdbus6 org.kde.KWin /KWin org.kde.KWin.reconfigure
qdbus6 org.kde.KWin /KWin supportInformation | sed -n '/^Decoration$/,/^$/p'
```

### Result
`PASS`

### Evidence
```
Decoration
Plugin: org.kde.breeze      Theme: Breeze      (before)
Plugin: org.kde.kwin.aurorae Theme: kwin4_decoration_qml_plastik   (after switch)
Plugin: org.kde.breeze      Theme:                 (after restore)
```
Panel-crop pixel diff across the switch: `0.0168`; restore returned `kwinrc` byte-identical.

### Problems
1. On restore, `Theme:` came back **empty** rather than `Breeze`, even though
   `kwinrc` said `theme=Breeze`. The decoration still rendered as Breeze. Verification
   must therefore treat "plugin matches and theme is empty-or-equal" as success, or
   compare against the plugin's reported name.
2. Only `org.kde.breeze` and `org.kde.kwin.aurorae` are installed; `/usr/share/aurorae/themes/`
   is empty, so no classic Aurorae theme could be exercised. A QML decoration
   (`plastik`) was used instead.
3. Decoration configuration lives in `kwinrc`, **not** `kdeglobals` (the latter had no
   `[org.kde.kdecoration2]` group), contradicting a common Plasma 5 assumption.

### Alternative approaches

1. `kwinrc [org.kde.kdecoration2]` + `KWin.reconfigure` — validated live.
2. Editing the file without `reconfigure` — no visible effect.
3. `kwin_wayland --replace` — forbidden (ADR-020); it is a compositor restart.

### Final recommendation
Adopt decorations. Verify against `supportInformation` `Plugin:` and tolerate an empty
`Theme:` for plugins that do not name a theme. Ship an original Aurorae theme in the
demo packs and test it on the release matrix.

---

## 10. Color schemes

### Goal
Detect, install, apply, verify, and roll back a color scheme.

### Environment

14 installed `.colors` files in `/usr/share/color-schemes/`; current scheme
`BreezeDark`; Plasma style `Vapor` controls the panel's colors, so panel pixels do not
track the color scheme.

### Preferred approach

`plasma-apply-colorscheme` (the tool performs the live notification), with
`kdeglobals [General] ColorScheme` as the read-back.

### Commands / APIs

```bash
plasma-apply-colorscheme BreezeLight
kreadconfig6 --file kdeglobals --group General --key ColorScheme
# installation: copy <name>.colors into ~/.local/share/color-schemes/
```

### Result
`PASS`

### Evidence
- Apply `BreezeLight`: `Successfully applied the color scheme BreezeLight to your current
  Plasma session`; `kdeglobals [General] ColorScheme=BreezeLight`; the `[Colors:*]` blocks
  were rewritten (e.g. `[Colors:Button] BackgroundNormal=252,252,252`).
- Restore `BreezeDark`: `ColorScheme=BreezeDark`, `BackgroundNormal=41,44,48`.
- Installation: a copy of `BreezeDark.colors` with `Name=Morphix Probe` placed in
  `~/.local/share/color-schemes/MorphixProbe.colors` was selectable and applied
  (`config readback=MorphixProbe`), then removed and the previous scheme restored.
- Panel-strip pixel diff after a scheme change was **0**, because the panel is styled by
  the Plasma style (`Vapor`), not by the color scheme. Visual verification must therefore
  target an application window, not the panel.

### Problems
- Applying a scheme writes a large `[Colors:*]` block; a snapshot must capture the whole
  block, not just the name.
- Restoring added `ColorScheme=BreezeDark` where the key had previously been inherited.

### Alternative approaches

1. `plasma-apply-colorscheme` — validated, performs the live notification.
2. Direct `kdeglobals [Colors:*]` write — would not notify running applications.
3. `kded`/`KGlobalSettings` notification by hand — no documented, stable entry point.

### Final recommendation
Adopt color schemes, including installation into `~/.local/share/color-schemes/`.
Snapshot `ColorScheme` + all `[Colors:*]` sections; delete the key on rollback if it was
absent. Do not use panel pixels as the verification signal for color schemes.

---

## 11. Icons

### Goal
Detect and activate an icon theme, verify visually, and roll back.

### Environment

Installed icon themes include `breeze`, `breeze-dark`, `Adwaita`, `steam`; current
`breeze-dark`. Verification region: the panel strip (1000x30 at y=1050).

### Preferred approach

`kdeglobals [Icons] Theme` write, then a plasmashell restart, then pixel comparison of the
static panel strip.

### Commands / APIs

```bash
kwriteconfig6 --file kdeglobals --group Icons --key Theme breeze
systemctl --user reset-failed plasma-plasmashell.service   # only in the test harness
systemctl --user restart plasma-plasmashell.service
magick compare -metric RMSE before.panel.png after.panel.png null:
```

### Result
`PASS` **only with a plasmashell restart**; live change `FAIL`.

### Evidence
- Live: `kwriteconfig6 Icons Theme breeze` → `readback=breeze`, panel-strip RMSE after
  4 s = **0**; restore RMSE = **0**. No live effect.
- After `systemctl --user restart plasma-plasmashell.service`:
  `RMSE dark→breeze = 0.00845`, and `dark→restored = 4.2e-05` (pixel-exact recovery).

### Problems
1. No live notification path was found; the theme is resolved at shell start.
2. Restarting plasmashell is disruptive and can fail (see §14).
3. Installing an icon theme was **not tested**; only selection of an already-installed
   theme. Installation is an XDG directory copy, verified separately for color schemes.

### Alternative approaches

1. `kdeglobals` write + live reload — **no live effect observed** (RMSE 0 after 4 s).
2. `kbuildsycoca6` — not tested; would only rebuild the cache, not notify the shell.
3. Shell restart — validated and pixel-exact, but forbidden for the engine (ADR-020).

### Final recommendation
Adopt icons with a documented **restart-required** class. The plan must say "requires
Plasma restart" before the user consents, and the engine must not restart the shell
automatically. Offer "apply now, takes effect on next login" as the default.

---

## 12. Cursors

### Goal
Detect, activate on Wayland, verify live, and roll back.

### Environment

Current cursor `capitaine-cursors`, size 24; installed themes include `breeze_cursors`,
`capitaine-cursors`, `Adwaita`. Wayland session, so KWin owns the cursor.

### Preferred approach

`plasma-apply-cursortheme`, verified through KWin's own live state.

### Commands / APIs

```bash
plasma-apply-cursortheme breeze_cursors
qdbus6 org.kde.KWin /KWin supportInformation    # Cursor: themeName
kreadconfig6 --file kcminputrc --group Mouse --key cursorTheme
```

### Result
`PASS` for the theme; `PARTIAL` for size.

### Evidence
```
before: config=capitaine-cursors  live themeName: capitaine-cursors
apply:  "Successfully applied the mouse pointer theme Breeze Dark"
after:  config=breeze_cursors     live themeName: breeze_cursors
restore:config=capitaine-cursors  live themeName: capitaine-cursors
```

### Problems
1. **Desync trap**: after restoring `kcminputrc` by file copy, the tool refused to act —
   `The requested theme “capitaine-cursors” is already set as the theme for the current
   Plasma session` — while KWin still reported `breeze_cursors`. The tool compares against
   config, not live state. Workaround that works: apply a *different* theme first, then
   the target.
2. **Cursor size does not apply**: writing `kcminputrc [Mouse] cursorSize=40` (with and
   without `KWin.reconfigure`) left KWin `themeSize: 24`.
3. Cursor theme installation was **not tested** (selection only).

### Alternative approaches

1. `plasma-apply-cursortheme` — validated live.
2. `kcminputrc [Mouse] cursorTheme` write — config only, no notification.
3. Forced two-step re-apply — required when config and live state disagree.

### Final recommendation
Adopt cursor *themes* as a fully live capability, with the forced-reapply sequence encoded
in the adapter. Treat cursor *size* as unsupported until a working path is found; refuse
rather than silently not applying.

---

## 13. Fonts

### Goal
Read the font configuration, change it safely, and restore it.

### Environment

No `[General] font` key in `~/.config/kdeglobals`; the effective font comes from compiled
defaults (`Noto Sans,10,-1,5,…`).

### Preferred approach

Write the allowlisted font keys in `kdeglobals` and verify by config read-back.

### Commands / APIs

```bash
kwriteconfig6 --file kdeglobals --group General --key font "Noto Sans,16,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0"
kwriteconfig6 --file kdeglobals --group General --key font --delete
```

### Result
`PARTIAL`

### Evidence
- `kdeglobals [General] font` was absent; the effective value came from compiled defaults
  (`Noto Sans,10,…`).
- Writing `font` with size 16 (and later 14) produced **panel RMSE 0**.
- After a plasmashell restart, comparing two post-restart states (font 16 vs restored)
  gave `RMSE = 3.89e-05`, i.e. indistinguishable; the earlier `0.0135` was the restart
  itself, not the font.

### Problems
- Plasma's panel/chrome does not use `[General] font`; it uses theme fonts. So "change the
  font" has no effect on Plasma UI, only on applications that read `KGlobalSettings` at
  start-up.
- Already-running applications keep their font until restarted.

### Alternative approaches

1. Config write — validated at state level; running apps keep their font.
2. Shell restart — tested; panel rendered identically (RMSE 3.9e-05), so Plasma chrome
   does not consume this key at all.
3. Per-application font configuration — out of the fixed target map.

### Final recommendation
Support font *keys* as configuration with a documented limitation: it affects newly
started applications, not Plasma chrome, and not running apps. Do not advertise it as a
desktop-wide visual change. Ship no proprietary fonts; only reference installed families
and validate availability.

---

## 14. Shortcuts

### Goal
Read, set, detect conflicts, verify, and restore a global shortcut.

### Environment

`~/.config/kglobalshortcutsrc` present; `org.kde.kglobalaccel` is served by `kwin_wayland`.
Probe action: `plasmashell` / `show dashboard` (Ctrl+F12 = `83886139`).

### Preferred approach

`org.kde.kglobalaccel.setShortcut` with `SetPresent`, verified by `shortcut()`.

### Commands / APIs

```bash
gdbus call --session --dest org.kde.kglobalaccel --object-path /kglobalaccel \
  --method org.kde.KGlobalAccel.isGlobalShortcutAvailable 83886138 plasmashell
gdbus call --session --dest org.kde.kglobalaccel --object-path /kglobalaccel \
  --method org.kde.KGlobalAccel.setShortcut \
  "['plasmashell','show dashboard','plasmashell','Show Desktop']" "[83886138]" 4
gdbus call --session --dest org.kde.kglobalaccel --object-path /kglobalaccel \
  --method org.kde.KGlobalAccel.shortcut \
  "['plasmashell','show dashboard','plasmashell','Show Desktop']"
```

### Result
`PASS`

### Evidence
```
shortcut(["plasmashell","show dashboard",…]) -> ([83886139],)   # Ctrl+F12
isGlobalShortcutAvailable(Ctrl+F11=83886138, plasmashell) -> true
isGlobalShortcutAvailable(Meta+1=83886129, kwin)          -> false
setShortcut(…, [83886138], 4) -> readback ([83886138],)
kglobalshortcutsrc: show dashboard=Ctrl+F11,Ctrl+F12,Show Desktop
restore:            show dashboard=Ctrl+F12,Ctrl+F12,Show Desktop
```

### Problems
1. **Flags matter.** `flags=0`, `1`, `2` were silent no-ops; only `flags=4`
   (`SetPresent`) changed the binding.
2. The `QKeySequence` wire format is an integer; computing it requires knowing
   `Qt::Key_*` values (F1 = `0x01000030`).
3. After saving, KGlobalAccel rewrote the whole `[$InverseAction]` table, so byte-level
   comparison fails; the semantic shortcut value is unchanged.

### Alternative approaches

1. `kglobalaccel` DBus — validated, including conflict detection and read-back.
2. `kglobalshortcutsrc` write — not live; KGlobalAccel owns the runtime table.
3. `kwriteconfig6` + `kwin` restart — forbidden and unnecessary.

### Final recommendation
Adopt shortcuts. Use `isGlobalShortcutAvailable` before writing, `setShortcut` with
`SetPresent`, read back with `shortcut()`, and compare the decoded key sequence rather
than the file.

---

## 15. Desktop behaviour

### Goal

Determine whether virtual desktops, workspace layout, hot corners, window rules, animation
speed, task switching and overview behaviour can be controlled safely and reversibly.

### Environment

One virtual desktop (`58c5aa6f-…`), `Rows=1`; `kwinrc` has no `[ElectricBorders]` keys yet;
`kwinrulesrc` contains one pre-existing Steam rule; `VirtualDesktopManager` is available on
the session bus.

### Preferred approach

`org.kde.KWin.VirtualDesktopManager` for desktops; `kwinrc` + `KWin.reconfigure` for edges,
animation and switchers; `kwinrulesrc` for window rules.

### Commands / APIs

```bash
qdbus6 org.kde.KWin /VirtualDesktopManager org.kde.KWin.VirtualDesktopManager.createDesktop 1 MorphixTest
qdbus6 org.kde.KWin /VirtualDesktopManager org.kde.KWin.VirtualDesktopManager.removeDesktop <uuid>
kwriteconfig6 --file kwinrc --group ElectricBorders --key TopLeft ShowDesktop
qdbus6 org.kde.KWin /KWin org.kde.KWin.reconfigure
qdbus6 org.kde.KWin /KWin supportInformation | sed -n '/^Screen Edges$/,/^$/p'
```

### Result

`PASS` for virtual desktops and hot corners; `PARTIAL` for window rules, animations and
workspace rows; `NOT TESTED` for task switching and overview.

### Evidence

#### Virtual desktops — PASS
```
count=1 -> createDesktop(1,"MorphixTest") -> count=2
desktops: (0, 58c5aa6f…, "Desktop 1"), (1, 825dcfd8…, "MorphixTest")
kwinrc: Id_1=… Id_2=825dcfd8… Name_2=MorphixTest
removeDesktop(825dcfd8…) -> count=1, kwinrc Desktops back to Number=1
```
Residue: `[Tiling][825dcfd8-…][…]` remained in `kwinrc` after removal.

#### Hot corners — PASS
```
kwriteconfig6 [ElectricBorders] TopLeft=ShowDesktop + KWin.reconfigure
readback=ShowDesktop ; --delete -> UNSET ; kwinrc byte-identical to baseline
supportInformation: actionTopLeft: 0 (numeric live state available)
```

#### Window rules — PARTIAL
A rule was written to `kwinrulesrc` (`wmclass=morphix-probe-nonexistent`,
`skiptaskbar=true`, `types=1`) and appended to `[General] Order`; `KWin.reconfigure`
left it intact with no journal errors, and removal restored the file byte-identically.
**No verification channel exists**: no DBus call lists rules or reports a match, so the
engine can only confirm what it wrote, not that KWin applied it.

#### Animations — PARTIAL
`kdeglobals [KDE] AnimationDurationFactor` read/write/restore verified (`0.25 → 2.0 → 0.25`).
No visual verification was attempted (it is timing-dependent and not reliably
screenshot-verifiable).

#### Task switching / Overview — NOT TESTED
`kwinrc [WindowSwitcher] LayoutName` and the `overview` effect are present, but neither was
exercised.

#### Workspace rows / wrapping — PARTIAL
`rows` and `navigationWrappingAround` are readable DBus properties; only reads were done.

---

### Problems

1. Removing a desktop left a `[Tiling][<uuid>]` section behind.
2. Window rules have **no verification channel**: no DBus call lists rules or reports a
   match, so the engine can only confirm what it wrote.
3. `kwin.kcfg` exposes `ScreenEdges` with only `RemainActiveOnFullscreen` and `TouchTarget`;
   the corner *actions* live in `[ElectricBorders]`, which is not in that schema group.
4. Task switching and overview were not exercised.

### Alternative approaches

1. DBus desktop manager — validated, including persistence to `kwinrc`.
2. Direct `kwinrc [Desktops]` editing — would not notify KWin.
3. Plasma scripting for panel-adjacent behaviour — not applicable to KWin settings.

### Final recommendation

Adopt virtual desktops and hot corners. Adopt window rules and animation keys as
state-verified only, with the UI stating that a visual effect was not verified. Mark task
switching and overview `PENDING` until they are exercised on a disposable VM.

## 16. Snapshot and transaction model

### Goal
Prove that the plan → snapshot → apply → verify → commit/rollback cycle works against real
KDE mechanisms, including partial failure and a hard crash.

### Environment

Prototype engine `txn.py` over five real resources (color scheme, cursor theme, panel height, KWin effect, wallpaper), JSON journal fsynced before each transition, reverse-order rollback. Same Plasma 6.8.0 session.

### Preferred approach
A prototype engine (`txn.py`) with five resources — color scheme, cursor theme, panel
height, KWin effect, wallpaper — each with `snapshot/apply/verify/restore`, a JSON journal
fsynced before every state transition, and reverse-order rollback.

### Commands / APIs

```bash
python3 txn.py apply                 # happy path, five resources
python3 txn.py apply --fail-at 2     # injected failure at operation 3 of 5
python3 txn.py apply --crash-at 3    # SIGKILL during operation 4
python3 txn.py recover               # startup recovery, idempotent
python3 txn.py rollback              # rollback of a committed transaction
```

### Result
`PASS` in all four scenarios.

### Evidence

**Happy path**
```
snapshot[0] appearance.color_scheme = 'BreezeDark'
apply[0] … verified=True  … apply[4] … verified=True
COMMITTED
```

**Rollback of a committed transaction**
```
revert[4] wallpaper verified=True
revert[3] effect → None verified=True
revert[2] panel.height → 30 verified=True
revert[1] cursor → 'capitaine-cursors' verified=True
revert[0] color scheme → 'BreezeDark' verified=True
ROLLED_BACK (verified)
```

**Injected failure at operation 3 of 5**
```
apply[0] verified=True
apply[1] verified=True
!! injected failure at op 2 (panel.height)
revert[1] … verified=True
revert[0] … verified=True
ROLLED_BACK (verified)
```
Operations 4 and 5 never ran; journal recorded `snapshotted` for them.

**Hard crash (`kill -9`) during operation 4 of 5**
```
exit=137
state: colorscheme=BreezeLight cursor=breeze_cursors height=44 wallpaper=Steam…
journal: applying; ops 0-2 applied, op 3 executing, op 4 snapshotted
recover:
  revert[3] … verified=True
  revert[2] … verified=True
  revert[1] … verified=True
  revert[0] … verified=True
ROLLED_BACK (verified)
second recover: "journal state=rolled_back: nothing to recover"
```

### Problems
1. Rollback ordering matters: restoring files first and then re-applying live state is
   required. Doing it in the other order (as one early attempt did) left KWin's cursor on
   `breeze_cursors` while config claimed `capitaine-cursors`.
2. Some resources need a *forced* re-apply because their tool refuses to act when config
   already matches (§12).
3. "Restore to absent" is a distinct operation from "restore to value"; conflating them
   creates permanent overrides (demonstrated on `plasmarc`).
4. Persistence lag means the engine must quiesce before hashing files (§6).

### Alternative approaches

1. Journal + inverse operations (chosen) — verified in all four scenarios.
2. Whole-file snapshots without a journal — cannot identify progress after a crash.
3. SQLite-only state without a separate journal — the prototype's JSON journal was fsynced
   independently, which made recovery after `SIGKILL` deterministic.

### Final recommendation
Keep the model. Add three things the prototype proved necessary: a **quiesce/persist
wait**, a **forced re-apply** path, and **residue cleanup** for engine-created objects
(panels, desktops, orphan sections). Journal-before-action plus idempotent recovery worked
first time, including after `SIGKILL`.

---

## 17. Reload and restart behaviour

### Goal
Determine which changes need a reload or restart, and whether restarting plasmashell is
safe for the engine to do.

### Result
Per-change results are in [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md) §3.

### Problems
**Restarting plasmashell failed under repetition.** Two restarts in quick succession
produced:

```
Job for plasma-plasmashell.service failed because start of the service was attempted too often.
```

leaving the session **without a panel** (screenshot RMSE jumped to `0.179`, shell
`is-active=failed`) until `systemctl --user reset-failed` + `start` recovered it.

### Final recommendation
The engine must **never** restart plasmashell as part of apply. Changes that need a shell
restart (icon theme, some fonts) are reported as "takes effect on next login". If a
restart is ever offered, it is an explicit user action with a health check and automatic
`reset-failed` recovery.

---

## 18. What remains unverified

| Item | Why |
| --- | --- |
| Multi-screen wallpaper/panels | single-screen machine |
| Panel length / width | not exercised (properties exist) |
| Panel opacity alternative path | scripting property is read-only; `plasmashellrc` + restart untested |
| Cursor size live effect | no working path found |
| Icon / cursor theme *installation* | only selection tested; install verified for color schemes |
| Window-rule effect on a live window | no verification channel, no matching window |
| Task switching, overview | out of the tested subset |
| `--resetLayout` | deliberately not run on the live session |
| Plasma 6 versions other than 6.8.0 | single-version session |
