//! The reader must survive ANY input — the reader's contract: a malformed or
//! unusual document renders (broken parts fall back to plain text) instead of
//! crashing or freezing. Each fixture is parsed AND rendered to a real scene; a
//! panic or an infinite loop fails the test rather than the app.

use std::cell::RefCell;

use pebbles_core::{IntoWidget, Ui, component};
use pebbles_foundation::{Size, palette};
use pebbles_render::TextEnv;
use pebbles_markdown::markdown;
use pebbles_widgets::View;

/// Documents that used to freeze or panic, plus a spread of malformed GFM. The
/// unifying theme: non-ASCII in code blocks (the freeze) and half-written
/// constructs (the "show it as text" cases).
const FIXTURES: &[&str] = &[
    // The freeze: non-ASCII inside a fenced code block, every flavor.
    "```rust\nlet café = 1; // caffè\n```",
    "```\nx — y → z ± ∞\n```",
    "```python\n数 = \"文字列\"  # コメント\n```",
    "```js\nconst party = `🎉🎊`; /* 世界 */\n```",
    "```\nnaïve “smart” ‘quotes’ …\n```",
    // Unclosed fence — pulldown runs it to EOF as a code block (with non-ASCII).
    "```rust\nfn main() { println!(\"héllo\"); }",
    // Open block comment with unicode, no close.
    "```c\n/* über\n世界\n```",
    // Half-written / malformed constructs → should degrade to text, not crash.
    "**bold never closed and a [link](that never closes",
    "| a | b |\n|---|\n| 1 | 2 | 3 | 4 |", // ragged table, short delimiter row
    "|broken|table|\nno delimiter row at all",
    "> quote\n>> deeper\n>>> deeper still — é →",
    "- [x] done é\n- [ ] todo 世界\n  - nested — child",
    "18446744073709551615. huge start\n2. next", // >9-digit start → plain text
    "###### h6\n####### not a heading (7 hashes) café",
    "a lone backslash at EOL \\\nand a tab\there\tand emoji 🚀",
    "<div>raw html</div> and <not-a-tag café>",
    "```\n\n\n```",                                     // empty code block
    "",                                                 // empty document
    "\u{0}\u{1}\u{2} control chars and \u{200B}zero-width",
    "![img](nonexistent.png) ![](  ) ![alt only](", // broken images
];

thread_local! {
    static CURRENT: RefCell<String> = const { RefCell::new(String::new()) };
}

fn current_doc() -> String {
    CURRENT.with(|c| c.borrow().clone())
}

fn robustness_root() -> impl IntoWidget {
    markdown(current_doc())
}

#[test]
fn malformed_markdown_renders_without_crashing() {
    pebbles_widgets::overlay::init();
    pebbles_core::focus::init();
    pebbles_widgets::theme::init();

    for (i, doc) in FIXTURES.iter().enumerate() {
        CURRENT.with(|c| *c.borrow_mut() = (*doc).to_string());
        let mut ui = Ui::new();
        ui.make_current();
        let mut env = TextEnv::new();
        // Mount, reconcile, lay out, and PAINT — the highlighter runs during the
        // build, so a freeze/panic in any fixture aborts the test here.
        ui.mount_root(View::new(palette::WHITE, component(robustness_root)).into_widget());
        ui.rebuild_if_dirty();
        ui.layout(&mut env, Size::new(760.0, 900.0));
        let mut scene = pebbles_render::Scene::new();
        ui.paint(&mut env, &mut scene);
        assert!(
            ui.element_count() > 0,
            "fixture #{i} produced a tree ({:?}…)",
            doc.chars().take(24).collect::<String>()
        );
    }
}

/// The exact real-world trigger, end to end: a code block with an accented
/// identifier used to hang the whole app. It must now render as text.
#[test]
fn code_block_with_accented_identifier_does_not_hang() {
    pebbles_widgets::theme::init();
    CURRENT.with(|c| *c.borrow_mut() = "```rust\nlet número = 42; // época\n```".to_string());
    let mut ui = Ui::new();
    ui.make_current();
    let mut env = TextEnv::new();
    ui.mount_root(View::new(palette::WHITE, component(robustness_root)).into_widget());
    ui.rebuild_if_dirty();
    ui.layout(&mut env, Size::new(600.0, 400.0));
    let mut scene = pebbles_render::Scene::new();
    ui.paint(&mut env, &mut scene);
    assert!(ui.render_node_count() > 0, "the code block rendered");
}
