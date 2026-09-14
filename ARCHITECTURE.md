# Architecture

`pebbles-markdown` is organized **by domain** — each concern lives in its own module with a
single, clear responsibility. `lib.rs` is a thin crate root: crate docs, the module tree, the
three shared callback type aliases, and the public re-exports. Nothing else.

Contributions must keep this shape. When you add a feature, put it in the module that owns that
concern (or add a new domain module) — never grow one file into a grab-bag.

## Pipeline

```
source (&str)
   │  parse.rs  (front matter, :::containers, emoji, tags, comments, autolinks)
   ▼
Vec<Block>   ── model.rs  (Block / Inline / Run / Flags / ListItem …)
   │  widget.rs  (Markdown builder + Cx + orchestration)
   ▼
render.rs  ── highlight.rs (code), math.rs (display math), advanced.rs (emoji/TeX/slug), html.rs
   ▼
themed Pebbles widget tree
```

`edit.rs` is a separate, pure source→source layer (task toggles, formatting helpers). `editor.rs`
composes the reader with a bound `text_area` into the Edit/Split/Read editor.

## Module map

```
src/
  lib.rs        Crate root — docs, module wiring, callback aliases, public re-exports. Keep it thin.

  # ── the document (model) ───────────────────────────────────────────────
  model.rs      The parsed block/inline tree: Block, Inline, Run + Flags, ListItem, CalloutKind,
                CellAlign, MdFrame. Plain data — no parsing, no widgets.

  # ── parsing (source → model) + export ──────────────────────────────────
  parse.rs      pulldown-cmark → Vec<Block> with this crate's option set, plus the in-tree
                extensions: front matter, `:::type` containers, `#tags`, `%% comments %%`,
                bare-URL/email autolinks, emoji, heading slugs, and the `to_html` export.
                Panic-free on any input.

  # ── code highlighting (pure) ────────────────────────────────────────────
  highlight.rs  A small, dependency-free lexer: source → per-line colored spans, from the
                themeable `SyntaxColors`. Whole-char safe; never panics.

  # ── advanced markup (pure helpers) ──────────────────────────────────────
  advanced.rs   Emoji shortcodes (`:smile:`), the inline TeX→Unicode transform, GitHub-style
                heading slugs.
  math.rs       The 2-D display-math layout engine for `$$…$$` (fractions, roots, matrices,
                scripts). Degrades to the Unicode transform on anything it can't structure.
  html.rs       A dependency-free parser for the safe HTML subset (block + inline tags) →
                native blocks/inline. Nothing is executed.

  # ── theming ─────────────────────────────────────────────────────────────
  style.rs      `MarkdownStyle` (every visual knob) + `SyntaxColors`. Build from the live theme
                (light/dark aware) or by hand. The public styling surface.

  # ── source transforms (pure) ────────────────────────────────────────────
  edit.rs       `toggle_task`, `wrap_selection`, `toggle_line_prefix` — the reusable core of a
                formatting toolbar / keyboard shortcuts. Pure string ops over the source.

  # ── the reader widget ───────────────────────────────────────────────────
  widget.rs     The `Markdown` builder, its per-render context `Cx`, and orchestration:
                virtualized vs. eager block building, heading-collapse, find, selection groups,
                keyboard-link groups. The public reading entry point.
  render.rs     Turns each Block/Inline into the themed widget tree — code, callouts,
                `<details>`, tables, images, embeds, footnotes, and the inline flow (emphasis,
                links, tags, math). The place that builds reader widgets.

  # ── the editor ──────────────────────────────────────────────────────────
  editor.rs     `MarkdownEditor` — a `Signal<String>`-bound Edit / Split / Read editor. Ships no
                chrome; the app owns the mode signal. Split preview follows the source (debounced).
```

## Layering (dependencies point downward)

```
editor ─▶ widget ─▶ render ─▶ highlight / math / advanced / html
                      └─▶ style
         widget ─▶ parse ─▶ model
         edit  (standalone, source→source)
         style (leaf)
```

- **`model`, `style`** are leaves — no dependency on parsing or rendering.
- **`parse`, `highlight`, `advanced`, `math`, `html`, `edit`** are pure functions — no reactive
  state, no widgets; unit-tested next to the code.
- **`widget`, `render`** are the only places that build widgets; they read `MdProps`/`Cx` and the
  parsed model and render. `Cx` (the per-render context) is threaded through `render`.
- **`editor`** composes the reader with a bound text area.

## Conventions

- **One concern per file.** If a file starts mixing concerns, split it.
- **Pure where possible.** `parse`/`highlight`/`advanced`/`math`/`html`/`edit`/`model` are pure —
  unit-test them next to the code.
- **`widget`/`render` own widgets; nothing else does.** Everything else returns data.
- **Never panic on bad input.** A reader is always fed half-written, malformed Markdown — the
  `robustness`/`storm` suites hammer this.
- **Per-widget local state is its own `component_props`** — never a `create_signal` in a plain
  helper (it churns the parent's hook order). See the copy button, `<details>`, and the editor.
- **Every user-facing feature must be shown in `examples/demo`** — a section in the sample
  document or a control in the demo chrome — so it's visible and runnable (`cargo run -p demo`).
  A feature that isn't in the demo isn't done.
