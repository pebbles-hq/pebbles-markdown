//! The Markdown reader/editor (feature `markdown`): source-level task toggling,
//! live re-render on bound-source edits, and the editor's mode switching —
//! headless through a real Ui.

use pebbles_core::{IntoWidget, Ui, component, create_signal};
use pebbles_foundation::{Offset, Size, palette};
use pebbles_markdown::{MarkdownMode, MarkdownStyle, markdown, markdown_editor, toggle_task};
use pebbles_render::TextEnv;
use pebbles_widgets::View;

// ---------------------------------------------------------------------------
// toggle_task — the source rewrite
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
// §5: code-block line numbers, highlighted lines, and the copy button render
// ---------------------------------------------------------------------------

#[test]
fn code_block_line_numbers_highlights_and_copy_render() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 800.0);
    let mut style = MarkdownStyle::from_theme();
    style.code_line_numbers = true;
    // A fenced block with a language label, highlighted lines `{2,3}`, and body.
    let doc = "```rust {2,3}\nfn a() {}\nlet x = 1;\nlet y = 2;\nlet z = 3;\n```\n";
    ui.mount_root(View::new(palette::WHITE, markdown(doc).style(style)).into_widget());
    ui.layout(&mut env, win);
    // The per-line path + header (label + copy button) build a real tree, no panic.
    assert!(
        ui.element_count() > 8,
        "code block with line numbers renders a tree ({})",
        ui.element_count()
    );
}

// ---------------------------------------------------------------------------
// §14/§6: custom per-block renderer + host-supplied backlinks section
// ---------------------------------------------------------------------------

thread_local! {
    static BLOCK_KINDS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[test]
fn block_renderer_and_backlinks_render() {
    use pebbles_widgets::text;
    pebbles_widgets::theme::init();
    BLOCK_KINDS.with(|c| c.borrow_mut().clear());
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 800.0);
    let doc = "# Heading\n\npara text\n\n```rust\nfn x() {}\n```\n";
    ui.mount_root(
        View::new(
            palette::WHITE,
            markdown(doc)
                .block_renderer(|kind, _text| {
                    BLOCK_KINDS.with(|c| c.borrow_mut().push(kind.to_string()));
                    (kind == "code").then(|| text("[custom code]").into_widget())
                })
                .backlinks([("Note A".to_string(), "note-a".to_string())]),
        )
        .into_widget(),
    );
    ui.layout(&mut env, win);
    let kinds = BLOCK_KINDS.with(|c| c.borrow().clone());
    assert!(
        kinds.contains(&"heading".to_string()),
        "renderer saw the heading"
    );
    assert!(
        kinds.contains(&"code".to_string()),
        "renderer saw + replaced the code block"
    );
    assert!(
        kinds.contains(&"backlinks".to_string()),
        "backlinks section rendered"
    );
    assert!(ui.element_count() > 5);
}

// ---------------------------------------------------------------------------
// §7: selectable rendered text — drag selects a block, copies on release
// (framework `RichText::selectable` + `RenderParagraph` selection)
// ---------------------------------------------------------------------------

#[test]
fn selectable_text_drag_copies_to_clipboard() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 200.0);
    ui.mount_root(
        View::new(
            palette::WHITE,
            markdown("Select this whole sentence.").selectable(),
        )
        .into_widget(),
    );
    ui.layout(&mut env, win);
    // Resolve the pan target (the paragraph's gesture listener), then drag across it.
    let start = Offset::new(2.0, 10.0);
    let src = ui
        .pan_target_at(start)
        .expect("the selectable paragraph is a pan target");
    ui.dispatch_pan_start(src, start);
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    ui.dispatch_pan_update(src, Offset::new(560.0, 10.0)); // past the end → to line end
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    ui.dispatch_pan_end(src, Offset::new(560.0, 10.0));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    // On release, the selected text is on the clipboard.
    let copied = pebbles_core::clipboard::read();
    assert!(
        copied.contains("Select") && copied.contains("sentence"),
        "the drag selected + copied the line: {copied:?}"
    );
}

// ---------------------------------------------------------------------------
// §9/§12: inline-link hover reports the URL (framework `RichText::on_link_hover`)
// ---------------------------------------------------------------------------

