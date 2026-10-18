# Morphix — Customization Pack Specification (schema v1)

- **Status:** Design baseline. Implemented in Phase 2 (see [PHASES.md](PHASES.md)).
- **Applies to:** Morphix V0.1 and later until a new schema version supersedes it.
- **Related:** [ARCHITECTURE.md](ARCHITECTURE.md) §6 (capability/target map),
  [SECURITY.md](SECURITY.md), [MVP.md](MVP.md) §4.

A Morphix pack is a **declarative description of a desktop customization**. It contains
assets and a manifest that selects assets and allowlisted settings. It contains no
executable logic. Morphix never executes, sources, or interprets anything a pack provides.

---

## 1. Design rules

1. **No scripts, ever.** No shell, JavaScript, Python, binaries, or "post-install hooks".
2. **No absolute paths and no path grants.** A pack names asset *keys*; the engine maps them
   to fixed install locations. The illustrative `filesystem = [...]` permission list from
   the original product brief is deliberately not part of this schema (ADR-010).
3. **No URLs.** Assets are local files inside the pack. `homepage` is display-only and is
   never fetched.
4. **Closed capabilities.** A pack may request only the fifteen capabilities in §6.
5. **Closed schema.** Unknown fields are errors, not warnings. Forward compatibility is
   handled by `schema_version`, not by ignoring data.
6. **Fully declarative.** Applying a pack is reproducible from `pack.toml` + assets +
   engine version.
7. **Reviewable.** Every file is either referenced by the manifest, a preview, a required
   document, or the checksum file. Unreferenced files are rejected.
8. **Small.** Hard size and count limits keep validation and review practical.

## 2. Directory structure

```
cupertino/
├── pack.toml                 # required, the only manifest file
├── README.md                 # required, human description
├── ATTRIBUTION.md            # required, asset provenance and licenses
├── checksums.sha256          # required for distributed packs; optional for local dev
├── previews/                 # 1..8 preview images, referenced by pack.toml
│   ├── icon.png
│   ├── desktop-light.webp
│   └── desktop-dark.webp
├── themes/
│   ├── color-schemes/
│   │   ├── CupertinoDark.colors
│   │   └── CupertinoLight.colors
│   └── aurorae/
│       └── Cupertino/
├── icons/
│   └── Cupertino/
├── cursors/
│   └── Cupertino/
├── plasma/
│   └── desktoptheme/
│       └── Cupertino/
└── wallpapers/
    ├── light.webp
    └── dark.webp
```

Rules:

- File names are UTF-8, case-sensitive, at most 255 bytes, no control characters.
- Maximum path depth is 8; maximum path length is 255 characters.
- Asset paths must be relative to the pack root and must start with one of
  `themes/`, `icons/`, `cursors/`, `plasma/`, `wallpapers/`, or `previews/`.
- Symlinks, hardlinks, device nodes, FIFOs, sockets, and executable-bit files are rejected
  anywhere in the pack.
- Zero-byte files are rejected for every asset kind, preview, and required document.

## 3. Limits

| Limit | Value |
| --- | --- |
| `pack.toml` size | 256 KiB |
| Total pack size (uncompressed) | 200 MiB |
| Single asset or preview file | 50 MiB |
| File count | 2,000 |
| Preview count | 8 |
| Preview size | 5 MiB each |
| Asset keys per kind | 64 |
| `[[settings]]` entries | 128 |
| `[[shortcuts]]` entries | 64 |
| KWin effect entries | 32 |
| Dependency entries | 16 |
| Pack ID length | 128 bytes |
| Version string length | 64 bytes |
| Description length | 8 KiB |
| Summary length | 200 characters |

Limits are enforced before allocation where possible (size checks precede reads).

## 4. Manifest: complete reference

The canonical, valid example below uses every main section except the optional dependency
table (shown separately in §5.12). A minimal pack uses the same shapes with unused sections
omitted.

