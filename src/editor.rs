//! The editor — a `Signal<String>`-bound [`MarkdownEditor`] with Edit / Split /
//! Read modes. Ships no chrome: the app owns the mode signal and builds any
//! switcher around it. The Split preview follows the source through a debounce.

use pebbles_foundation::{CrossAxisAlignment, MainAxisSize};

use pebbles_core::widget::{AnyWidget, IntoWidget};
use pebbles_core::{Signal, children, component_props};
use pebbles_widgets::{expanded, gap_w, row};

use crate::*;

/// The editor's display mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkdownMode {
    /// Source only.
    Edit,
    /// Source + live preview side by side.
    Split,
    /// Rendered only.
    Read,
}

/// A `Signal<String>`-bound Markdown editor with Edit / Split / Read modes.
/// Build with [`markdown_editor`]; own the mode via [`mode_signal`](Self::mode_signal)
/// and build your own switcher — the widget ships no chrome.
pub struct MarkdownEditor {
    source: Signal<String>,
    mode: Option<Signal<MarkdownMode>>,
    style: Option<MarkdownStyle>,
    on_link: Option<LinkFn>,
    lines: u32,
    collapsible: bool,
    find: Option<Signal<String>>,
}

/// A Markdown editor over a live `Signal<String>` (default mode: Split).
pub fn markdown_editor(source: Signal<String>) -> MarkdownEditor {
    MarkdownEditor {
        source,
        mode: None,
        style: None,
        on_link: None,
        lines: 16,
        collapsible: false,
        find: None,
    }
}

impl MarkdownEditor {
    /// Drive the mode from YOUR signal (build any switcher UI around it).
    pub fn mode_signal(mut self, mode: Signal<MarkdownMode>) -> Self {
        self.mode = Some(mode);
        self
    }
    /// Override the preview style.
    pub fn style(mut self, s: MarkdownStyle) -> Self {
        self.style = Some(s);
        self
    }
    /// Link-click handler for the preview.
    pub fn on_link(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.on_link = Some(Rc::new(f));
        self
    }
    /// The source pane's MINIMUM line count (default 16). The pane auto-grows
    /// with the content past this — the editor never scrolls internally. To box
    /// it, wrap the whole widget in a scroll area.
    pub fn lines(mut self, n: u32) -> Self {
        self.lines = n;
        self
    }
    /// Make headings in the rendered preview fold on click ([`Markdown::collapsible_headings`]).
    pub fn collapsible_headings(mut self) -> Self {
        self.collapsible = true;
        self
    }
    /// Highlight a live query in the rendered preview ([`Markdown::find`]).
    pub fn find(mut self, query: Signal<String>) -> Self {
        self.find = Some(query);
        self
    }
}

pub(crate) struct EdProps {
    source: Signal<String>,
    mode: Option<Signal<MarkdownMode>>,
    style: Option<MarkdownStyle>,
    on_link: Option<LinkFn>,
    lines: u32,
    collapsible: bool,
    find: Option<Signal<String>>,
}

impl IntoWidget for MarkdownEditor {
    fn into_widget(mut self) -> AnyWidget {
        component_props(
            render_editor,
            EdProps {
                source: self.source,
                mode: self.mode.take(),
                style: self.style.take(),
                on_link: self.on_link.take(),
                lines: self.lines,
                collapsible: self.collapsible,
                find: self.find,
            },
        )
        .into_widget()
    }
}

pub(crate) fn render_editor(p: &EdProps) -> AnyWidget {
    use pebbles_widgets::components::text_area;
    // Hooks first (stable order across mode flips). The SPLIT preview follows the
    // source through a ~150 ms debounce so typing never races the parser; Read
    // mode stays directly bound (no typing happening there).
    let source = p.source;
    let preview_src = pebbles_core::create_signal(source.peek());
    // One id-keyed timer per editor instance: re-registering REPLACES the pending
    // timer, which is exactly debounce semantics — only the last edit in a quiet
    // window syncs the preview.
    let debounce_id = pebbles_core::owner_id().unwrap_or(0) ^ 0xDEB0_0000_0000_0000;
    pebbles_core::create_effect(move || {
        let _ = source.get(); // subscribe to every edit
        pebbles_core::set_timeout(debounce_id, 0.15, move || {
            preview_src.set(source.peek());
        });
    });
    pebbles_core::create_cleanup(move || pebbles_core::clear_timeout(debounce_id));
    let mode = match p.mode {
        Some(sig) => sig.get(),
        None => MarkdownMode::Split,
    };
    let preview = |src: Signal<String>, write_back: bool| {
        let mut md = markdown("").bind(src);
        if let Some(s) = p.style.clone() {
            md = md.style(s);
        }
        if p.collapsible {
            md = md.collapsible_headings();
        }
        if let Some(q) = p.find {
            md = md.find(q);
        }
        if let Some(f) = p.on_link.clone() {
            let f = f.clone();
            md = md.on_link(move |u| f(u));
        }
        if !write_back {
            // The debounced mirror is display-only: checkbox toggles rewrite the
            // REAL source (the mirror re-syncs through the debounce).
            md = md.on_task(move |ordinal, _| {
                if let Some(new) = toggle_task(&source.peek(), ordinal) {
                    source.set(new);
                }
            });
        }
        md.into_widget()
    };
    let editor = || {
        // Auto-grow: the pane is as tall as the source (never shorter than
        // `lines`), so the editor shows the FULL content with no internal
        // scrolling — same contract as the rendered view. Boxing is the app's
        // job: wrap the widget in a scroll area.
        let rows = p.source.get().split('\n').count() as u32;
        text_area(rows.max(p.lines))
            .bind(p.source)
            .placeholder("Write some Markdown…")
            .into_widget()
    };
    match mode {
        MarkdownMode::Edit => editor(),
        MarkdownMode::Read => preview(source, true),
        MarkdownMode::Split => row(children![
            expanded(editor()),
            gap_w(12.0),
            expanded(preview(preview_src, false)),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .main_axis_size(MainAxisSize::Min)
        .into_widget(),
    }
}
