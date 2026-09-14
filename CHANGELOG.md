# Changelog

All notable changes to `pebbles-markdown` are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow [SemVer](https://semver.org).

## [Unreleased]

Completes the feature checklist through **§15** — CommonMark (§1–§2), GFM (§3),
advanced markup (§4), code blocks (§5), Obsidian PKM (§6), rendering & layout
(§7), theming (§8), interaction (§9), editor (§10), performance (§11),
accessibility (§12), security (§13), extensibility (§14), and export (§15).
Remaining items are documented framework blocks or reasoned scope boundaries.

### Packaging & structure (finalization)

- **Domain-split source.** The monolithic `lib.rs` is now a thin crate root over
  one module per concern: `model`, `parse`, `highlight`, `style`, `edit`,
  `widget`, `render`, `editor` (plus the existing `advanced` / `math` / `html`
  helpers). No behavior change — a pure reorganization; see
  [`ARCHITECTURE.md`](ARCHITECTURE.md).
- **Runnable showcase** moved to its own workspace member, `examples/demo`
  (`cargo run -p demo`), with a headless screenshot path
  (`SHOT=<w>:<h>:<out.rgba>`) and a `docs/demo.png`.
- **Project docs**: added `ARCHITECTURE.md`, `CONTRIBUTING.md`, a GitHub Actions
  CI workflow (fmt · clippy · test · doc), and an expanded `README.md`.

### Deepening pass (former scope boundaries)

- **2-D display math** (`src/math.rs`): `$$…$$` now renders through a real box
  layout — stacked fractions, roots with an overline, `\begin{matrix}` grids, and
  raised/lowered super/subscripts — instead of a single Unicode line. Inline
  `$…$` keeps the compact Unicode transform. Panic-free on any TeX subset.
- **`:::type … :::` containers** → typed callouts (`expand_containers`,
  code-fence-aware).
- **Vault resolver** (`Markdown::resolver`): `![[Note]]` embeds now **transclude
  the resolved Markdown inline** (cycle-bounded) in a titled card; embeds/reference
  chips **preview the resolved snippet on hover** (tooltip). Forward resolution for
  wikilinks; backlinks/graph remain a host-side reverse index.
- **Inline-link hover** (`Markdown::on_link_hover`): the pointer over an inline
  `[text](url)` link now reports `Some(url)` (and `None` on leave) for a preview /
  status URL. Backed by a new **framework primitive** — `RichText::on_link_hover`
  in `pebbles-widgets` — which hit-tests the paragraph's laid-out link boxes.
- **Selectable text** (`Markdown::selectable()`): drag to select rendered text,
  copied to the clipboard on release. Backed by **new framework support** in
  `RenderParagraph` (selection painting via parley `Selection` + point→byte
  hit-testing published through `text_edit`) and `RichText::selectable()` /
  `on_selection`. **Cross-block selection**: a drag — and the copy on release —
  now spans **across** paragraphs/blocks (a document-wide `SelectionGroup` maps a
  global point to whichever leaf is under the pointer and concatenates the covered
  slices). Backed by the new `SelectionGroup` / `RichText::selectable_in(group,
  index)` framework primitives. (Opt-in — a drag otherwise scrolls.)
- **Keyboard link traversal** (`Markdown::keyboard_links()`, a11y): link-bearing
  paragraphs become Tab focus stops; the arrow keys cycle their links with a
  visible focus ring and Enter/Space activates the focused link through `on_link`.
  Arrowing past the first/last link releases focus so Tab moves on. Backed by a new
  **framework primitive** — `RichText::keyboard_nav()` + a `RenderParagraph`
  keyboard-focus ring keyed by link span.
- **Custom per-block renderer** (`Markdown::block_renderer(|kind, text| …)`):
  override how any block draws, keyed by kind (`"heading"`, `"code"`, `"callout"`,
  …); return `None` to fall through.
- **Backlinks** (`Markdown::backlinks([(label, target)])`): renders a clickable
  Backlinks section at the document end from host-supplied vault data.

### §12 Accessibility

- **Screen-reader semantics** per block: headings (`Label`, "Heading N, …"),
  fold headings (`Button`, expanded/collapsed), lists (`List` / `ListItem`), and
  images (`Image` with caption/alt). Parent-data annotations — zero layout impact.

### §13 Security / sanitization

- **Link-scheme allowlist** (default on): only `http(s)`, `mailto`, `tel`,
  `ftp(s)`, `sms`, and scheme-less links are clickable; `javascript:`, `data:`,
  `vbscript:`, `file:` … render as inert text. `Markdown::allow_all_link_schemes()`
  opts out.

### §14 Extensibility & integration

- **Fenced-block handler**: `Markdown::fenced_handler(|lang, code| Option<AnyWidget>)`
  renders a widget in place of a code block (map ```` ```mermaid ```` to a diagram,
  wire any custom fenced language) — no engines baked into the core.

### §15 Export / output

- **HTML export**: `to_html(source)` renders the CommonMark + GFM/extension set to
  an HTML string (via pulldown's `html` feature), front matter stripped and emoji /
  `%% comments %%` resolved first.

### §10 Editor (live editing)

- **Formatting source-transforms** (public API): `wrap_selection` toggles inline
  markers (`**` bold, `*` italic, `~~` strike, `` ` `` code) around a byte range;
  `toggle_line_prefix` adds/removes a line prefix (`> `, `- `, `# `, …) across the
  touched lines. Both return the new source + adjusted selection — the reusable
  core of a formatting toolbar / keyboard shortcuts, driven by a selection-aware
  edit pane (the bundled `text_area` exposes no selection; wire them to
  `pebbles-code-editor`).
- **Editor preview** now honors `markdown_editor(src).find(query)` and
  `.collapsible_headings()` (pass-through to the rendered pane).
- Edit↔preview scroll sync, WYSIWYG, and using `pebbles-code-editor` as the edit
  pane stay host compositions (no cross-package dep) — documented in the checklist.

### §9 Interaction

- **Fold-on-click headings**: `Markdown::collapsible_headings()` makes headings
  clickable (chevron) to collapse the section beneath them — down to the next
  heading of the same or a higher level. Off by default; not applied in
  virtualized mode.
- **Find in document**: `Markdown::find(Signal<String>)` highlights every
  case-insensitive match of a live query (`find_bg`/`find_fg`, both themeable).
  The host owns the query field and scroll viewport; the reader highlights.
- Anchor-scroll (`#slug`) and note-hover previews stay host-owned (an embedded
  reader doesn't own its scroll viewport; previews need vault content) —
  documented in the checklist.

### §7 Rendering & layout

- **Image sizing + captions**: an image's alt text parses an optional size
  (`200`, `200x100`, or `caption|200`); a plain alt renders as a centered caption
  below the image. Network images load asynchronously via `ImageView`.
- **Wide tables scroll**: tables now use the framework's `layout_table` for
  content-sized, alignment-aware columns (capped at `IntrinsicMax(420)`) wrapped
  in a horizontal `ScrollView`, so a wide table scrolls instead of squishing.
- Selectable/copyable rendered text remains framework-blocked (no cross-widget
  text-selection surface yet); code blocks still offer a copy button (§5).

### §6 Obsidian-flavored (PKM)

- **Wikilinks** (`ENABLE_WIKILINKS`): `[[Page]]`, `[[Page|alias]]`, and
  `[[Note#Section]]` become clickable links (the app resolves the target via
  `on_link`).
- **Embeds** `![[…]]`: image embeds render as images; note embeds render as a
  clickable reference chip (full content transclusion needs a vault resolver).
- **Tags** `#tag`: in-tree detection → themeable chip (`tag_bg`/`tag_fg`),
  clickable; `#123` (no letter) is not a tag.
- **Comments** `%% … %%`: inline comments are stripped from reading view.
- Frontmatter renders as a read-only **properties panel** (from §1).
- Vault-dependent features (block references, backlinks/graph, note-content
  transclusion) remain out of scope — documented in the checklist.

### §5 Code blocks

- **Header bar** on fenced code blocks: the language label on the left, a
  **copy-to-clipboard** button on the right (writes the raw code via
  `pebbles_core::clipboard`; flashes a check mark for ~1.5s).
- **Line numbers**: opt-in via `MarkdownStyle::code_line_numbers` — a right-aligned
  gutter rendered through a per-line path (the default single-shaped-paragraph path
  is unchanged when line numbers are off and no lines are highlighted).
- **Highlighted lines**: a `{2,4-6}` group in the fence info string marks lines,
  rendered with a subtle background.
- Reusing the `pebbles-code-editor` grammars stays a deliberate non-goal — this
  crate ships its own dependency-free highlighter so it depends only on the
  framework's public API.

### §4 Extended / advanced markup

- **Callouts / admonitions**: `> [!NOTE|TIP|IMPORTANT|WARNING|CAUTION]` render as
  typed callouts (accent bar, icon, title) via `ENABLE_GFM` blockquote kinds.
- **Math**: `$…$` / `$$…$$` (`ENABLE_MATH`) render through a heuristic TeX→Unicode
  transform (Greek, operators/relations, digit super/subscripts, `\frac`, `\sqrt`).
  Not a full typesetter — complex layouts are approximated; unknown commands
  degrade to readable text.
- **Definition lists** (`ENABLE_DEFINITION_LIST`), **`==highlight==`** (themeable
  `mark_bg`/`mark_fg`), **subscript `~x~` / superscript `^x^`**
  (`ENABLE_SUBSCRIPT`/`ENABLE_SUPERSCRIPT`), and **smart punctuation**
  (`ENABLE_SMART_PUNCTUATION`).
- **Emoji shortcodes** (`:rocket:` …) via an in-tree map of ~90 common codes.
- **Heading anchors**: GitHub-style deduped slugs per heading, plus an auto
  **table of contents** from a `[toc]` / `[[toc]]` marker.
- **Collapsible `<details>`/`<summary>`**: a stateful disclosure that spans
  multiple markdown blocks between the tags.
- New `src/advanced.rs` (emoji map, TeX→Unicode, slugify) with its own unit tests.
- Scope boundaries (documented in the checklist, not implemented): full math
  typesetting, live mermaid/diagram rendering, non-standard `:::` containers, and
  abbreviations.

### §3 GFM extensions

- **Table column alignment**: the `:---` / `:--:` / `---:` markers in a table's
  header row are captured per column and applied (`TextAlign::Left/Center/Right`)
  to both header and body cells.
- **Footnotes** (`ENABLE_FOOTNOTES`): references render as superscript markers
  numbered by first-reference order; definitions are collected into a `Footnotes`
  section rendered at the end of the document (referenced first, then any
  defined-but-unreferenced ones).
- (Autolink literals were delivered in §2.)

### §2 CommonMark — inline

- **Bare-URL / email autolinks** (GFM extended autolinks): plain-text
  `http(s)://…`, `www.…`, and `foo@bar.tld` become links (with `http://` prefixed
  for `www.` and `mailto:` for emails). Trailing punctuation and an unbalanced
  closing `)` are trimmed; matching is skipped inside inline code, inside an
  explicit link, and mid-word. (pulldown-cmark 0.13 has no autolink option, so
  this is done in-tree.)
- **Inline HTML** (`<b> <i> <del> <code> <a href> <br> <img>`) now drives the
  inline formatting state and renders, instead of showing as escaped text.

### §1 CommonMark — blocks

- **Loose vs tight lists**: a list is detected as loose when a paragraph is the
  direct child of one of its items; loose lists now render with block-level
  spacing between items, tight lists stay compact.
- **HTML blocks**: raw HTML is parsed to native blocks by a new dependency-free,
  panic-free subset parser (`src/html.rs`) instead of leaking as escaped inline
  text — `<p> <h1>…<h6> <hr> <blockquote> <ul>/<ol>/<li> <pre> <img>` plus inline
  `<b>/<strong> <i>/<em> <del>/<s> <code> <a href> <br>`. HTML comments are
  dropped, `&…;` entities decoded, and **unknown tags are transparent** (dropped,
  their text kept). Nothing is executed or fetched — tags are inert markup.
- **Front matter**: leading `---` (YAML) / `+++` (TOML) front matter is parsed
  into a compact properties block and stripped from the rendered body; a
  setext-`---` look-alike with no `key: value` pairs is left intact.
- Tests: six new lib units cover the above (tight/loose, front-matter parse +
  strip, the setext guard, HTML block structure, inline-HTML formatting, and a
  malformed/unknown-HTML safety case).

## [0.1.0]

Initial release as a standalone package. The Markdown reader +
editor was extracted from `pebbles-widgets` (where it had lived behind a
`markdown` feature) into its own crate — the reference example of a third-party
Pebbles widget package. No behavior change from the in-tree version; it depends
only on Pebbles' public API.

- **Reader** (`markdown`): GFM via pulldown-cmark — headings, emphasis, inline +
  fenced code with syntax highlighting, links, nested quotes/lists, task lists
  with live checkboxes that rewrite the bound source, tables, rules, images.
- **Editor** (`markdown_editor`): `Signal<String>`-bound, View / Edit / Split
  modes driven by a caller-owned mode signal; split preview debounced.
- **Virtualized reader** (`markdown(..).virtualized()`): viewport-bounded — only
  on-screen blocks are built.
- **Robust by contract**: any input renders; malformed or non-ASCII content
  (accented identifiers, em-dashes, emoji, CJK in code blocks) degrades to plain
  text instead of freezing — the syntax highlighter walks whole characters, never
  raw bytes. Covered by `tests/robustness.rs`.
- Tests carried over intact: parse/highlight units, task-toggle + live re-render,
  the multibyte interaction storm, and the huge-document performance tripwires.
- Constructors follow the D10 lowercase-function convention throughout (both the
  library and the example): `container()` / `expanded()` / `padding()` /
  `gesture_detector()`, never `Type::new()` — the reference package models the
  one call-site convention cleanly.
- **Showcase sample** (`examples/reader.rs`): reader + editor with View / Edit /
  Split modes, themeable style variants (default / serif / compact with custom
  syntax palettes), and the virtualized reader on a deterministic multi-megabyte
  stress document — the widget's own standalone demo (the gallery no longer
  ships a Markdown screen).
