//! The reader widget — the [`Markdown`] builder, its per-render context ([`Cx`]),
//! and the block/heading-collapse orchestration. The crate's public reading
//! entry point; the per-block widget building lives in [`render`](crate::render).

use std::rc::Rc;

use pebbles_foundation::{Color, CrossAxisAlignment, EdgeInsets, MainAxisSize, TextAlign};
use pebbles_render::{Cursor, lucide};

use pebbles_core::widget::{AnyWidget, IntoWidget};
use pebbles_core::{Signal, children, component_props};
use pebbles_widgets::{
    SelectionGroup, SemanticsRole, column, expanded, gap_w, gesture_detector, padding, row,
    selection_group, semantics,
};

use crate::*;

#[derive(Clone)]
pub(crate) enum Source {
    Fixed(String),
    Bound(Signal<String>),
}

/// A rendered Markdown document. Build with [`markdown`] (fixed text) or
/// [`markdown().bind(sig)`](Markdown::bind) (live source: edits re-render, task
/// checkboxes rewrite the source).
pub struct Markdown {
    pub(crate) source: Source,
    pub(crate) style: Option<MarkdownStyle>,
    pub(crate) on_link: Option<LinkFn>,
    pub(crate) on_task: Option<TaskFn>,
    pub(crate) virtualized: bool,
    pub(crate) collapsible: bool,
    pub(crate) find: Option<Signal<String>>,
    pub(crate) fenced: Option<FencedFn>,
    pub(crate) resolver: Option<ResolverFn>,
    pub(crate) on_link_hover: Option<LinkHoverFn>,
    pub(crate) selectable: bool,
    pub(crate) keyboard: bool,
    pub(crate) block_renderer: Option<BlockRenderFn>,
    pub(crate) backlinks: Vec<(String, String)>,
    pub(crate) allow_all_schemes: bool,
}

/// A fenced-code handler: given `(language, code)`, return a widget to render in
/// place of the highlighted block (e.g. a diagram), or `None` to fall through.
pub(crate) type FencedFn = Rc<dyn Fn(&str, &str) -> Option<AnyWidget>>;

/// A per-block renderer override: `(kind, plain_text)` → an optional replacement
/// widget. `kind` is `"heading"`, `"paragraph"`, `"code"`, `"quote"`, `"list"`,
/// `"table"`, `"callout"`, … — return `None` to fall through to default rendering.
pub(crate) type BlockRenderFn = Rc<dyn Fn(&str, &str) -> Option<AnyWidget>>;

/// A vault resolver: given a wikilink/embed target (e.g. `"Some Note"`), return
/// its Markdown content — used to transclude `![[Note]]` and preview links.
pub(crate) type ResolverFn = Rc<dyn Fn(&str) -> Option<String>>;

/// Guard against runaway `![[...]]` transclusion cycles.
pub(crate) const MAX_EMBED_DEPTH: u8 = 4;

/// Render `source` as Markdown (GFM: tables, task lists, strikethrough).
pub fn markdown(source: impl Into<String>) -> Markdown {
    Markdown {
        source: Source::Fixed(source.into()),
        style: None,
        on_link: None,
        on_task: None,
        virtualized: false,
        collapsible: false,
        find: None,
        fenced: None,
        resolver: None,
        on_link_hover: None,
        selectable: false,
        keyboard: false,
        block_renderer: None,
        backlinks: Vec::new(),
        allow_all_schemes: false,
    }
}

