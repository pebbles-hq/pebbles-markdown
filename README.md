# pebbles-markdown

A live **Markdown reader + editor** widget for the
[Pebbles](https://github.com/pebbles-hq/pebbles) GUI framework, maintained as a
**separate package**.

It is also the reference example of a **third-party Pebbles widget crate**:
it depends only on Pebbles' *public* API and composes the built-in catalog into
new widgets — the same thing any ecosystem package does. If markdown can live
outside the framework as a normal crate, so can your components.

```rust
use pebbles::prelude::*;
use pebbles_markdown::{markdown, markdown_editor, MarkdownMode};

fn notes() -> impl IntoWidget {
    // A read-only rendered document…
    markdown("# Hello\n\n- [x] tasks\n- [ ] tables\n\n`code`, **bold**, *italic*.")
}

fn editor(src: Signal<String>) -> impl IntoWidget {
    // …or a live editor with View / Edit / Split modes you drive by signal.
    markdown_editor(src).mode_signal(create_signal(MarkdownMode::Split))
}
```

Run the showcase sample — reader + editor with View / Edit / Split modes,
themeable style variants (default / serif / compact), and the virtualized reader
on a deterministic multi-megabyte stress document:

```sh
cargo run --example reader
```

## What it does

- **Reading** (`markdown`): full GFM via [`pulldown-cmark`] — headings, emphasis /
  strong / strikethrough, inline + fenced code (syntax-highlighted, JetBrains
  Mono), clickable links, nested quotes and lists, **task lists with live
  checkboxes** that rewrite the bound source, tables, rules, and
  images (with the `image-view` feature; alt text otherwise).
- **Editing** (`markdown_editor`): a `Signal<String>`-bound editor with Edit /
  Split / Read modes, driven by a mode signal you own — no built-in chrome.
- **Theming**: everything visual lives in `MarkdownStyle`, defaulting from the
  live Pebbles theme (follows light/dark).
- **Robust by contract**: any input renders — malformed or unusual constructs
  fall back to plain text instead of crashing (see `tests/robustness.rs`).
- **Virtualized**: `markdown(..).virtualized()` renders only the blocks in view,
  so a multi-megabyte document stays a few hundred widgets.

## Depending on Pebbles

This crate needs the Pebbles framework crates. Two supported layouts:

**Local / side-by-side (this repo's default).** Clone `pebbles` and
`pebbles-markdown` as siblings; the `Cargo.toml` path dependencies resolve
against `../pebbles`.

```
some-dir/
  pebbles/            # github.com/pebbles-hq/pebbles
  pebbles-markdown/   # this repo
```

**External consumer (git).** Point at the framework by git — this is how any
ecosystem package (yours included) depends on Pebbles until it is published to a
registry:

```toml
[dependencies]
pebbles          = { git = "https://github.com/pebbles-hq/pebbles" }
pebbles-markdown = { git = "https://github.com/pebbles-hq/pebbles-markdown" }
```

## Building your own Pebbles widget package

This crate is the template. A Pebbles widget package:

1. Depends on `pebbles-core` (the reactive + widget contract: `Signal`,
   `component` / `component_props`, `IntoWidget`, `children!`), `pebbles-widgets`
   (the catalog + `theme()` to compose and match the app's look), and
   `pebbles-render` / `pebbles-foundation` for primitives (`Color`, `TextSpan`,
   layout types). For a genuinely new low-level widget, implement
   `pebbles_render::RenderObject` and bind it with the `render_widget!` macro —
   all public.
2. Exposes plain constructor functions returning `impl IntoWidget` (the Pebbles
   convention — lowercase `markdown(..)`, not `Markdown::new()`), so callers use
   your widget exactly like a built-in one.
3. Never reaches into framework internals — everything here is reachable from the
   public API. If you find a gap, that's a framework bug worth filing upstream.

## License

Licensed under the **Apache License, Version 2.0** — see [LICENSE](LICENSE) and
[NOTICE](NOTICE). The same license as the Pebbles framework.

Copyright © 2026 Reyco Seguma.

[`pulldown-cmark`]: https://github.com/pulldown-cmark/pulldown-cmark
