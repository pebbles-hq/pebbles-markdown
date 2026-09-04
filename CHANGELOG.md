# Changelog

All notable changes to `pebbles-markdown` are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow [SemVer](https://semver.org).

## [0.1.0]

Initial release as a standalone package. The Obsidian-style Markdown reader +
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
