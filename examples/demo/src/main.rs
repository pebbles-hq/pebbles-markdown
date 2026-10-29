//! The `pebbles-markdown` showcase — the widget's own sample app.
//!
//! ```sh
//! cargo run -p demo
//! # headless screenshot:
//! SHOT=1080:800:/tmp/markdown.rgba cargo run -p demo
//! ```
//!
//! Demonstrates the reader + editor, the View / Edit / Split modes (driven by a
//! signal you own), themeable [`MarkdownStyle`] variants, selectable + copyable
//! text (drag across blocks), keyboard link traversal, and the virtualized reader
//! on a deterministic multi-megabyte stress document. It uses nothing but
//! Pebbles' public API plus this crate — exactly what any app (or another widget
//! package) has to work with.

#[cfg(not(target_family = "wasm"))]
mod capture;

use std::rc::Rc;

use pebbles::prelude::*;
use pebbles_markdown::{MarkdownMode, MarkdownStyle, SyntaxColors, markdown, markdown_editor};

const DEMO: &str = "\
---
title: pebbles-markdown
author: Pebbles
tags: reader, editor, widget
---
# pebbles-markdown

A **GFM** reader + editor, shipped as a *separate widget package* for Pebbles —
the reference example of a third-party widget crate.

[toc]

## Task list — click the checkboxes

- [x] Parse GFM — tables, tasks, ~~strikethrough~~
- [ ] Toggle me — the SOURCE rewrites
- [ ] Build your own widget package

## Tight vs loose lists

A *tight* list (no blank lines) stays compact:

- one
- two
- three

A *loose* list (blank lines between items) breathes:

- first paragraph-spaced item

- second item

- third item

## Inline & block HTML

Inline HTML works: <b>bold</b>, <i>italic</i>, <code>code</code>, and a
<a href=\"https://github.com/pebbles-hq\">link</a>.<br>Line breaks too.

<blockquote>A block-level &lt;blockquote&gt; — parsed to a real quote widget.</blockquote>

<hr>

<ul><li>HTML list item one</li><li>HTML list item two</li></ul>

> Block quotes carry whole blocks — including **formatting**, `code`, and
> > nested quotes. Unicode is safe everywhere: café — naïve — 世界 — 🎉 — ∈ ≤ ∞.

## Fenced code (label · copy button · highlighted line 3)

```rust {3}
fn main() {
    // Non-ASCII in code no longer freezes the reader.
    let número = 42; // época — 世界 🎉
    println!(\"JetBrains Mono, bundled — {número}\");
}
```

## Images (sized + captioned)

