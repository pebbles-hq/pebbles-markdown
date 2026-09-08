//! Interaction storm: click/drag/double-click/type EVERYWHERE over a Markdown
//! editor whose source is full of multi-byte characters (em dashes) — hunting
//! byte-boundary panics that only show up under real mouse use.

use pebbles_core::{IntoWidget, KeyInput, Motion, Ui, component, create_signal};
use pebbles_foundation::{Offset, Size, palette};
use pebbles_render::TextEnv;
use pebbles_markdown::{MarkdownMode, markdown_editor};
use pebbles_widgets::View;

const DEMO: &str = "# Pebbles Markdown\n\nA GFM document rendered **live** \u{2014} edit the source on the left and watch it\nupdate. *Italic*, **bold**, ***both***, ~~strikethrough~~ and `inline code`\nall flow inside wrapped paragraphs, and [links are clickable](https://example.com).\n\n## Task list \u{2014} click the checkboxes\n\n- [x] Parse GFM (tables, tasks, strikethrough)\n- [ ] Toggle me \u{2014} the SOURCE rewrites\n- [ ] Ship an IDE on Pebbles\n\n> Block quotes carry whole blocks \u{2014}\n> including **formatting** and nested content.\n\n```rust\nfn main() {\n    println!(\"JetBrains Mono, bundled\");\n}\n```\n";

thread_local! {
    static MODE: std::cell::RefCell<Option<pebbles_core::Signal<MarkdownMode>>> =
        const { std::cell::RefCell::new(None) };
}

fn storm_root() -> impl IntoWidget {
    let src = create_signal(DEMO.to_string());
    let mode = create_signal(MarkdownMode::Edit);
    MODE.with(|c| *c.borrow_mut() = Some(mode));
    markdown_editor(src).mode_signal(mode).lines(24)
}

#[test]
fn interaction_storm_over_multibyte_markdown() {
    pebbles_widgets::theme::init();
    pebbles_widgets::overlay::init();
    pebbles_core::focus::init();
    pebbles_core::keyboard::set_modifiers(false, false, false, false);

    let mut ui = Ui::new();
    let mut env = TextEnv::new();
    let win = Size::new(760.0, 640.0);
    ui.mount_root(View::new(palette::WHITE, component(storm_root)).into_widget());
    ui.layout(&mut env, win);
    let frame = |ui: &mut Ui, env: &mut TextEnv| {
        ui.rebuild_if_dirty();
        ui.layout(env, win);
        let mut scene = pebbles_render::Scene::new();
        ui.paint(env, &mut scene);
    };
    frame(&mut ui, &mut env);

    let modes = [MarkdownMode::Edit, MarkdownMode::Split, MarkdownMode::Read];
    for (mi, m) in modes.iter().enumerate() {
        MODE.with(|c| c.borrow().expect("mode")).set(*m);
        frame(&mut ui, &mut env);
        let mut step = 0;
        let mut y = 4.0;
        while y < 630.0 {
            let mut x = 4.0;
            while x < 750.0 {
                let p = Offset::new(x, y);
                ui.dispatch_pointer_down(p);
                ui.dispatch_tap(p);
                ui.dispatch_pointer_up(p);
                if step % 3 == 0 {
                    ui.dispatch_double_tap(p); // word selection — boundary math
                }
                if step % 5 == 0 {
                    // drag-select across the line (multi-byte hit-testing)
                    let q = Offset::new((x + 90.0).min(748.0), y);
                    if let Some(t) = ui.pan_target_at(p) {
                        ui.dispatch_pan_start(t, p);
                        ui.dispatch_pan_update(t, q);
                        ui.dispatch_pan_end(t, q);
                    }
                }
                if step % 7 == 0 {
                    // edit at the caret: motions + deletions + typing
                    ui.dispatch_key(KeyInput::Move { motion: Motion::WordLeft, extend: true });
                    ui.dispatch_key(KeyInput::Move { motion: Motion::Right, extend: false });
                    ui.dispatch_key(KeyInput::Backspace);
                    ui.dispatch_key(KeyInput::Insert("é\u{2014}x".to_string()));
                    ui.dispatch_key(KeyInput::Move { motion: Motion::LineEnd, extend: true });
                    ui.dispatch_key(KeyInput::Delete);
                }
                frame(&mut ui, &mut env);
                step += 1;
                x += 37.0;
            }
            y += 23.0;
        }
        eprintln!("mode {mi} survived {step} interaction points");
    }
}