```toml
schema_version = 1

[pack]
id = "dev.morphix.cupertino"
name = "Cupertino"
version = "0.1.0"
summary = "A macOS-inspired light and dark desktop composition built from original assets."
description = """
A calm, high-contrast desktop composition inspired by the visual language of macOS,
built entirely from original open assets. Includes light and dark color schemes,
an icon theme, a cursor theme, an Aurorae window decoration, a Plasma style and two
wallpapers. Panel changes are documented in README.md as optional manual steps.
"""
authors = ["Morphix Demo Team"]
license = "CC-BY-4.0"
homepage = "https://github.com/morphix-project/morphix"
icon = "previews/icon.png"

[compatibility]
desktop = "kde"
plasma = ">=6.0, <7.0"

[capabilities]
requires = [
    "appearance.color_scheme",
    "appearance.icons",
    "appearance.cursors",
    "appearance.wallpaper",
    "appearance.window_decorations",
    "appearance.plasma_style",
    "settings.kde_global",
    "settings.kwin_effects",
]

[assets.color_schemes]
cupertino-dark = "themes/color-schemes/CupertinoDark.colors"
cupertino-light = "themes/color-schemes/CupertinoLight.colors"

[assets.icon_themes]
cupertino = "icons/Cupertino"

[assets.cursor_themes]
cupertino = "cursors/Cupertino"

[assets.plasma_styles]
cupertino = "plasma/desktoptheme/Cupertino"

[assets.window_decorations]
cupertino = "themes/aurorae/Cupertino"

[assets.wallpapers]
dark = "wallpapers/dark.webp"
light = "wallpapers/light.webp"

[appearance]
color_scheme = "cupertino-dark"
icon_theme = "cupertino"
cursor_theme = "cupertino"
cursor_size = 24
plasma_style = "cupertino"
window_decoration = "cupertino"
wallpaper = "dark"

[[settings]]
target = "kdeglobals"
group = "General"
key = "font"
value = "Noto Sans,10,-1,5,50,0,0,0,0,0"

[[settings]]
target = "kdeglobals"
group = "General"
key = "fixedWidthFont"
value = "Noto Sans Mono,10,-1,5,50,0,0,0,0,0"

[[kwin.effects]]
id = "blur"
enabled = true

[[previews]]
image = "previews/desktop-dark.webp"
caption = "Dark composition"

[[previews]]
image = "previews/desktop-light.webp"
caption = "Light composition"

[integrity]
algorithm = "sha256"
manifest = "checksums.sha256"
```

## 5. Field reference

### 5.1 Top level

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `schema_version` | integer | yes | Must be exactly `1` in V0.1. Any other value is `E_SCHEMA_VERSION_UNSUPPORTED`. |
| `pack` | table | yes | §5.2 |
| `compatibility` | table | yes | §5.3 |
| `capabilities` | table | yes | §5.4 |
| `assets` | table | yes | §5.5; may be empty only if no asset capability is requested |
| `appearance` | table | no | §5.6; at least one field if present |
| `settings` | array of tables | no | §5.7 |
| `kwin.effects` | array of tables | no | §5.8 |
| `shortcuts` | array of tables | no | §5.9 |
| `previews` | array of tables | yes | §5.10; 1..8 entries |
| `integrity` | table | no | §5.11 |
| `dependencies` | table | no | §5.12 |
| `layout`, `panel`, `scripts`, `hooks`, `permissions` | — | **never** | Unknown field rejection; these names are reserved and always invalid in schema v1 |

### 5.2 `[pack]`

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `id` | string | yes | Lowercase reverse-DNS: `^[a-z][a-z0-9]*(\.[a-z0-9][a-z0-9-]*)+$`, 1..128 bytes, at least one dot. Must not start with `org.kde.` or `dev.morphix.reserved.`. |
| `name` | string | yes | 1..80 characters, no control characters, trimmed, unique per catalog. |
| `version` | string | yes | Strict SemVer 2.0.0 (`MAJOR.MINOR.PATCH[-pre][+build]`), 1..64 bytes. |
| `summary` | string | yes | 1..200 characters, single line. |
| `description` | string | yes | 1..8192 characters; Markdown allowed, rendered as text with no HTML. |
| `authors` | array of strings | yes | 1..16 entries, each 1..128 characters. |
| `license` | string | yes | SPDX expression string; validated to be a syntactically valid SPDX ID or expression. |
| `homepage` | string | no | `https://` URL, display-only, never fetched. |
| `icon` | string | yes | Path under `previews/`, PNG/WebP/JPEG, ≤ 1 MiB. |