impl Markdown {
    /// Render from (and write task toggles back to) a live `Signal<String>`.
    pub fn bind(mut self, source: Signal<String>) -> Self {
        self.source = Source::Bound(source);
        self
    }
    /// Override the visual style (default: [`MarkdownStyle::from_theme`]).
    pub fn style(mut self, s: MarkdownStyle) -> Self {
        self.style = Some(s);
        self
    }
    /// Called with the URL when a link is clicked (open a browser, route
    /// internally, …). Links render inert without it.
    pub fn on_link(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.on_link = Some(Rc::new(f));
        self
    }
    /// Called after a task checkbox toggles with `(ordinal, now_checked)` —
    /// the bound source (if any) has already been rewritten.
    pub fn on_task(mut self, f: impl Fn(usize, bool) + 'static) -> Self {
        self.on_task = Some(Rc::new(f));
        self
    }
    /// Render through the virtualized list: only the blocks in (or within one
    /// cache margin of) the viewport are BUILT — the reader becomes its own
    /// scroll view, so give it a bounded box on the main axis (a fixed height
    /// or an `Expanded` slot). This is how a multi-megabyte document stays a
    /// few hundred widgets no matter its length.
    pub fn virtualized(mut self) -> Self {
        self.virtualized = true;
        self
    }
    /// Make headings clickable to fold/unfold the section beneath them (until the
    /// next heading of the same or a higher level). Off by default; not applied in
    /// [`virtualized`](Self::virtualized) mode.
    pub fn collapsible_headings(mut self) -> Self {
        self.collapsible = true;
        self
    }
    /// Highlight every case-insensitive match of a live query in the rendered
    /// text (find-in-document). The app owns the query field / the scroll
    /// viewport; the reader highlights the matches.
    pub fn find(mut self, query: Signal<String>) -> Self {
        self.find = Some(query);
        self
    }
    /// Register a handler for fenced code blocks: `f(language, code)` returns a
    /// widget to render instead of the highlighted block (e.g. a `mermaid`
    /// diagram), or `None` to fall through to normal code rendering.
    pub fn fenced_handler(mut self, f: impl Fn(&str, &str) -> Option<AnyWidget> + 'static) -> Self {
        self.fenced = Some(Rc::new(f));
        self
    }
    /// Register a vault resolver: `f(target)` returns the Markdown content for a
    /// wikilink/embed target. `![[Note]]` embeds then transclude the resolved
    /// content inline (bounded against cycles); links/embeds preview it on hover.
    pub fn resolver(mut self, f: impl Fn(&str) -> Option<String> + 'static) -> Self {
        self.resolver = Some(Rc::new(f));
        self
    }
    /// Called as the pointer moves over inline links: `Some(url)` when over a link,
    /// `None` when it leaves — for a status-bar URL / hover preview.
    pub fn on_link_hover(mut self, f: impl Fn(Option<&str>) + 'static) -> Self {
        self.on_link_hover = Some(Rc::new(f));
        self
    }
    /// Make rendered text **selectable**: drag within a text block to select, and
    /// the selection copies to the clipboard on release. Opt-in (a drag otherwise
    /// scrolls); selection is per-block (drag stays within one paragraph/heading).
    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }
    /// Make inline links **keyboard-navigable** (accessibility): each paragraph
    /// with links becomes a Tab focus stop, the arrow keys cycle its links with a
    /// visible focus ring, and Enter/Space activates the focused link through
    /// [`on_link`](Self::on_link). Link paragraphs opt out of drag-selection so
    /// focus stays clean; non-link text still selects when
    /// [`selectable`](Self::selectable) is on.
    pub fn keyboard_links(mut self) -> Self {
        self.keyboard = true;
        self
    }
    /// Override how any block renders: `f(kind, plain_text)` returns a widget to
    /// render in place of that block, or `None` to fall through. `kind` is one of
    /// `"heading"`, `"paragraph"`, `"code"`, `"quote"`, `"callout"`, `"list"`,
    /// `"table"`, `"rule"`, `"math"`, `"details"`, `"toc"`, `"frontmatter"`.
    pub fn block_renderer(mut self, f: impl Fn(&str, &str) -> Option<AnyWidget> + 'static) -> Self {
        self.block_renderer = Some(Rc::new(f));
        self
    }
    /// Append a **Backlinks** section (notes that link here) at the document end.
    /// Each entry is `(label, target)`; targets route through `on_link`. Backlink
    /// data comes from the host's vault index — the widget renders it.
    pub fn backlinks(mut self, links: impl IntoIterator<Item = (String, String)>) -> Self {
        self.backlinks = links.into_iter().collect();
        self
    }
    /// Opt out of link-scheme sanitization. By default only safe schemes
    /// (`http(s)`, `mailto`, `tel`, `ftp(s)`, `sms`) and scheme-less links are
    /// clickable; `javascript:` / `data:` / … render as inert text. Call this to
    /// make every link clickable regardless of scheme.
    pub fn allow_all_link_schemes(mut self) -> Self {
        self.allow_all_schemes = true;
        self
    }
}

