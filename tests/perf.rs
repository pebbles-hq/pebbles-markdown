//! Perf tripwires: the huge-document fixture through a headless Ui.
//!
//! Prints wall times + the `pebbles_render::stats` pipeline counters for the
//! worst-case markdown document. The assertions are the viewport-bounded
//! rendering gates and tighten as the phases land (culling, scroll-is-paint,
//! spans, virtualization, caches). Run with `--nocapture` to read the numbers.

use std::time::Instant;

use pebbles_core::{IntoWidget, Ui, component};
use pebbles_foundation::{Size, palette};
use pebbles_render::{TextEnv, stats};
use pebbles_markdown::markdown;
use pebbles_widgets::View;

/// A smaller cut of the gallery's deterministic stress document (CI-fast, same
/// block mix): styled paragraphs, fenced code, tables, quotes/lists, tasks, and
/// one pathological long paragraph.
fn huge_gfm(paragraphs: usize) -> String {
    const WORDS: [&str; 16] = [
        "viewport", "render", "signal", "widget", "layout", "scene", "glyph", "arena",
        "frame", "paint", "scroll", "anchor", "extent", "cache", "measure", "pebbles",
    ];
    let mut s = String::with_capacity(paragraphs * 320);
    s.push_str("# Stress document\n\n");
    for p in 0..paragraphs {
        if p % 50 == 0 {
            s.push_str(&format!("\n## Section {}\n\n", p / 50));
        }
        for w in 0..40usize {
            let word = WORDS[(p + w) % WORDS.len()];
            match (p + w) % 23 {
                0 => s.push_str(&format!("**{word}** ")),
                7 => s.push_str(&format!("*{word}* ")),
                11 => s.push_str(&format!("`{word}` ")),
                17 => s.push_str(&format!("[{word}](https://example.com/{word}) ")),
                _ => {
                    s.push_str(word);
                    s.push(' ');
                }
            }
        }
        s.push_str("\n\n");
        if p % 25 == 0 {
            s.push_str("```rust\n");
            for l in 0..12usize {
                s.push_str(&format!("fn item_{p}_{l}(x: u64) -> u64 {{ x + {l} }}\n"));
            }
            s.push_str("```\n\n");
        }
        if p % 40 == 0 {
            s.push_str("| a | b | c |\n|---|---|---|\n| 1 | `2` | **3** |\n| 4 | 5 | 6 |\n\n");
        }
        if p % 30 == 0 {
            s.push_str(&format!("- [ ] task {p}\n- [x] done {p}\n\n> quoted **line**\n\n"));
        }
    }
    // Pathological: one huge unbroken paragraph (~24k chars).
    s.push_str("\n## Pathological\n\n");
    for i in 0..3000usize {
        s.push_str(WORDS[i % WORDS.len()]);
        s.push(' ');
    }
    s.push('\n');
    s
}