### 5.3 `[compatibility]`

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `desktop` | string | yes | Exactly `"kde"` in schema v1. Future values require a schema version bump. |
| `plasma` | string | yes | SemVer range expression, e.g. `">=6.0, <7.0"`. Must not admit any version `< 6.0.0`; must admit at least one version below `7.0.0`. |

The range is evaluated against the detected runtime Plasma version. A pack that admits
6.0–6.9 but is tested only on 6.3 is *compatible* on 6.4; releases may additionally carry
a tested-version note in `README.md`, but the manifest cannot claim tested versions it has
not declared. Evaluated with the `semver` crate after normalizing Plasma versions to
`MAJOR.MINOR.PATCH`.

### 5.4 `[capabilities]`

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `requires` | array of strings | yes | 1..9 unique capability IDs from §6, sorted or unsorted. Unknown ID → `E_CAPABILITY_UNKNOWN`. A resource used without its capability → `E_CAPABILITY_UNDECLARED`. A capability requested but not exercised → `E_CAPABILITY_UNUSED`. |

### 5.5 `[assets]`

Each sub-table is optional: `color_schemes`, `icon_themes`, `cursor_themes`,
`plasma_styles`, `window_decorations`, `wallpapers`.

| Asset kind | Key type | Value | Structural rules |
| --- | --- | --- | --- |
| `color_schemes` | asset key | path to a single `.colors` file | File extension `.colors`; valid INI with `[General]` section |
| `icon_themes` | asset key | path to a directory | Directory must contain `index.theme`; files must be `.svg`, `.png`, `.theme`, `.txt` |
| `cursor_themes` | asset key | path to a directory | Directory must contain a `cursors/` subdirectory with ≥ 1 file |
| `plasma_styles` | asset key | path to a directory | Non-empty directory; KDE structural requirements are checked by the adapter |
| `window_decorations` | asset key | path to a directory | Non-empty directory; adapter validates Aurorae structure |
| `wallpapers` | asset key | path to a single image | `.png`, `.jpg`, `.jpeg`, or `.webp` |

Asset key rules: `^[a-z][a-z0-9-]{0,63}$`, unique within its kind. Keys are stable
identifiers referenced by `[appearance]`. The on-disk name is engine-derived as
`<pack-slug>-<asset-key>`; packs cannot choose it. `pack-slug` is the last dotted segment
of `id`, lowercased, with non-alphanumerics replaced by `-`.

Directory assets are copied recursively. A directory asset may not include a nested
`pack.toml`, `checksums.sha256`, or another pack's `ATTRIBUTION.md`.

### 5.6 `[appearance]`

All fields are optional, at least one is required if the table is present. Each field must
reference an asset key of the matching kind, and the corresponding capability must be
requested.

| Field | Type | References | Engine effect |
| --- | --- | --- | --- |
| `color_scheme` | string | `assets.color_schemes` | Install `.colors`, write `kdeglobals [General] ColorScheme` |
| `icon_theme` | string | `assets.icon_themes` | Install tree, write `kdeglobals [Icons] Theme` |
| `cursor_theme` | string | `assets.cursor_themes` | Install tree, write `kdeglobals [General] cursorTheme` |
| `cursor_size` | integer | — | Write `kdeglobals [General] cursorSize`; 16..=128 |
| `plasma_style` | string | `assets.plasma_styles` | Install tree, write `plasmarc [Theme] name` |
| `window_decoration` | string | `assets.window_decorations` | Install tree, write `kdeglobals [org.kde.kdecoration2] theme` and `library=org.kde.kwin.aurorae` |
| `wallpaper` | string | `assets.wallpapers` | Install image, set wallpaper through the adapter (gated, ADR-015) |

`cursor_size` requires `cursor_theme`. A field referencing an undefined key is
`E_ASSET_MISSING`.

### 5.7 `[[settings]]`