pub(crate) struct MdProps {
    pub(crate) source: Source,
    pub(crate) style: Option<MarkdownStyle>,
    pub(crate) on_link: Option<LinkFn>,
    pub(crate) on_task: Option<TaskFn>,
    pub(crate) virtualized: bool,
    pub(crate) collapsible: bool,
    pub(crate) find: Option<Signal<String>>,
    pub(crate) fenced: Option<FencedFn>,
    pub(crate) resolver: Option<ResolverFn>,
    pub(crate) on_link_hover: Option<LinkHoverFn>,
    pub(crate) selectable: bool,
    pub(crate) keyboard: bool,
    pub(crate) block_renderer: Option<BlockRenderFn>,
    pub(crate) backlinks: Vec<(String, String)>,
    pub(crate) allow_all_schemes: bool,
}

impl IntoWidget for Markdown {
    fn into_widget(mut self) -> AnyWidget {
        component_props(
            render_markdown,
            MdProps {
                source: self.source.clone(),
                style: self.style.take(),
                on_link: self.on_link.take(),
                on_task: self.on_task.take(),
                virtualized: self.virtualized,
                collapsible: self.collapsible,
                find: self.find.take(),
                fenced: self.fenced.take(),
                resolver: self.resolver.take(),
                on_link_hover: self.on_link_hover.take(),
                selectable: self.selectable,
                keyboard: self.keyboard,
                block_renderer: self.block_renderer.take(),
                backlinks: std::mem::take(&mut self.backlinks),
                allow_all_schemes: self.allow_all_schemes,
            },
        )
        .into_widget()
    }
}

/// Render context threaded through the block renderers.
#[derive(Clone)]
pub(crate) struct Cx {
    pub(crate) style: Rc<MarkdownStyle>,
    pub(crate) on_link: Option<LinkFn>,
    pub(crate) on_task: Option<TaskFn>,
    pub(crate) bound: Option<Signal<String>>,
    /// Text color override (block quotes mute their body).
    pub(crate) color: Color,
    /// Active find query (lowercased, non-empty) — matches are highlighted.
    pub(crate) find: Option<String>,
    /// Optional fenced-code handler (e.g. diagrams).
    pub(crate) fenced: Option<FencedFn>,
    /// Optional vault resolver for wikilink/embed transclusion + previews.
    pub(crate) resolver: Option<ResolverFn>,
    /// Optional inline-link hover reporter.
    pub(crate) on_link_hover: Option<LinkHoverFn>,
    /// Render text blocks as selectable (drag to select, copy on release).
    pub(crate) selectable: bool,
    /// Make link-bearing paragraphs keyboard-navigable focus stops.
    pub(crate) keyboard: bool,
    /// Document-wide selection group (drag/copy span blocks) + a per-leaf counter.
    pub(crate) sel_group: Option<SelectionGroup>,
    pub(crate) sel_index: Rc<std::cell::Cell<usize>>,
    /// Optional per-block renderer override.
    pub(crate) block_renderer: Option<BlockRenderFn>,
    /// Current `![[...]]` transclusion depth (cycle guard).
    pub(crate) embed_depth: u8,
    /// When false, links with an unsafe scheme render as inert text.
    pub(crate) allow_all_schemes: bool,
}

