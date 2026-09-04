//! The Markdown reader/editor (feature `markdown`): source-level task toggling,
//! live re-render on bound-source edits, and the editor's mode switching —
//! headless through a real Ui.

use pebbles_core::{IntoWidget, Ui, component, create_signal};
use pebbles_foundation::{Size, palette};
use pebbles_render::TextEnv;
use pebbles_markdown::{MarkdownMode, markdown, markdown_editor, toggle_task};
use pebbles_widgets::View;

// ---------------------------------------------------------------------------
// toggle_task — the Obsidian source rewrite
// ---------------------------------------------------------------------------

#[test]
fn toggle_task_flips_the_right_checkbox_in_source() {
    let src = "# t\n\n- [ ] first\n- [x] second\n  - [ ] nested\n\n1. [ ] ordered\n";
    // Ordinal 0: check "first".
    let s = toggle_task(src, 0).expect("first task");
    assert!(s.contains("- [x] first"), "{s}");
    // Ordinal 1: uncheck "second".
    let s = toggle_task(src, 1).expect("second task");
    assert!(s.contains("- [ ] second"), "{s}");
    // Ordinal 2: the nested one keeps its indentation.
    let s = toggle_task(src, 2).expect("nested task");
    assert!(s.contains("  - [x] nested"), "{s}");
    // Ordinal 3: ordered-list tasks work too.
    let s = toggle_task(src, 3).expect("ordered task");
    assert!(s.contains("1. [x] ordered"), "{s}");
    // Out of range → None; non-task bullets are not counted.
    assert!(toggle_task(src, 4).is_none());
    assert!(toggle_task("- plain bullet\n", 0).is_none());
    // Uppercase X unchecks as well.
    assert_eq!(toggle_task("- [X] done", 0).as_deref(), Some("- [ ] done"));
}

// ---------------------------------------------------------------------------
// Rendering: a bound source re-renders live
// ---------------------------------------------------------------------------

thread_local! {
    static SRC: std::cell::RefCell<Option<pebbles_core::Signal<String>>> =
        const { std::cell::RefCell::new(None) };
    static MODE: std::cell::RefCell<Option<pebbles_core::Signal<MarkdownMode>>> =
        const { std::cell::RefCell::new(None) };
}

const DOC: &str = "# Title\n\nSome **bold** and a [link](https://x.y).\n\n- [ ] task\n\n```rust\nfn x() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> quoted\n";

fn reader_root() -> impl IntoWidget {
    let src = create_signal(DOC.to_string());
    SRC.with(|c| *c.borrow_mut() = Some(src));
    markdown("").bind(src)
}

#[test]
fn bound_markdown_renders_and_rerenders_on_source_and_task_toggle() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 800.0);
    ui.mount_root(View::new(palette::WHITE, component(reader_root)).into_widget());
    ui.layout(&mut env, win);
    let before = ui.element_count();
    assert!(before > 20, "the GFM document produced a real tree ({before} elements)");

    // Append a heading → the view re-renders with more elements.
    let src = SRC.with(|c| c.borrow().expect("src"));
    src.update(|s| s.push_str("\n## More\n\nwords here\n"));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(ui.element_count() > before, "live edit re-rendered the view");

    // A source-level task toggle round-trips through the same signal.
    let toggled = toggle_task(&src.peek(), 0).expect("task present");
    assert!(toggled.contains("- [x] task"));
    src.set(toggled);
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
}

fn editor_root() -> impl IntoWidget {
    let src = create_signal(DOC.to_string());
    let mode = create_signal(MarkdownMode::Edit);
    SRC.with(|c| *c.borrow_mut() = Some(src));
    MODE.with(|c| *c.borrow_mut() = Some(mode));
    markdown_editor(src).mode_signal(mode).lines(8)
}

#[test]
fn editor_switches_modes_via_the_external_signal() {
    pebbles_widgets::theme::init();
    pebbles_core::focus::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(700.0, 600.0);
    ui.mount_root(View::new(palette::WHITE, component(editor_root)).into_widget());
    ui.layout(&mut env, win);
    let edit_count = ui.element_count();

    let mode = MODE.with(|c| c.borrow().expect("mode"));
    mode.set(MarkdownMode::Read);
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    let read_count = ui.element_count();
    assert_ne!(edit_count, read_count, "Read renders the document, not the source pane");

    mode.set(MarkdownMode::Split);
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(ui.element_count() > read_count, "Split shows both panes");
}

// ---------------------------------------------------------------------------
// One flow = ONE paragraph: rich inline text shapes as a single layout with
// per-range spans — real spaces, engine-owned wrapping, no widget-per-word.
// ---------------------------------------------------------------------------

