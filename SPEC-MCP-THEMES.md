# Spec: MCP server (pass 1) and app themes (pass 2)

Handoff for the implementing agent. Core behaviour does not change: no change to the engine, playback, pause rules, downloads or storage formats beyond what is listed here. Read `AGENTS.md` first; its rules (no agent credit in git, no apologetic public text, small modules, one release per change) apply to both passes. Pass 1 = `feat` → minor release. Pass 2 = `feat` → minor release.

---

## Pass 1 — MCP server

**Built (see AGENTS.md "MCP").** What changed from the plan below: the server is an on/off switch on its own MCP page (it never runs unless the user turned it on, and the top bar shows "MCP on"), and besides stdio it serves MCP over local HTTP, a plain JSON API, a web page for browser agents and a shell command. Hidden items are reachable with `include_hidden`. Still open from this pass: theme tools (pass 2), listing widget looks.

### Goal
Anyone with an agent (Claude Code, OpenCode, Cursor, …) can do from chat what they do in the GUI. Example: "import this video link" → the agent downloads the file itself when Paperloom has no reader for that site, then calls `import_file`.

### Shape
- New crate `crates/paperloom-mcp`, binary `paperloom-mcp`, MCP over **stdio**. Depends on `paperloom-common` and `paperloom-fetch` only. Never on GUI or engine code, and the engine never depends on it.
- Works with the GUI closed. It talks to the engine through the existing IPC (`ipc::send`, as `cli.rs` does) and reads/writes the same files the GUI uses (`library.toml`, `config.toml`, `rotation.toml`, `widgets.toml`, `engine-state.toml`).
- Ships next to the other binaries in every package (deb, rpm, flatpak, msi, exe, dmg). `process::find` rules apply.
- **Prerequisite refactor:** `optimizer.rs` (re-encode after import) currently lives in the GUI crate. Move it to a shared place so MCP imports get the same copy → fingerprint-dedupe → optimize pipeline as GUI imports. Same for the import logic itself (copy, fingerprint, `media::plan`, collections). One code path, two front ends.
- **File safety with a running GUI:** both processes write `library.toml`. Writes must be atomic and locked (lock file), and the GUI must reload when the file's mtime changes (cheap `stat` on its existing tick, not a watcher thread). Check what the GUI does today before assuming.

