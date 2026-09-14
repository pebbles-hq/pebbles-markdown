# Contributing to pebbles-markdown

Thanks for helping! This crate is the live Markdown reader + editor widget for the
[Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework — a worked example of a
third-party Pebbles widget package (it depends only on Pebbles' *public* API). It's organized by
concept so you can find things by name and land changes with confidence.

## Setup

You need a recent stable Rust (edition 2024; `rust-version = 1.90`). The crate depends on
`pebbles` (by git for external consumers, or by path for side-by-side development), so the first
build fetches and compiles the framework (and its `wgpu`/`vello` stack) — expect a few minutes
the first time, fast after that.

```sh
git clone https://github.com/pebbles-hq/pebbles-markdown
cd pebbles-markdown
cargo build
cargo run -p demo            # the reader/editor showcase
cargo run --example reader   # the minimal single-file example
```

## Where the code lives

`lib.rs` is a thin hub: crate docs, the module tree, the callback aliases, and the public
re-exports. See [`ARCHITECTURE.md`](ARCHITECTURE.md) for the full map and the layering rules. The
essentials:

| Path | What's in it |
|---|---|
| `src/model.rs` | The parsed block/inline tree: `Block`/`Inline`/`Run`/`Flags`/`ListItem`. Plain data. |
| `src/parse.rs` | pulldown-cmark → `Vec<Block>` + the in-tree extensions (front matter, `:::`, `#tags`, comments, autolinks, emoji, slugs) + `to_html`. Panic-free. |
| `src/highlight.rs` | The dependency-free code lexer: source → colored spans from `SyntaxColors`. Pure. |
| `src/advanced.rs` `src/math.rs` `src/html.rs` | Emoji/TeX/slug helpers; the 2-D display-math engine; the safe HTML-subset parser. Pure. |
| `src/style.rs` | `MarkdownStyle` (every visual knob) + `SyntaxColors`. The styling surface. |
| `src/edit.rs` | Source transforms: `toggle_task`, `wrap_selection`, `toggle_line_prefix`. Pure. |
| `src/widget.rs` | The `Markdown` builder + render context `Cx` + orchestration (virtualization, folding, find, selection). |
| `src/render.rs` | Block/inline → themed widget tree (code, callouts, details, tables, images, embeds, inline flow). |
| `src/editor.rs` | `MarkdownEditor` — the `Signal<String>`-bound Edit/Split/Read editor. No chrome. |

## Conventions

- **One concern per file.** If a file starts mixing concerns, split it.
- **Pure where possible.** `parse`/`highlight`/`advanced`/`math`/`html`/`edit`/`model` are pure
  functions of text — unit-test them next to the code.
- **`widget`/`render` own widgets; nothing else does.** Everything else returns data.
- **Never panic on bad input.** A reader is always fed half-written, malformed Markdown.
- **Per-widget local state is its own `component_props`** — never a `create_signal` in a plain
  helper (it churns the parent's hook order). See `ARCHITECTURE.md`.
- **Every user-facing feature must be shown in `examples/demo`** (a section in the sample doc or a
  control in the demo) — a feature that isn't in the demo isn't done.

## Before you push

The same gates CI runs:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

A good change also adds/updates: a test (`tests/rendering.rs` for widget behavior,
`tests/robustness.rs` / `tests/storm.rs` for malformed input, a `#[cfg(test)]` unit test for the
pure modules), the demo, and `CHANGELOG.md`.