```toml
[[settings]]
target = "kdeglobals"   # kdeglobals | plasmarc | kwinrc
group = "General"
key = "font"
value = "Noto Sans,10,-1,5,50,0,0,0,0,0"
```

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `target` | string | yes | One of `kdeglobals`, `kwinrc`. Other files (including `plasmarc` and `kglobalshortcutsrc`) are rejected (`E_CONFIG_TARGET_UNSUPPORTED`). |
| `group` | string | yes | `^[A-Za-z0-9][A-Za-z0-9 _.-]{0,127}$`, no brackets. |
| `key` | string | yes | Same pattern; must be on the engine allowlist for that target/group (`E_CONFIG_KEY_NOT_ALLOWED`). |
| `value` | string / integer / boolean / array of strings | yes | Type must match the allowlist entry. Strings ≤ 4096 bytes; arrays ≤ 64 items, each ≤ 1024 bytes. |

Duplicate `(target, group, key)` entries are `E_DUPLICATE_KEY`. Settings may not target
`kglobalshortcutsrc` (use §5.9) or `plasmarc` (use `[appearance].plasma_style`).

Initial allowlist (code-owned; changing it requires tests and docs updates):

| Target | Group | Keys |
| --- | --- | --- |
| `kdeglobals` | `General` | `font`, `fixed`, `fixedWidthFont`, `menuFont`, `toolBarFont`, `smallestReadableFont` |
| `kwinrc` | `Plugins` | `<allowlisted-effect>Enabled` only |
| `kwinrc` | `Effect-<allowlisted-effect>` | allowlisted effect keys only |

Appearance-managed keys (`ColorScheme`, `cursorTheme`, `cursorSize`, `[Icons] Theme`,
`[org.kde.kdecoration2] theme`, and `plasmarc [Theme] name`) are engine-owned and cannot be
set through `[[settings]]`; attempting it is `E_CONFIG_KEY_NOT_ALLOWED`. They are reachable
only through `[appearance]`, which requires the matching capability.

A pack must not declare both an `[appearance]` field and a `[[settings]]` entry that
resolve to the same target key (`E_DUPLICATE_TARGET`).

### 5.8 `[[kwin.effects]]`

```toml
[[kwin.effects]]
id = "blur"
enabled = true

[[kwin.effects]]
id = "wobblywindows"
enabled = false
```

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `id` | string | yes | Must be on the KWin effect allowlist. |
| `enabled` | boolean | yes | Desired state. |

Initial effect allowlist (verified during the Phase 6 prototype; code-owned):
`blur`, `contrast`, `kwin4_effect_dimscreen`, `kwin4_effect_fadingpopups`,
`kwin4_effect_scale`, `magiclamp`, `slidingpopups`, `wobblywindows`, `zoom`.
Effect IDs not on this list are rejected because unknown IDs can silently do nothing or
refer to third-party plugins with unreviewed behavior.

Effects are applied by writing `<id>Enabled` in `kwinrc [Plugins]` and calling
`org.kde.KWin.reconfigure()` over DBus. Effects that are not installed on the machine are
reported as a plan incompatibility, not applied blindly.

### 5.9 `[[shortcuts]]`

```toml
[[shortcuts]]
component = "kwin"
action = "Show Desktop"
keys = ["Meta+D"]
```

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `component` | string | yes | Must be on the shortcut component allowlist (initially `kwin`, `plasmashell`). |
| `action` | string | yes | Must be on the action allowlist for that component. |
| `keys` | array of strings | yes | 1..4 chords; each chord matches `^([A-Za-z0-9]+\\+)*[A-Za-z0-9]+$`. Modifiers limited to `Meta`, `Ctrl`, `Alt`, `Shift`. |

Applied by writing `kglobalshortcutsrc` and synchronizing through
`org.kde.kglobalaccel`. Shortcuts are never cleared in schema v1 (no empty `keys`), because
silently unbinding a user's shortcut is a destructive change with poor visibility.

### 5.10 `[[previews]]`

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `image` | string | yes | Path under `previews/`, PNG/WebP/JPEG, ≤ 5 MiB |
| `caption` | string | yes | 1..120 characters |

