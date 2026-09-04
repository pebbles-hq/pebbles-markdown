//! A standalone demo of the `pebbles-markdown` widget — a live editor with a
//! View / Edit / Split mode toggle, driven by a signal you own. Run it with:
//!
//! ```sh
//! cargo run --example reader
//! ```
//!
//! It uses nothing but Pebbles' public API plus this crate — exactly what a
//! third-party widget package (and its users) has to work with.

use pebbles::prelude::*;
use pebbles_markdown::{MarkdownMode, markdown_editor};

const DOC: &str = "\
# pebbles-markdown

A **GFM** reader + editor, shipped as a *separate widget package* for Pebbles.

- [x] Parse GFM — tables, tasks, ~~strikethrough~~
- [ ] Toggle me — the SOURCE rewrites, Obsidian-style
- [ ] Build your own widget package

> Block quotes carry whole blocks — including **formatting**, `code`, and
> nested content. Unicode is safe everywhere: café — naïve — 世界 — 🎉.

```rust
fn main() {
    // Non-ASCII in code no longer freezes the reader.
    let número = 42; // época
    println!(\"JetBrains Mono, bundled — {número}\");
}
```

| Feature | State |
|---------|-------|
| Reader  | ✅    |
| Editor  | ✅    |
| Package | ✅    |
";

fn app() -> impl IntoWidget {
    let source = create_signal(DOC.to_string());
    let mode = create_signal(MarkdownMode::Split);

    // A tiny segmented control over the mode signal the editor reads.
    let seg = |label: &str, m: MarkdownMode| {
        let active = mode.get() == m;
        button(label)
            .size(ButtonSize::Sm)
            .variant(if active { ButtonVariant::Primary } else { ButtonVariant::Outline })
            .on_pressed(move || mode.set(m))
    };

    container().padding(EdgeInsets::all(16.0)).child(
        column(children![
            row(children![
                text("pebbles-markdown").size(18.0).semibold(),
                Expanded::new(container()),
                seg("View", MarkdownMode::Read),
                gap_w(6.0),
                seg("Edit", MarkdownMode::Edit),
                gap_w(6.0),
                seg("Split", MarkdownMode::Split),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center),
            gap_h(14.0),
            Expanded::new(
                container()
                    .decoration(
                        BoxDecoration::new()
                            .border(Border::new(theme().colors.border, 1.0))
                            .radius(BorderRadius::all(8.0)),
                    )
                    .padding(EdgeInsets::all(12.0))
                    .child(scroll_view(markdown_editor(source).mode_signal(mode).lines(20))),
            ),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Stretch),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Theme::light().make_current();
    App::new(component(app)).title("pebbles-markdown — reader").size(1040, 760).run()
}