thread_local! {
    static HOVER_URL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[test]
fn inline_link_hover_reports_url() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(400.0, 200.0);
    HOVER_URL.with(|c| *c.borrow_mut() = None);
    // The link text IS the whole paragraph, so any hover over it hits the link box.
    ui.mount_root(
        View::new(
            palette::WHITE,
            markdown("[https://pebbles-hq.dev](https://pebbles-hq.dev)")
                .on_link_hover(|u| HOVER_URL.with(|c| *c.borrow_mut() = u.map(str::to_string))),
        )
        .into_widget(),
    );
    ui.layout(&mut env, win);
    ui.dispatch_hover(Offset::new(10.0, 10.0));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert_eq!(
        HOVER_URL.with(|c| c.borrow().clone()).as_deref(),
        Some("https://pebbles-hq.dev"),
        "hovering the link reported its URL"
    );
}

// ---------------------------------------------------------------------------
// §12: keyboard link traversal — Tab focuses the paragraph, arrows cycle its
// links, Enter activates the focused one (framework `RichText::keyboard_nav`).
// ---------------------------------------------------------------------------

thread_local! {
    static ACTIVATED_URL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[test]
fn keyboard_links_traverse_and_activate() {
    use pebbles_core::{KeyInput, Motion};
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(500.0, 200.0);
    ACTIVATED_URL.with(|c| *c.borrow_mut() = None);
    ui.mount_root(
        View::new(
            palette::WHITE,
            markdown("Go to [one](https://one.dev) or [two](https://two.dev).")
                .keyboard_links()
                .on_link(|u| ACTIVATED_URL.with(|c| *c.borrow_mut() = Some(u.to_string()))),
        )
        .into_widget(),
    );
    ui.layout(&mut env, win);

    // Tab lands on the link paragraph (its sole focus stop); Enter activates the
    // first link.
    assert!(ui.focus_move(true), "the link paragraph is a focus stop");
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(
        ui.dispatch_key(KeyInput::Enter),
        "Enter is consumed by the focused paragraph"
    );
    assert_eq!(
        ACTIVATED_URL.with(|c| c.borrow().clone()).as_deref(),
        Some("https://one.dev"),
        "Enter activated the first (focused) link"
    );

    // Right-arrow advances to the second link, Enter activates it.
    assert!(
        ui.dispatch_key(KeyInput::Move {
            motion: Motion::Right,
            extend: false
        }),
        "the arrow key is consumed while more links remain"
    );
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(ui.dispatch_key(KeyInput::Enter));
    assert_eq!(
        ACTIVATED_URL.with(|c| c.borrow().clone()).as_deref(),
        Some("https://two.dev"),
        "arrow moved focus to the second link, Enter activated it"
    );

    // Arrowing past the last link releases the key so Tab can move on.
    assert!(
        !ui.dispatch_key(KeyInput::Move {
            motion: Motion::Right,
            extend: false
        }),
        "past the last link the arrow is not consumed (focus can move on)"
    );
}

// ---------------------------------------------------------------------------
// §6: a resolver transcludes `![[Note]]` content inline; §4: 2-D display math
// ---------------------------------------------------------------------------

#[test]
fn resolver_transcludes_and_math_renders() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 800.0);
    // The embed resolves to real Markdown that is parsed + rendered inline; the
    // display-math block goes through the 2-D layout engine.
    let doc = "![[Note]]\n\n$$\\frac{a}{b} + x^2$$\n";
    ui.mount_root(
        View::new(
            palette::WHITE,
            markdown(doc)
                .resolver(|t| (t == "Note").then(|| "## Resolved\n\nInner body.".to_string())),
        )
        .into_widget(),
    );
    ui.layout(&mut env, win);
    assert!(
        ui.element_count() > 10,
        "transcluded note + display math build a real tree ({})",
        ui.element_count()
    );
}

// ---------------------------------------------------------------------------
// §14: a fenced-block handler renders in place of the default code block
// ---------------------------------------------------------------------------

thread_local! {
    static FENCED_HITS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[test]
fn fenced_handler_is_invoked() {
    use pebbles_widgets::text;
    pebbles_widgets::theme::init();
    FENCED_HITS.with(|c| c.set(0));
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 800.0);
    let doc = "```mermaid\ngraph TD; A-->B;\n```\n\n```rust\nfn x() {}\n```\n";
    ui.mount_root(
        View::new(
            palette::WHITE,
            markdown(doc).fenced_handler(|lang, code| {
                if lang == "mermaid" {
                    FENCED_HITS.with(|c| c.set(c.get() + 1));
                    Some(text(format!("[diagram: {} chars]", code.len())).into_widget())
                } else {
                    None // fall through to normal code rendering
                }
            }),
        )
        .into_widget(),
    );
    ui.layout(&mut env, win);
    assert_eq!(
        FENCED_HITS.with(std::cell::Cell::get),
        1,
        "the mermaid fence was handled; the rust fence fell through"
    );
}