#[test]
fn a_paragraph_is_one_shaped_layout_with_real_spaces() {
    use pebbles_render::RenderParagraph;
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(palette::WHITE, component(|| markdown("alpha beta gamma"))).into_widget(),
    );
    ui.layout(&mut env, Size::new(600.0, 400.0));
    let tree = ui.render_tree();
    let paras = tree.find_all::<RenderParagraph>();
    assert_eq!(paras.len(), 1, "one flow = one paragraph, got {}", paras.len());
    let text = &tree.object_ref(paras[0]).downcast_ref::<RenderParagraph>().unwrap().text;
    assert_eq!(text, "alpha beta gamma", "spaces survive into the layout");
    // The shaped width is wider than the glyphs alone (spaces carry advance).
    assert!(tree.size_of(paras[0]).width > 80.0, "sentence measures like a sentence");
}

// ---------------------------------------------------------------------------
// Rich spans: styles/links/chips map to byte ranges of ONE layout, and the
// paragraph publishes link geometry for the tap resolver.
// ---------------------------------------------------------------------------

#[test]
fn rich_paragraph_spans_map_styles_links_and_chips() {
    use pebbles_render::RenderParagraph;
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            palette::WHITE,
            component(|| {
                markdown("Some **bold** and `code` plus a [link](https://x.y) end")
                    .on_link(|_| {})
            }),
        )
        .into_widget(),
    );
    ui.layout(&mut env, Size::new(600.0, 400.0));
    let tree = ui.render_tree();
    let paras = tree.find_all::<RenderParagraph>();
    assert_eq!(paras.len(), 1, "one flow = one paragraph");
    let p = tree.object_ref(paras[0]).downcast_ref::<RenderParagraph>().unwrap();
    assert_eq!(p.text, "Some bold and code plus a link end");
    let bold = p.spans.iter().find(|s| s.weight == Some(600.0)).expect("bold span");
    assert_eq!(&p.text[bold.range.clone()], "bold");
    let chip = p.spans.iter().find(|s| s.chip.is_some()).expect("chip span");
    assert_eq!(&p.text[chip.range.clone()], "code");
    let link = p.spans.iter().find(|s| s.link.is_some()).expect("link span");
    assert_eq!(&p.text[link.range.clone()], "link");
    assert!(link.underline, "links underline");
    // The paragraph published the link's laid-out boxes for the tap resolver.
    let boxes = p.link_boxes.as_ref().expect("link geometry cell").borrow();
    assert_eq!(boxes.len(), 1, "a one-line link publishes one box");
    let (r, ix) = boxes[0];
    assert_eq!(ix, 0);
    assert!(r.width() > 5.0 && r.height() > 5.0, "the box covers real glyphs: {r:?}");
}

#[test]
fn code_blocks_keep_indentation_in_one_unwrapped_layout() {
    use pebbles_render::RenderParagraph;
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    ui.mount_root(
        View::new(
            palette::WHITE,
            component(|| markdown("```\nfn main() {\n    let x = 1;\n}\n```")),
        )
        .into_widget(),
    );
    ui.layout(&mut env, Size::new(600.0, 400.0));
    let tree = ui.render_tree();
    let paras = tree.find_all::<RenderParagraph>();
    assert_eq!(paras.len(), 1, "one code block = one paragraph");
    let p = tree.object_ref(paras[0]).downcast_ref::<RenderParagraph>().unwrap();
    assert!(p.text.contains("\n    let x = 1;"), "real newlines + real indentation: {:?}", p.text);
    assert!(!p.style.soft_wrap, "code never soft-wraps; lines break at newlines only");
    assert!(p.spans.iter().any(|s| s.color.is_some()), "tokens carry color spans");
}

// ---------------------------------------------------------------------------
// Split preview debounce: typing updates the editor immediately; the rendered
// preview follows ~150 ms later (one parse per pause, not per keystroke).
// ---------------------------------------------------------------------------

#[test]
fn split_preview_debounces_typing() {
    pebbles_widgets::theme::init();
    pebbles_core::focus::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(900.0, 700.0);
    ui.mount_root(View::new(palette::WHITE, component(editor_root)).into_widget());
    let mode = MODE.with(|c| c.borrow().expect("mode"));
    mode.set(MarkdownMode::Split);
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);

    // Type: the source changes; the preview must NOT re-render yet.
    let src = SRC.with(|c| c.borrow().expect("src"));
    src.update(|s| s.push_str("\n## Debounced\n\nnew paragraph here\n"));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    let during = ui.element_count();

    // Let the debounce window elapse: the first tick ARMS the pending timer
    // (delays anchor at the next frame), the second fires it.
    pebbles_core::animation::tick(1.0e9);
    pebbles_core::animation::tick(2.0e9);
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    let after = ui.element_count();
    assert!(
        after > during,
        "the preview updates only after the debounce ({during} -> {after} elements)"
    );
}