Preview images are display-only. They are never loaded into the apply pipeline and never
used as assets. At least one preview is required for local installation; bundled packs must
provide one icon plus at least two desktop screenshots.

### 5.11 `[integrity]`

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `algorithm` | string | yes if section present | Exactly `"sha256"` in schema v1 |
| `manifest` | string | yes if section present | Must be `"checksums.sha256"` in schema v1 |

If the section is absent, the pack has no author-provided checksums. On local install,
Morphix computes a content hash at install time, stores it in the local pack index with
`integrity_source = "generated_local"`, and the UI labels the pack **Unverified**. The
pack's own files are never rewritten. If the section is present, the file must exist and
cover every pack file.

### 5.12 `[dependencies]`

```toml
[[dependencies.packs]]
id = "dev.example.base"
version = "^0.1.0"
```

| Field | Type | Required | Rules |
| --- | --- | --- | --- |
| `id` | string | yes | Pack ID syntax; must not equal this pack's ID |
| `version` | string | yes | SemVer range; must match an installed version |

V0.1 behavior: dependencies are parsed and validated; a dependency must be **installed and
applied** before the dependent pack can be planned, otherwise `E_DEPENDENCY_MISSING`.
Dependency cycles are rejected at install time (`E_DEPENDENCY_CYCLE`). Dependencies do not
cause automatic installation. Optional dependencies and conflict declarations are not in
schema v1.

## 6. Capabilities

Exactly fifteen IDs exist. The table is identical to [MVP.md](MVP.md) §4 and
[ARCHITECTURE.md](ARCHITECTURE.md) §6. All fifteen were exercised on a live Plasma 6.8
session ([CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md)).

| Capability ID | Grants |
| --- | --- |
| `appearance.color_scheme` | Install and select one color scheme |
| `appearance.icons` | Install and select one icon theme |
| `appearance.cursors` | Install and select one cursor theme |
| `appearance.plasma_style` | Install and select one Plasma style |
| `appearance.window_decorations` | Install and select one window decoration |
| `appearance.wallpaper` | Install one wallpaper and set the current background |
| `appearance.look_and_feel` | Install and apply one global theme, decomposed into primitive operations |
| `appearance.fonts` | Write allowlisted font keys |
| `panel.properties` | Set edge, height, alignment, floating and hiding on existing panels |
| `panel.widgets` | Add, configure and remove allowlisted widgets |
| `panel.create` | Create and remove a panel |
| `settings.kde_global` | Write allowlisted `kdeglobals` keys |
| `settings.kwin_effects` | Toggle/configure allowlisted KWin effects |
| `settings.shortcuts` | Set allowlisted global shortcuts |
| `settings.desktop_behavior` | Change allowlisted KWin behaviour keys and virtual desktops |

Capabilities describe *classes* of change, never file paths. A pack cannot request "write
`~/.config/kdeglobals`", only `settings.kde_global`, and even then only allowlisted keys.

Two capabilities are deliberately **absent**, because testing found no mechanism that
works reliably: panel opacity and cursor size. A pack requesting them fails validation with
`E_CAPABILITY_UNKNOWN`; it is not silently downgraded to a no-op.

## 7. Checksums

`checksums.sha256` uses the standard `sha256sum` text format:

```
9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08  wallpapers/dark.webp
2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae  themes/color-schemes/CupertinoDark.colors
```

Rules:

- One entry per line: 64 lowercase hex characters, two spaces (or ` *`), relative path.
- Every file in the pack except `checksums.sha256` itself must appear exactly once.
- No duplicate paths, no absolute paths, no `..`, no entries for missing files.
- Extra entries → `E_CHECKSUM_MALFORMED`; missing entries → `E_CHECKSUM_INCOMPLETE`.
- Hash mismatch → `E_CHECKSUM_MISMATCH`; installation is refused.
- Checksums prove integrity relative to the manifest, not authenticity. Signing is
  post-MVP (see [DECISIONS.md](DECISIONS.md) ADR-006 and PRD §17).

## 8. Validation pipeline

Validation runs at install time and again at plan time (cheap content-hash check only; the
full pipeline runs on install and on catalog load for bundled packs).