// ---------------------------------------------------------------------------
// §9: collapsible headings + find-highlight render without panic
// ---------------------------------------------------------------------------

#[test]
fn collapsible_headings_and_find_render() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(600.0, 800.0);
    let query = create_signal(String::from("world"));
    let doc = "# One\n\nhello world\n\n## Two\n\nmore text\n";
    ui.mount_root(
        View::new(
            palette::WHITE,
            markdown(doc).collapsible_headings().find(query),
        )
        .into_widget(),
    );
    ui.layout(&mut env, win);
    let with_query = ui.element_count();
    assert!(
        with_query > 8,
        "collapsible + find builds a tree ({with_query})"
    );

    // Clearing the query re-renders (no panic on the reactive path).
    query.set(String::new());
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(ui.element_count() > 5);
}

// ---------------------------------------------------------------------------
// §7: a wide table renders through the table-layout + horizontal-scroll path
// ---------------------------------------------------------------------------

#[test]
fn wide_table_renders_without_panic() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(400.0, 800.0); // narrow viewport → the table must scroll
    let doc = "\
| Alpha | Beta | Gamma | Delta | Epsilon | Zeta |\n\
|:------|:----:|------:|-------|---------|------|\n\
| one two three | b | c | d | e | f |\n\
| 1 | 2 | 3 | 4 | 5 | 6 |\n";
    ui.mount_root(View::new(palette::WHITE, markdown(doc)).into_widget());
    ui.layout(&mut env, win);
    assert!(
        ui.element_count() > 12,
        "a 6-column table builds a real tree ({})",
        ui.element_count()
    );
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
    assert!(
        before > 20,
        "the GFM document produced a real tree ({before} elements)"
    );

    // Append a heading → the view re-renders with more elements.
    let src = SRC.with(|c| c.borrow().expect("src"));
    src.update(|s| s.push_str("\n## More\n\nwords here\n"));
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    assert!(
        ui.element_count() > before,
        "live edit re-rendered the view"
    );

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
    assert_ne!(
        edit_count, read_count,
        "Read renders the document, not the source pane"
    );

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
    assert_eq!(
        paras.len(),
        1,
        "one flow = one paragraph, got {}",
        paras.len()
    );
    let text = &tree
        .object_ref(paras[0])
        .downcast_ref::<RenderParagraph>()
        .unwrap()
        .text;
    assert_eq!(text, "alpha beta gamma", "spaces survive into the layout");
    // The shaped width is wider than the glyphs alone (spaces carry advance).
    assert!(
        tree.size_of(paras[0]).width > 80.0,
        "sentence measures like a sentence"
    );
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
                markdown("Some **bold** and `code` plus a [link](https://x.y) end").on_link(|_| {})
            }),
        )
        .into_widget(),
    );
    ui.layout(&mut env, Size::new(600.0, 400.0));
    let tree = ui.render_tree();
    let paras = tree.find_all::<RenderParagraph>();
    assert_eq!(paras.len(), 1, "one flow = one paragraph");
    let p = tree
        .object_ref(paras[0])
        .downcast_ref::<RenderParagraph>()
        .unwrap();
    assert_eq!(p.text, "Some bold and code plus a link end");
    let bold = p
        .spans
        .iter()
        .find(|s| s.weight == Some(600.0))
        .expect("bold span");
    assert_eq!(&p.text[bold.range.clone()], "bold");
    let chip = p
        .spans
        .iter()
        .find(|s| s.chip.is_some())
        .expect("chip span");
    assert_eq!(&p.text[chip.range.clone()], "code");
    let link = p
        .spans
        .iter()
        .find(|s| s.link.is_some())
        .expect("link span");
    assert_eq!(&p.text[link.range.clone()], "link");
    assert!(link.underline, "links underline");
    // The paragraph published the link's laid-out boxes for the tap resolver.
    let boxes = p.link_boxes.as_ref().expect("link geometry cell").borrow();
    assert_eq!(boxes.len(), 1, "a one-line link publishes one box");
    let (r, ix) = boxes[0];
    assert_eq!(ix, 0);
    assert!(
        r.width() > 5.0 && r.height() > 5.0,
        "the box covers real glyphs: {r:?}"
    );
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
    let p = tree
        .object_ref(paras[0])
        .downcast_ref::<RenderParagraph>()
        .unwrap();
    assert!(
        p.text.contains("\n    let x = 1;"),
        "real newlines + real indentation: {:?}",
        p.text
    );
    assert!(
        !p.style.soft_wrap,
        "code never soft-wraps; lines break at newlines only"
    );
    assert!(
        p.spans.iter().any(|s| s.color.is_some()),
        "tokens carry color spans"
    );
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