pub(crate) fn render_markdown(p: &MdProps) -> AnyWidget {
    // Hooks first (stable order): per-widget fold state + the reactive find query.
    let collapsed = pebbles_core::create_signal(Vec::<String>::new());
    // A document-wide selection group so a drag/copy spans blocks (opt-in).
    let sel_group = p
        .selectable
        .then(|| selection_group(pebbles_core::owner_id().unwrap_or(0) ^ 0x5E1E_D0C0_0000_0000));
    let find = p.find.and_then(|q| {
        let q = q.get().trim().to_lowercase();
        (!q.is_empty()).then_some(q)
    });
    let style = Rc::new(p.style.clone().unwrap_or_else(MarkdownStyle::from_theme));
    let cx = Cx {
        color: style.body_color,
        style,
        on_link: p.on_link.clone(),
        on_task: p.on_task.clone(),
        bound: match &p.source {
            Source::Bound(sig) => Some(*sig),
            Source::Fixed(_) => None,
        },
        find,
        fenced: p.fenced.clone(),
        resolver: p.resolver.clone(),
        on_link_hover: p.on_link_hover.clone(),
        selectable: p.selectable,
        keyboard: p.keyboard,
        sel_group,
        sel_index: Rc::new(std::cell::Cell::new(0)),
        block_renderer: p.block_renderer.clone(),
        embed_depth: 0,
        allow_all_schemes: p.allow_all_schemes,
    };
    // Parse ONCE per source value: a task toggle or theme flip must not re-parse
    // a multi-megabyte document, only re-render blocks. Bound sources memoize
    // through the reactive graph (dependency-driven); fixed sources go through a
    // small content-keyed cache (a bounded memo, not a lifecycle registry).
    let blocks: Rc<Vec<Block>> = match &p.source {
        Source::Bound(sig) => {
            let sig = *sig;
            pebbles_core::create_memo(move || Rc::new(parse_blocks(&sig.get()))).get()
        }
        Source::Fixed(s) => parse_cached(s),
    };
    // Append a host-supplied backlinks section, if any.
    let blocks: Rc<Vec<Block>> = if p.backlinks.is_empty() {
        blocks
    } else {
        let mut v = (*blocks).clone();
        v.push(Block::Backlinks(p.backlinks.clone()));
        Rc::new(v)
    };
    if p.virtualized {
        // Only the blocks in the line of sight (+ cache margin) are BUILT; the
        // rest of the document exists as per-kind extent estimates that real
        // measurements replace as they scroll in.
        let gap = cx.style.block_gap;
        let fallback = 3.0 * f64::from(cx.style.body_size);
        let est_style = cx.style.clone();
        let est_blocks = blocks.clone();
        let count = blocks.len();
        let item_cx = cx.clone();
        return pebbles_widgets::ListView::builder_auto(count, move |i| {
            let inner = render_block(&blocks[i], &item_cx);
            if i + 1 < count {
                padding(EdgeInsets::only(0.0, 0.0, 0.0, gap), inner).into_widget()
            } else {
                inner
            }
        })
        .estimated_extent_of(move |i| estimate_block(&est_blocks[i], &est_style) + gap)
        .estimated_extent(fallback)
        .into_widget();
    }
    if p.collapsible {
        render_collapsible(&blocks, &cx, collapsed)
    } else {
        render_blocks(&blocks, &cx)
    }
}

/// Per-block visibility under a set of collapsed heading slugs: a heading hides
/// every following block until the next heading of the same or a higher level.
pub(crate) fn hidden_flags(blocks: &[Block], collapsed: &[String]) -> Vec<bool> {
    let mut out = vec![false; blocks.len()];
    let mut hide_until: Option<u8> = None;
    for (i, b) in blocks.iter().enumerate() {
        if let Some(lvl) = hide_until {
            match b {
                Block::Heading(l, ..) if *l <= lvl => hide_until = None, // this heading shows
                _ => {
                    out[i] = true;
                    continue;
                }
            }
        }
        if let Block::Heading(l, _, slug) = b
            && collapsed.iter().any(|s| s == slug)
        {
            hide_until = Some(*l);
        }
    }
    out
}

/// Render blocks with fold-on-click headings; sections under a collapsed heading
/// are hidden until the next same-or-higher heading.
pub(crate) fn render_collapsible(
    blocks: &[Block],
    cx: &Cx,
    collapsed: Signal<Vec<String>>,
) -> AnyWidget {
    let set = collapsed.get();
    let hidden = hidden_flags(blocks, &set);
    let mut kids: Vec<AnyWidget> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if hidden[i] {
            continue;
        }
        if let Block::Heading(level, inls, slug) = b {
            let is_collapsed = set.iter().any(|s| s == slug);
            kids.push(render_foldable_heading(
                *level,
                inls,
                slug,
                is_collapsed,
                cx,
                collapsed,
            ));
        } else {
            kids.push(render_block(b, cx));
        }
    }
    column(kids)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .spacing(cx.style.block_gap)
        .into_widget()
}