```
read file with size cap
  -> UTF-8 decode (E_MANIFEST_ENCODING)
  -> TOML parse (E_MANIFEST_SYNTAX)
  -> schema_version gate (E_SCHEMA_VERSION_UNSUPPORTED)
  -> strict struct deserialization (E_UNKNOWN_FIELD / E_MISSING_FIELD / E_TYPE_MISMATCH)
  -> field-level checks (IDs, versions, strings, limits)
  -> capability resolution (unknown / undeclared / unused)
  -> asset path safety (absolute, traversal, symlink, file type, extension)
  -> asset existence and structural checks
  -> cross-references (appearance keys, settings allowlist, effects, shortcuts)
  -> compatibility expression sanity (desktop, plasma range)
  -> dependencies (syntax, cycles at install)
  -> checksums (if declared): parse, completeness, match
  -> referenced-file audit (no unreferenced files)
  -> canonical manifest hash
```

Any error aborts with all collected errors (up to 100) and writes nothing.

### Validation error codes

| Code | Condition |
| --- | --- |
| `E_MANIFEST_TOO_LARGE` | `pack.toml` exceeds 256 KiB |
| `E_MANIFEST_ENCODING` | Not valid UTF-8 |
| `E_MANIFEST_SYNTAX` | TOML parse error |
| `E_SCHEMA_VERSION_UNSUPPORTED` | Missing or not `1` |
| `E_UNKNOWN_FIELD` | Unknown or reserved field |
| `E_MISSING_FIELD` | Required field absent |
| `E_TYPE_MISMATCH` | Wrong TOML type |
| `E_LIMIT_EXCEEDED` | Size/count/length limit exceeded |
| `E_INVALID_ID` | Pack ID invalid or reserved |
| `E_INVALID_VERSION` | Version not strict SemVer |
| `E_INVALID_PATH` | Asset path invalid, absolute, or outside allowed dirs |
| `E_PATH_TRAVERSAL` | `..` component or escape after normalization |
| `E_SYMLINK_REJECTED` | Symlink or hardlink in pack |
| `E_FILE_TYPE_REJECTED` | Not a regular file/directory, or executable bit set |
| `E_ASSET_MISSING` | Referenced asset does not exist or is empty |
| `E_ASSET_TYPE_UNSUPPORTED` | Extension/structure not allowed for kind |
| `E_CHECKSUM_MALFORMED` | Bad checksum line, duplicate, or extra entry |
| `E_CHECKSUM_INCOMPLETE` | A pack file is missing from the checksum manifest |
| `E_CHECKSUM_MISMATCH` | Content does not match declared hash |
| `E_UNREFERENCED_FILE` | Pack file not referenced by manifest/checksums/docs |
| `E_DUPLICATE_KEY` | Duplicate asset key or settings target |
| `E_DUPLICATE_TARGET` | Two declarations resolve to the same target |
| `E_CAPABILITY_UNKNOWN` | Capability ID not in the closed set |
| `E_CAPABILITY_UNDECLARED` | Resource used without requesting its capability |
| `E_CAPABILITY_UNUSED` | Capability requested but nothing uses it |
| `E_CONFIG_TARGET_UNSUPPORTED` | Settings target not in the fixed map |
| `E_CONFIG_KEY_NOT_ALLOWED` | Key not on the allowlist |
| `E_VALUE_INVALID` | Value type/range rejected by the allowlist |
| `E_EFFECT_NOT_ALLOWED` | Effect ID not allowlisted |
| `E_SHORTCUT_NOT_ALLOWED` | Component/action not allowlisted or chord malformed |
| `E_COMPATIBILITY_INVALID` | Desktop not `kde`, or Plasma range admits <6.0 |
| `E_DEPENDENCY_INVALID` | Dependency ID/range invalid |
| `E_DEPENDENCY_CYCLE` | Cycle among packs |
| `E_DEPENDENCY_MISSING` | Dependency not installed/applied at plan time |
| `E_REQUIRED_FILE_MISSING` | `README.md` or `ATTRIBUTION.md` missing |
| `E_PACK_ALREADY_INSTALLED` | Same ID and version already installed |
| `E_PACK_ID_CONFLICT` | Same ID, different version installed (V0.1 refuses upgrades) |

