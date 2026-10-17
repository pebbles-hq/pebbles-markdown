//! Headless render for the demo: `SHOT=<w>:<h>:<out.rgba>` rasterizes the app to raw
//! RGBA through the framework's offscreen renderer (pebbles::shell::capture::OffscreenCapture) (no window, no display) so the editor can be screenshotted
//! in CI or over SSH. Convert the dump to PNG with any tool (e.g. PIL).

use pebbles::core::Ui;
use pebbles::prelude::*;
use pebbles::render::{Scene, TextEnv};
use pebbles::shell::capture::OffscreenCapture;

fn scene_for(ui: &mut Ui, env: &mut TextEnv, w: u32, h: u32) -> Scene {
    let size = Size::new(f64::from(w), f64::from(h));
    ui.make_current();
    let mut scene = Scene::new();
    for _ in 0..4 {
        ui.rebuild_if_dirty();
        ui.layout(env, size);
        scene = Scene::new();
        if !ui.paint(env, &mut scene) {
            break;
        }
    }
    scene
}

/// `SHOT=<w>:<h>:<out.rgba>` — render the demo app once and dump tight RGBA.
pub fn shot(
    spec: &str,
    root: fn() -> AnyWidget,
    base: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let p: Vec<&str> = spec.split(':').collect();
    let (w, h, out) = (p[0].parse::<u32>()?, p[1].parse::<u32>()?, p[2]);
    // Mirror App::run()'s reduced-motion detection (the capture path bypasses run), so
    // `PEBBLES_REDUCED_MOTION=1 SHOT=...` renders the static, animation-free state.
    if let Ok(v) = std::env::var("PEBBLES_REDUCED_MOTION")
        && matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    {
        set_prefers_reduced_motion(true);
    }
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let mut gpu = OffscreenCapture::new()?;
    let mut env = TextEnv::new();
    let mut ui = Ui::new();
    ui.make_current();
    ui.mount_root(View::new(base, OverlayHost::wrap(root().into_widget())).into_widget());
    pebbles::widgets::overlay::set_window_size(f64::from(w), f64::from(h));
    let scene = scene_for(&mut ui, &mut env, w, h);
    let px = gpu.render_scene(&scene, w, h, base)?;
    std::fs::write(out, &px)?;
    println!("{w} {h} {out}");
    Ok(())
}