/// A clickable heading with a fold chevron; tapping toggles its slug in `collapsed`.
pub(crate) fn render_foldable_heading(
    level: u8,
    inls: &[Inline],
    slug: &str,
    is_collapsed: bool,
    cx: &Cx,
    collapsed: Signal<Vec<String>>,
) -> AnyWidget {
    let s = &cx.style;
    let size = s.body_size * s.heading_scale[(level as usize - 1).min(5)];
    let chevron = if is_collapsed {
        lucide::CHEVRON_RIGHT
    } else {
        lucide::CHEVRON_DOWN
    };
    let slug = slug.to_string();
    let label = format!(
        "Heading {level}, {}, {}",
        inline_plain(inls),
        if is_collapsed {
            "collapsed"
        } else {
            "expanded"
        }
    );
    let w = gesture_detector(
        row(children![
            pebbles_widgets::components::icon(chevron)
                .size(f64::from(size * 0.7))
                .color(s.heading_color),
            gap_w(6.0),
            expanded(inline_flow(
                inls,
                cx,
                size,
                s.heading_color,
                true,
                TextAlign::Start
            )),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .main_axis_size(MainAxisSize::Min),
    )
    .cursor(Cursor::Pointer)
    .on_tap(move || {
        collapsed.update(|v| match v.iter().position(|x| x == &slug) {
            Some(i) => {
                v.remove(i);
            }
            None => v.push(slug.clone()),
        });
    });
    semantics(SemanticsRole::Button, label, w).into_widget()
}

/// A cheap per-kind extent guess for a block (pre-measurement): keeps the
/// virtual list's scrollbar stable and deep jumps accurate. Real layout
/// measurements replace these as blocks scroll into view.
pub(crate) fn estimate_block(b: &Block, s: &MarkdownStyle) -> f64 {
    let line = f64::from(s.body_size) * 1.5;
    fn inline_len(inls: &[Inline]) -> usize {
        inls.iter()
            .map(|i| match i {
                Inline::Run(r) => r.text.len(),
                Inline::Image { .. } => 200,
                Inline::Footnote(_) => 2,
                Inline::Math { tex, .. } => tex.len(),
            })
            .sum()
    }
    match b {
        Block::FrontMatter(props) => {
            (props.len().max(1) as f64) * (f64::from(s.body_size) + 8.0) + 12.0
        }
        Block::Heading(l, _, _) => {
            f64::from(s.body_size * s.heading_scale[(*l as usize - 1).min(5)]) * 1.6
        }
        Block::Para(inls) => {
            let chars = inline_len(inls).max(1) as f64;
            (chars / 90.0).ceil().max(1.0) * line
        }
        Block::Code { text, .. } => {
            (text.lines().count().max(1) as f64) * f64::from(s.body_size * 0.92) * 1.5 + 44.0
        }
        Block::Quote(body) => body.iter().map(|b| estimate_block(b, s)).sum::<f64>() + 4.0,
        Block::Callout { body, .. } => {
            body.iter().map(|b| estimate_block(b, s)).sum::<f64>() + f64::from(s.body_size) + 16.0
        }
        Block::DefList(entries) => {
            entries
                .iter()
                .map(|(_, defs)| {
                    defs.iter()
                        .map(|b| estimate_block(b, s))
                        .sum::<f64>()
                        .max(line)
                        + line
                })
                .sum::<f64>()
                + 8.0
        }
        Block::Details { body, .. } => {
            body.iter().map(|b| estimate_block(b, s)).sum::<f64>() + f64::from(s.body_size) + 16.0
        }
        Block::Toc(entries) => (entries.len().max(1) as f64) * line + 12.0,
        Block::List { items, .. } => {
            items
                .iter()
                .map(|it| {
                    it.blocks
                        .iter()
                        .map(|b| estimate_block(b, s))
                        .sum::<f64>()
                        .max(line)
                        + 4.0
                })
                .sum::<f64>()
                + 8.0
        }
        Block::Rule => 1.0,
        Block::Table { rows, .. } => {
            ((rows.len() + 1) as f64) * (f64::from(s.body_size) + 10.0) + 8.0
        }
        Block::Footnotes(entries) => {
            entries
                .iter()
                .map(|(_, blocks)| {
                    blocks
                        .iter()
                        .map(|b| estimate_block(b, s))
                        .sum::<f64>()
                        .max(line)
                })
                .sum::<f64>()
                + f64::from(s.body_size)
                + 12.0
        }
        Block::Backlinks(links) => {
            (links.len().max(1) as f64) * line + f64::from(s.body_size) + 12.0
        }
    }
}

pub(crate) fn render_blocks(blocks: &[Block], cx: &Cx) -> AnyWidget {
    let kids: Vec<AnyWidget> = blocks.iter().map(|b| render_block(b, cx)).collect();
    column(kids)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .spacing(cx.style.block_gap)
        .into_widget()
}

// ---------------------------------------------------------------------------