Runtime codes such as `E_PLAN_STALE`, `E_CAPABILITY_DENIED`, `E_ROLLBACK_CONFLICT`, and
`E_APPLY_FAILED` are defined in [TRANSACTIONS.md](TRANSACTIONS.md) and the error taxonomy
in [ARCHITECTURE.md](ARCHITECTURE.md) §12.

### 6.1 New declaration forms

Three capabilities need manifest shapes that did not exist in the original spec:

```toml
[appearance.look_and_feel]
package = "com.example.mytheme"       # installed Plasma/LookAndFeel package id
components = ["colors", "icons", "cursors", "plasma_style", "decorations", "wallpaper"]
# Applying it writes the LookAndFeelPackage marker and deletes conflicting overrides,
# exactly as the reference tool does; every component stays individually reversible.
# --resetLayout is never used: the user's panel layout is preserved.

[panel]
create = false                       # true only for packs that add a panel

[[panel.properties]]
location = "bottom"                  # top | bottom | left | right
height = 44                          # 16..128
alignment = "center"                 # left | center | right
floating = true
hiding = "none"                      # none | autohide | dodgewindows

[[panel.widgets]]
panel = 0                            # index into the pack's panel list, or existing panel id
op = "add"                           # add | configure | remove
type = "org.kde.plasma.digitalclock"
safe = true                          # must be in the curated allowlist

[[desktop_behavior.virtual_desktops]]
op = "add"                           # add | rename | remove
name = "Work"
```

Panel opacity, cursor size, and arbitrary widget types are **not** expressible: the parser
rejects unknown fields, so a pack cannot ask for something the engine cannot do.

## 9. Unsupported actions

Schema v1 has no representation for, and the engine refuses:

- shell commands, scripts, compiled binaries, or interpreter files;
- `install`/`uninstall` hooks or lifecycle scripts;
- absolute paths, symlinks, or writing outside the fixed target map;
- downloading anything or referencing remote assets;
- installing `.desktop` files, services, autostart entries, or systemd units;
- modifying system-wide paths, packages, kernel parameters, or drivers;
- panel configuration or Plasma layouts (reserved for schema v2, ADR-011);
- changing keyboard layouts, input methods, locale, timezone, or account settings;
- screensaver/lock screen configuration (security-sensitive, post-MVP);
- setting environment variables or shell configuration;
- KWin scripts or third-party KWin plugins;
- applying Plasma global look-and-feel packages (ADR-012).

A pack that needs any of these is out of scope for V1; the correct answer is "Morphix
cannot do this", not a workaround.

## 10. Lifecycle

```
authored
   |
   v  validate (schema, assets, checksums, capabilities, compatibility)
validated
   |
   v  install (copy to pack store, pin integrity metadata)
installed
   |
   v  plan (compatibility + capabilities + drift + collisions)
planned ---- discard ----> installed
   |
   v  apply (snapshot -> mutate -> verify -> commit)
applied
   | \
   |  +--> rollback (new transaction, restores pre-apply state) --> installed
   |
   v  upgrade (post-MVP; explicit new transaction)
newer version
```

Rules:

- Only one version of a pack ID may be installed in V0.1.
- Removing an applied pack is impossible; roll it back first (removal itself is post-MVP).
- Re-applying an applied pack is refused as a no-op with `E_PACK_ALREADY_APPLIED`; users
  should roll back first if they want a clean reapply.
- Install never mutates desktop state. Plan never writes. Apply is the only mutation.

## 11. Author checklist

1. Read [SECURITY.md](SECURITY.md) and this document.
2. Use only original or compatibly licensed assets; record provenance in `ATTRIBUTION.md`.
3. Write `pack.toml` with schema v1 and a truthful compatibility range.
4. Request the minimum capability set; use only allowlisted keys/effects/shortcuts.
5. Include at least one preview, a README, and attribution.
6. Generate `checksums.sha256` as the final step, after all assets are final.
7. Validate with the app's local install flow (the UI surfaces field-level errors).
8. Test on a clean Plasma 6 user account: apply, restart Plasma, check the result, roll
   back, confirm the original state.