![A captioned image|320](https://picsum.photos/640/360)

Wide tables scroll horizontally instead of squishing:

## A table (left / center / right aligned)

| Feature | State | Notes            |
|:--------|:-----:|-----------------:|
| Reader  | ✅    | GFM via pulldown |
| Editor  | ✅    | Edit/Split/Read  |
| Package | ✅    | its own crate    |

Inline: **bold**, *italic*, ***both***, `code`, and
[a link](https://github.com/pebbles-hq/pebbles-markdown).

Autolinks work with no brackets: https://pebbles-hq.dev, www.example.com, and
email hello@pebbles-hq.dev — all become clickable automatically.

## Advanced markup :sparkles:

> [!NOTE]
> Callouts (admonitions) come from `> [!NOTE]` — also TIP, IMPORTANT, WARNING, CAUTION.

> [!WARNING]
> Each kind gets its own accent color and icon.

Inline extras: ==highlighted==, H~2~O (subscript), E = mc^2^ (superscript),
emoji :rocket: :fire:, and math $e^{i\\pi} + 1 = 0$ via a TeX→Unicode transform.

Term
: A definition list — the term is bold, the definition indented.

<details>
<summary>Click to expand (a collapsible section)</summary>

Hidden markdown body — **formatting**, lists, and `code` all work inside.

</details>

## Obsidian-flavored (PKM)

Wikilinks: [[Some Page]] and aliased [[Some Page|a friendlier name]].
Tags organize notes: #project #status/active.
A note-embed reference: ![[Another Note]] (links to its target).
Comments %% this is hidden in reading view %% stay invisible.

## Selection & keyboard

Drag across this paragraph **and the next one** — selection spans blocks, and
releasing copies the text to the clipboard. Then press Tab to focus a paragraph
with links and use the arrow keys to move between
[the first link](https://pebbles-hq.dev), [the second](https://example.com), and
[the third](https://github.com/pebbles-hq); Enter opens the focused one.

## Footnotes

Footnotes render as superscript markers[^note] and collect at the very
bottom of the document[^more], numbered by first reference.

[^note]: A footnote definition — it can hold **formatting** and `code`.
[^more]: Definitions may appear anywhere; they are gathered here in order.
";

/// A serif-heading, roomier variant with a WARM syntax palette — code coloring
/// is fully themeable, not fixed.
fn serif_style() -> MarkdownStyle {
    MarkdownStyle {
        heading_family: Some("Lora".to_string()),
        heading_scale: [2.1, 1.7, 1.4, 1.2, 1.05, 0.95],
        block_gap: 14.0,
        syntax: SyntaxColors {
            keyword: palette::rose::S500,
            string: palette::amber::S600,
            comment: palette::stone::S400,
            number: palette::orange::S600,
            ident: palette::teal::S600,
            ..SyntaxColors::from_theme()
        },
        ..MarkdownStyle::from_theme()
    }
}

/// A dense variant for sidebars/tooltips, with a COOL syntax palette.
fn compact_style() -> MarkdownStyle {
    MarkdownStyle {
        body_size: 12.5,
        block_gap: 6.0,
        // The compact variant also turns on code-block line numbers.
        code_line_numbers: true,
        syntax: SyntaxColors {
            keyword: palette::indigo::S500,
            string: palette::emerald::S500,
            comment: palette::slate::S400,
            number: palette::cyan::S600,
            ident: palette::blue::S600,
            ..SyntaxColors::from_theme()
        },
        ..MarkdownStyle::from_theme()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Default,
    Serif,
    Compact,
}

fn app() -> AnyWidget {
    let source = create_signal(DEMO.to_string());
    let mode = create_signal(MarkdownMode::Split);
    let look = create_signal(Look::Default);
    let huge = create_signal(false);

    let style_of = move || match look.get() {
        Look::Default => MarkdownStyle::from_theme(),
        Look::Serif => serif_style(),
        Look::Compact => compact_style(),
    };

    // A segmented button bound to a signal: the active choice is filled.
    let seg = move |label: &str, active: bool, on: Rc<dyn Fn()>| {
        button(label)
            .size(ButtonSize::Sm)
            .variant(if active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Outline
            })
            .on_pressed(move || on())
    };
    let mode_seg = move |label: &str, m: MarkdownMode| {
        seg(
            label,
            mode.get() == m && !huge.get(),
            Rc::new(move || {
                // Leaving the (virtualized) huge view: restore the small doc so the
                // editor never lays out a multi-MB document non-virtualized.
                if huge.get() {
                    huge.set(false);
                    source.set(DEMO.to_string());
                }
                mode.set(m);
            }),
        )
    };
    let look_seg =
        move |label: &str, l: Look| seg(label, look.get() == l, Rc::new(move || look.set(l)));

    // The body switches between the editor and the virtualized reader on the
    // stress document — the virtualized reader is its own scroll view, so it
    // gets a bounded (Expanded) slot; the editor auto-grows inside a scroll box.
    // The reader is selectable (drag across blocks to select + copy) and its
    // links are keyboard-navigable (Tab to a paragraph, arrows cycle its links,
    // Enter opens). `on_link` receives every click / keyboard activation.
    let body: AnyWidget = if huge.get() {
        expanded(
            markdown(source.get())
                .virtualized()
                .style(style_of())
                .selectable()
                .keyboard_links()
                .on_link(|url| println!("link: {url}")),
        )
        .into_widget()
    } else {
        expanded(scroll_view(
            markdown_editor(source)
                .mode_signal(mode)
                .style(style_of())
                .on_link(|url| println!("link: {url}"))
                .lines(18),
        ))
        .into_widget()
    };

    container()
        .padding(EdgeInsets::all(16.0))
        .child(
            column(children![
                // Row 1: title + mode toggle.
                row(children![
                    text("pebbles-markdown").size(18.0).semibold(),
                    expanded(container()),
                    mode_seg("View", MarkdownMode::Read),
                    gap_w(6.0),
                    mode_seg("Edit", MarkdownMode::Edit),
                    gap_w(6.0),
                    mode_seg("Split", MarkdownMode::Split),
                    gap_w(12.0),
                    // Follows the OS light/dark setting (ThemeMode::System) until
                    // toggled; every colour below comes from the live theme.
                    button(if theme().dark { "Light" } else { "Dark" })
                        .size(ButtonSize::Sm)
                        .variant(ButtonVariant::Outline)
                        .on_pressed(toggle_theme),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Center),
                gap_h(10.0),
                // Row 2: style variants + the huge-document virtualization demo.
                row(children![
                    text("Style")
                        .size(13.0)
                        .color(theme().colors.muted_foreground),
                    gap_w(8.0),
                    look_seg("Default", Look::Default),
                    gap_w(6.0),
                    look_seg("Serif", Look::Serif),
                    gap_w(6.0),
                    look_seg("Compact", Look::Compact),
                    expanded(container()),
                    button(if huge.get() {
                        "Huge demo (virtualized) ✓"
                    } else {
                        "Load huge demo"
                    })
                    .size(ButtonSize::Sm)
                    .variant(if huge.get() {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Outline
                    })
                    .on_pressed(move || {
                        if huge.get() {
                            huge.set(false);
                            source.set(DEMO.to_string());
                        } else {
                            source.set(huge_document());
                            huge.set(true);
                        }
                    }),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Center),
                gap_h(14.0),
                expanded(
                    container()
                        .decoration(
                            BoxDecoration::new()
                                .border(Border::new(theme().colors.border, 1.0))
                                .radius(BorderRadius::all(8.0)),
                        )
                        .padding(EdgeInsets::all(12.0))
                        .child(body),
                ),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Stretch),
        )
        .into_widget()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Headless screenshot path (SHOT=<w>:<h>:<out.rgba>) — no window, for docs/CI.
    // Pinned to light so the docs shots are deterministic; the window follows the
    // system light/dark setting (the default `ThemeMode::System`).
    #[cfg(not(target_family = "wasm"))]
    {
        if let Ok(spec) = std::env::var("SHOT") {
            set_theme_mode(ThemeMode::Light);
            return capture::shot(&spec, app, theme().colors.background);
        }
    }
    App::new(component(app))
        .title("pebbles-markdown — showcase")
        .size(1080, 800)
        .run()
}

// ---------------------------------------------------------------------------
// A deterministic multi-megabyte GFM stress document — the virtualization demo.
// No randomness, no I/O: the same document every run.
// ---------------------------------------------------------------------------

/// Thousands of styled paragraphs, 100+ fenced code blocks, tables, nested
/// quotes/lists, task lists, and two pathological cases (one ~100k-char
/// paragraph, one ~20k-char code line). The virtualized reader renders it as a
/// few hundred widgets no matter its size.
fn huge_document() -> String {
    const WORDS: [&str; 16] = [
        "viewport", "render", "signal", "widget", "layout", "scene", "glyph", "arena", "frame",
        "paint", "scroll", "anchor", "extent", "cache", "measure", "pebbles",
    ];
    let mut s = String::with_capacity(1_600_000);
    s.push_str("# Stress document\n\nGenerated fixture: deterministic worst-case GFM.\n\n");
    for p in 0..4000usize {
        if p % 100 == 0 {
            s.push_str(&format!("\n## Section {}\n\n", p / 100));
        }
        if p % 200 == 199 {
            s.push_str("\n---\n\n");
        }
        for w in 0..40usize {
            let word = WORDS[(p + w) % WORDS.len()];
            match (p + w) % 23 {
                0 => s.push_str(&format!("**{word}** ")),
                7 => s.push_str(&format!("*{word}* ")),
                11 => s.push_str(&format!("`{word}` ")),
                17 => s.push_str(&format!("[{word}](https://example.com/{word}) ")),
                19 => s.push_str(&format!("~~{word}~~ ")),
                _ => {
                    s.push_str(word);
                    s.push(' ');
                }
            }
        }
        s.push_str("\n\n");
        if p % 40 == 0 {
            s.push_str("```rust\n");
            for l in 0..30usize {
                s.push_str(&format!(
                    "fn item_{p}_{l}(x: u64) -> u64 {{ x * {l} + {p} }} // {}\n",
                    WORDS[l % WORDS.len()]
                ));
            }
            s.push_str("```\n\n");
        }
        if p % 80 == 0 {
            s.push_str("| col a | col b | col c | col d |\n|---|---|---|---|\n");
            for r in 0..6usize {
                s.push_str(&format!("| a{p}r{r} | `b{r}` | **c{r}** | d{r} |\n"));
            }
            s.push('\n');
        }
        if p % 60 == 0 {
            s.push_str("> quoted **block** with nesting\n> > deeper quote\n\n");
            s.push_str(&format!(
                "1. ordered {p}\n2. next\n   - nested child\n   - `code` child\n\n"
            ));
        }
        if p % 50 == 0 {
            s.push_str(&format!("- [ ] open task {p}\n- [x] done task {p}\n\n"));
        }
    }
    s.push_str("\n## Pathological paragraph\n\n");
    for i in 0..12_500usize {
        s.push_str(WORDS[i % WORDS.len()]);
        s.push(' ');
    }
    s.push_str("\n\n## Pathological code line\n\n```\n");
    for i in 0..2_500usize {
        s.push_str(&format!("x{i};"));
    }
    s.push_str("\n```\n");
    s
}