thread_local! {
    static DOC: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn fixture_root() -> impl IntoWidget {
    markdown(DOC.with(|d| d.borrow().clone()))
}

#[test]
fn huge_markdown_document_headless_pipeline() {
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(900.0, 700.0);
    let doc = huge_gfm(400);
    eprintln!("[perf huge-md] source bytes={}", doc.len());
    DOC.with(|d| *d.borrow_mut() = doc);

    ui.mount_root(View::new(palette::WHITE, component(fixture_root)).into_widget());

    stats::reset_frame();
    let t = Instant::now();
    ui.rebuild_if_dirty();
    let build = t.elapsed();

    let t = Instant::now();
    ui.layout(&mut env, win);
    let layout_cold = t.elapsed();

    let mut scene = pebbles_render::Scene::new();
    let t = Instant::now();
    ui.paint(&mut env, &mut scene);
    let paint = t.elapsed();

    let nodes = ui.render_node_count();
    let elements = ui.element_count();
    eprintln!(
        "[perf huge-md] cold: build={build:?} layout={layout_cold:?} paint={paint:?} \
         elements={elements} nodes={nodes} layouts={} skips={} painted={} culled={} glyph_runs={}",
        stats::layout_calls(),
        stats::layout_skips(),
        stats::painted_nodes(),
        stats::culled_nodes(),
        stats::glyph_runs(),
    );

    // What a scroll frame costs pre-P1 (proxy): a 1-px resize forces the same
    // full relayout + full re-encode a scroll tick triggers today.
    stats::reset_frame();
    let t = Instant::now();
    ui.layout(&mut env, Size::new(win.width, win.height + 1.0));
    let layout_warm = t.elapsed();
    let mut scene2 = pebbles_render::Scene::new();
    let t = Instant::now();
    ui.paint(&mut env, &mut scene2);
    let paint_warm = t.elapsed();
    eprintln!(
        "[perf huge-md] scroll-frame proxy: layout={layout_warm:?} paint={paint_warm:?} \
         layouts={} skips={} painted={} culled={} glyph_runs={}",
        stats::layout_calls(),
        stats::layout_skips(),
        stats::painted_nodes(),
        stats::culled_nodes(),
        stats::glyph_runs(),
    );

    assert!(elements > 1_000, "the fixture produced a real tree ({elements} elements)");
    assert!(stats::painted_nodes() > 0, "paint traversed the tree");
    // P0 gates: paint is viewport-bounded — only a small fraction of the tree
    // encodes (culled counts subtree ROOTS; their descendants are never visited),
    // and the scene's glyph count tracks the window, not the document.
    assert!(
        stats::painted_nodes() * 10 < nodes as u64,
        "painted nodes are a small fraction of the tree (painted {} of {nodes})",
        stats::painted_nodes(),
    );
    assert!(stats::culled_nodes() > 0, "offscreen subtrees actually culled");
    assert!(
        stats::glyph_runs() < 4_000,
        "encoded glyph runs are viewport-bounded ({})",
        stats::glyph_runs(),
    );
}

// ---------------------------------------------------------------------------
// Virtualized reader: only line-of-sight blocks exist, however big the source.
// ---------------------------------------------------------------------------

fn virtual_root() -> impl IntoWidget {
    pebbles_widgets::container()
        .height(680.0)
        .child(markdown(DOC.with(|d| d.borrow().clone())).virtualized())
}

#[test]
fn huge_markdown_virtualized_stays_viewport_bounded() {
    pebbles_widgets::overlay::init();
    pebbles_core::focus::init();
    pebbles_widgets::theme::init();
    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(900.0, 700.0);
    DOC.with(|d| *d.borrow_mut() = huge_gfm(400));

    ui.mount_root(View::new(palette::WHITE, component(virtual_root)).into_widget());
    let t = Instant::now();
    ui.rebuild_if_dirty();
    ui.layout(&mut env, win);
    // Corrective passes: auto-measure feeds real extents back for a frame or two.
    for _ in 0..4 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, win);
    }
    let cold = t.elapsed();
    let nodes = ui.render_node_count();
    let elements = ui.element_count();
    let mut scene = pebbles_render::Scene::new();
    stats::reset_frame();
    ui.paint(&mut env, &mut scene);
    eprintln!(
        "[perf huge-md virtual] cold={cold:?} elements={elements} nodes={nodes} painted={} glyph_runs={}",
        stats::painted_nodes(),
        stats::glyph_runs(),
    );
    assert!(nodes < 900, "resident tree is O(viewport), not O(document): {nodes} nodes");

    // Fragments: after a warm frame, a repeat paint re-encodes NOTHING — every
    // clean item re-appends its retained fragment (P4).
    let mut scene2 = pebbles_render::Scene::new();
    stats::reset_frame();
    ui.paint(&mut env, &mut scene2);
    assert_eq!(
        stats::fragments_encoded(),
        0,
        "a clean frame re-encodes no fragments (reused {})",
        stats::fragments_reused(),
    );
    assert!(stats::fragments_reused() > 0, "fragments were re-appended");

    // Scroll deep: the window slides; the resident tree stays bounded.
    ui.dispatch_scroll(pebbles_foundation::Offset::new(450.0, 350.0), 5_000.0);
    for _ in 0..6 {
        ui.rebuild_if_dirty();
        ui.layout(&mut env, win);
    }
    let nodes_after = ui.render_node_count();
    assert!(
        nodes_after < 900,
        "after a deep scroll the window stays bounded: {nodes_after} nodes"
    );
}
