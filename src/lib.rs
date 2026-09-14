//! # pebbles-markdown
//!
//! A live Markdown reader + editor **widget for
//! [Pebbles](https://github.com/pebbles-hq/pebbles)**, maintained as a separate
//! package. It is a worked example of a third-party Pebbles widget crate: it
//! depends only on Pebbles' *public* API (`pebbles-widgets` for the catalog +
//! theme, `pebbles-core` for the reactive/widget contract, `pebbles-render` /
//! `pebbles-foundation` for primitives) and composes them into new widgets —
//! nothing here reaches into framework internals. Drop [`markdown`] or
//! [`markdown_editor`] into any Pebbles tree exactly like a built-in widget.
//!
//! **Reading** ([`markdown`]): full GFM via pulldown-cmark — headings, emphasis /
//! strong / strikethrough, inline code, fenced code blocks (JetBrains Mono),
//! links (clickable — wire [`Markdown::on_link`]), block quotes (nested), ordered +
//! unordered lists (nested), **task lists with live checkboxes** (toggling rewrites
//! the bound source), tables, horizontal rules, and images (rendered
//! with the `image-view` feature, alt-text otherwise).
//!
//! **Editing** ([`markdown_editor`]): a `Signal<String>`-bound editor with three
//! modes — [`MarkdownMode::Edit`] (source), [`MarkdownMode::Split`] (source + live
//! preview), [`MarkdownMode::Read`] (rendered only). The mode is a signal you can
//! own ([`MarkdownEditor::mode_signal`]) — build your own mode switcher, the widget
//! ships no chrome (the same philosophy as the file explorer).
//!
//! **Theming**: everything visual lives in [`MarkdownStyle`] — heading scale, body
//! size/color, code block/inline colors + family, quote bar, link color, rules,
//! tables. Defaults derive from the live [`theme()`](fn@pebbles_widgets::theme), so it
//! follows light/dark automatically; pass your own via [`Markdown::style`].

use std::rc::Rc;

/// A link-click handler (open a browser, route internally, …) — the reader's
/// `on_link`. Aliased (like the framework's `Callback`) so the callback fields
/// stay readable and clippy-clean.
pub(crate) type LinkFn = Rc<dyn Fn(&str)>;
/// A task-checkbox toggle handler: `(document-order ordinal, now_checked)`.
pub(crate) type TaskFn = Rc<dyn Fn(usize, bool)>;
/// A link-hover handler: `Some(url)` over a link, `None` on leaving it.
pub(crate) type LinkHoverFn = Rc<dyn Fn(Option<&str>)>;

mod advanced;
mod edit;
mod editor;
mod highlight;
mod html;
mod math;
mod model;
mod parse;
mod render;
mod style;
mod widget;

// ── Public API — the crate's front door ──────────────────────────────────────
pub use edit::{toggle_line_prefix, toggle_task, wrap_selection};
pub use editor::{MarkdownEditor, MarkdownMode, markdown_editor};
pub use parse::to_html;
pub use style::{MarkdownStyle, SyntaxColors};
pub use widget::{Markdown, markdown};

// ── Internal cross-module surface (flat namespace for the modules + tests) ────
pub(crate) use highlight::*;
pub(crate) use model::*;
pub(crate) use parse::*;
pub(crate) use render::*;
pub(crate) use widget::*;

#[cfg(test)]
mod tests;