### Tools (names are stable API; keep descriptions short and agent-readable)
**Wallpaper:** `status`, `set_wallpaper` (name, id or path), `next`, `pause`, `resume`, `mute`, `unmute`, `stop`.
**Library:** `list_wallpapers` (query, collection, favorites, tag, paging), `get_wallpaper`, `import_file` (path; honours the user's import mode; returns the id, or "already in your library as …"), `import_url` (direct video link or a page one of the built-in readers understands; if no reader can take it, return a clear error telling the agent to download the file itself and call `import_file`), `remove_wallpaper` (needs `confirm: true`), `set_favorite`, `set_tags`.
**Collections:** `list_collections`, `create_collection`, `rename_collection`, `delete_collection` (`confirm: true`), `add_to_collection`, `remove_from_collection`.
**Online:** `search_online` (query, sources, mature filter obeys the user's setting), `download_online` (item id + HD/4K, lands in the normal queue), `list_downloads`, `pause_downloads`, `resume_downloads`.
**Rotation:** `get_rotation`, `set_rotation` (collection, interval, shuffle, schedule rules).
**Settings:** `get_settings` (API keys, lock hash and any secret removed), `set_setting` (allow-list of keys only; the allow-list is a table in the crate, so adding a key is one line).
**Widgets:** `list_widgets`, `add_widget`, `move_widget`, `remove_widget`, `set_widget_look`, `list_looks`, `widgets_on/off`.
**Themes (added in pass 2, listed here so the surface is designed once):** `list_themes`, `get_theme_schema`, `validate_theme`, `install_theme` (folder, file or URL), `apply_theme`, `reset_theme`. An agent can write a theme file from a description, validate it against the schema and apply it.
**Resources:** `paperloom://status`, `paperloom://theme-schema` (pass 2), `paperloom://library/summary`.

### Rules
- **Hidden items are reachable.** The Hidden section's password is a courtesy gate in the UI, not security (nothing is encrypted), so MCP does not treat it as one. Lists and searches leave hidden wallpapers out unless the call passes `include_hidden: true`, which keeps an agent's normal answers tidy. The password hash itself is never returned.
- Removing or deleting anything requires `confirm: true` and says exactly what will go.
- No tool reads arbitrary files. `import_file` reads only the file path it is given, and only video/image types `media::plan` accepts.
- No secrets out, ever (API keys, hidden lock). `set_setting` cannot set them either.
- Errors are plain sentences an agent can act on ("No wallpaper matches “x”", "Paperloom has no reader for this site; download the file and use import_file").

### Settings → AI agents (new card, in the settings registry so search finds it)
- Copyable config snippets for Claude Code (`claude mcp add paperloom -- <path>`), OpenCode, Cursor/others (JSON). The path is filled in from `current_exe()`.
- Permission switches stored in `mcp.toml`, read by the server on every call (no restart): **Allow changes** (default on), **Allow removing** (default off), **Allow changing settings** (default off). With "Allow changes" off the server is read-only.
- Last few calls shown as a short activity list (the server appends to `logs/mcp.log`; 512 KB rotation like other logs).
- Wording follows the public-text rule: state what exists, no caveats.

### Done when
`cargo build`, `clippy`, unit tests for each tool's handler against scratch temp dirs (like `paperloom-common`'s tests), and one scripted stdio session (initialize → tools/list → status → import_file → list_wallpapers). README gets a short "Use with an AI agent" section.

---

## Pass 2 — Themes and the Appearance page

**Built (see AGENTS.md "App themes").** Differences from the plan below: `.paperloomtheme` is a text bundle in the share-code style, not a zip (no new dependency); density is one whole-UI `scale`; restylable parts are nav, top_bar, the four buttons, modal, screen and popup (cards follow the general colours and `card_radius`); a theme's text font applies after a restart. Still open: the live headless preview in the editor, an icon-pack override, per-part padding.

### Principle
The default look stays exactly as it is. It is rewritten as a built-in theme in the new format (a test checks the built-in "Paperloom" theme produces the same tokens as today's `theme.rs`). Everything after that is data, not code: no scripting, no web view, a validated declarative file, the same philosophy as looks/skins.

### Crate and format
- New crate `crates/paperloom-theme` (no iced dependency, like `paperloom-skin`): file format, parser, validator, schema text, built-in themes, defaults for omitted fields. The GUI converts a resolved theme to iced styles.
- A theme is a folder: `theme.toml`, optional `fonts/` (TTF/OTF, OFL only, licence file alongside), `images/` (PNG), `preview.png`. Same install rules as looks: only `.toml/.ttf/.otf/.png/.txt`, ≤12 MB, every file checksummed.
- Installed in `paperloom_common::themes_dir()`. Share/import forms: folder, a single `theme.toml`, and `.paperloomtheme` (a zip of the folder; double-click/drag-drop opens the import, like `.paperloomshare`).
- Every field is optional; missing fields fall back to the default theme. Unknown fields are ignored with a warning in the validator report (so newer themes load on older apps).

### What a theme can change (all of it, in layers)
1. **Colours.** The 10 current tokens plus: card, hover, selected, overlay (modal scrim), focus ring, accent-2, shadow colour, link. Separate `[colors.light]` and `[colors.dark]`; a theme may supply one mode only (then the mode switch is locked to it).
2. **Typography.** Body font, display font (bundled files or installed system family names), size scale (title / section / body / small), weight rules, line height.
3. **Shape.** Corner radius for controls / cards / pop-ups / screens, border width, shadow strength, density (compact / comfortable / spacious) which scales all padding and spacing, scrollbar style, switch style, button style (filled / soft / outline).
4. **Layout.** (There is no limit on what a theme may restyle: backdrop, roundness, borders of anything, pop-up looks, spacing, all of it.)
   - Navigation position: **left, right, top, bottom**; style: full sidebar, icon rail, tabs, or hidden behind a menu button; width; sections collapsed or open.
   - Top bar: visible or not, where the title sits, where the search box sits (top bar, inside the nav, or a floating button/shortcut), where the Downloads and Update buttons sit.
   - Content: max width, page margins, default gallery card ratio and gap.
   - Window backdrop: solid colour, gradient or PNG image with opacity.
   - **Image layers:** any number of PNG images placed anywhere (corner, edge, centre, tiled), with size, opacity, and behind or in front of the content, on every page or chosen pages. A flying snake in the background is a layer. Animation = a PNG frame sequence played at a set fps (the same frame-run technique the gallery hover previews use); if iced allows animated GIF/WebP cheaply, accept those too. Layers in front never take clicks.
5. **Component overrides.** A fixed list of named parts: `nav`, `nav_item`, `top_bar`, `card`, `button.primary|secondary|danger|ghost`, `input`, `chip`, `tab`, `tile`, `toggle`, `modal`, `screen`, `menu`, `search_popup`, `tooltip`, `scrollbar`. Each takes background, foreground, border, radius, shadow, padding. This is how pop-ups, dialogs and screens are themed: they read the same parts as everything else.
6. **Motion.** Animations on/off, duration scale.
7. **Icons** (later in the pass, if it stays small): a folder of SVGs overriding icon names from `icons.rs`.

### Recovery (nothing a theme can do is blocked; the way back always exists)
- Applying a theme that changes the layout shows "Keep this theme?" and reverts by itself after 10 seconds.
- Appearance → "Use the default theme", CLI `paperloom-gui --default-theme`, and an unreadable or invalid theme at startup falls back to the default and logs why.
- The editor warns about low contrast but never prevents it.
- A theme never changes the desktop widgets' looks (separate system) or the wallpaper engine. The existing responsive rule (nav folds into the top bar in a very narrow window) stays.

### Appearance page
Reached from a plain row in Settings that opens it ("Appearance →"); the old Appearance card's Light/Dark/System choice moves onto this page. It is **one workspace** (list on the left, details on the right, like Updates), not a long scroll of cards, and deliberately simpler than the Widgets page:
- **Left: two lists in one tab bar.** *On this computer* (built-in, downloaded and imported themes together) and *Download* (the online library). Downloading or importing a theme puts it in *On this computer*. A search box and a thumbnail on every row; the current theme is marked.
- **Right: the chosen theme.** Thumbnail, name, author, **Use**, **Edit**, **Duplicate**, **Remove** (installed ones only), and a short description.
- **Import** button + drag-drop of a folder, `theme.toml` or `.paperloomtheme`; result shown in the usual import pop-up (what loaded, what was ignored).
- **Edit** opens the theme's file as a form inside the same workspace: groups of sliders, colour pickers with hex, dropdowns, number fields and tables (colours, fonts, shape, layout, component parts, image layers), plus a "Raw file" tab for the TOML text. Built-in themes cannot be overwritten: Edit on one makes a copy first. Saving writes the theme's own folder. While editing, **Preview** applies the draft to the whole app (with the 10-second revert).
- Search registry entries for the page's options.
- **Planned later, not in this pass:** rendering the app headlessly inside the editor so the preview updates live without applying the theme. Keep the editor's preview behind one function so this can be swapped in.

There is no separate overrides layer: editing a theme edits that theme.

### Online library
- A `themes/` folder in the repo (one folder per theme) with `themes/index.json`, built by `scripts/build-themes-index.py` (SHA-256 + sizes; validates each theme with the crate's validator). Each theme has a small committed `preview.png` thumbnail (script checks it exists and is small, about 300 KB at most).
- `apt-repo.yml` copies `themes/` to Pages next to `widgets/`. The app reads raw.githubusercontent first, then the Pages copy, like looks. Keep the base URL in one constant (the assets may move to their own repository later; nothing else to prepare).
- Generalise the `paperloom_fetch::looks` installer (checksum, allowed extensions, size cap) so looks and themes share one implementation.
- Ship a few catalogue themes that show off the range: a right-hand sidebar, a top-tabs layout, a compact one, a high-contrast one, one with an animated background layer.

### Refactor needed inside the GUI
- `theme.rs`'s `Tokens` grows into the resolved-theme struct; `ui.rs` helpers (`primary`, `secondary`, `modal`, `screen`, `tile`, …) read component styles from it instead of fixed values.
- `App::layout` (≈ 1,200-line `app.rs`) is split into a `layout/` module: nav (four positions), top bar, content frame, search popup. Today's arrangement becomes the default theme's layout.
- Audit all pages for hard-coded `Color::from_rgb`, radii, padding and font sizes; route them through the theme. This is the biggest part of the pass; do it page by page.
- The tray's pop-up (`widget.rs`) follows the theme's colours, shape and fonts.

### Done when
Default theme renders identically to today; switching theme changes every page, pop-up, dialog and screen; nav can be put on all four sides; an invalid theme falls back safely; build + clippy + unit tests (parser, validator, default equals old tokens, contrast fallback); one screenshot per nav position sent to the maintainer. README and the download site get a short Themes section.

---

## Order and dependencies
1. Pass 1 first: move import/optimizer out of the GUI crate, then `paperloom-mcp`, then the AI agents settings card.
2. Pass 2 second: `paperloom-theme` crate → GUI refactor → Appearance page → online catalogue → MCP theme tools.
3. Nothing in pass 1 depends on themes, so each ships as its own release.
