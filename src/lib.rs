//! # pebbles-markdown
//!
//! An Obsidian-style Markdown reader + editor **widget for
//! [Pebbles](https://github.com/pebbles-hq/pebbles)**, maintained as a separate
//! package. It is a worked example of a third-party Pebbles widget crate: it
//! depends only on Pebbles' *public* API (`pebbles-widgets` for the catalog +
//! theme, `pebbles-core` for the reactive/widget contract, `pebbles-render` /
//! `pebbles-foundation` for primitives) and composes them into new widgets —
//! nothing here reaches into framework internals. Drop [`markdown`] or
//! [`markdown_editor`] into any Pebbles tree exactly like a built-in widget.
//!
//! **Reading** ([`markdown`]): full GFM via pulldown-cmark — headings, emphasis /
//! strong / strikethrough, inline code, fenced code blocks (JetBrains Mono),
//! links (clickable — wire [`Markdown::on_link`]), block quotes (nested), ordered +
//! unordered lists (nested), **task lists with live checkboxes** (toggling rewrites
//! the bound source, Obsidian-style), tables, horizontal rules, and images (rendered
//! with the `image-view` feature, alt-text otherwise).
//!
//! **Editing** ([`markdown_editor`]): a `Signal<String>`-bound editor with three
//! modes — [`MarkdownMode::Edit`] (source), [`MarkdownMode::Split`] (source + live
//! preview), [`MarkdownMode::Read`] (rendered only). The mode is a signal you can
//! own ([`MarkdownEditor::mode_signal`]) — build your own mode switcher, the widget
//! ships no chrome (the same philosophy as the file explorer).
//!
//! **Theming**: everything visual lives in [`MarkdownStyle`] — heading scale, body
//! size/color, code block/inline colors + family, quote bar, link color, rules,
//! tables. Defaults derive from the live [`theme()`](pebbles_widgets::theme), so it
//! follows light/dark automatically; pass your own via [`Markdown::style`].

use std::rc::Rc;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use pebbles_foundation::{Color, CrossAxisAlignment, EdgeInsets, MainAxisSize, palette};
use pebbles_render::{Border, BorderRadius, BoxDecoration, Cursor, lucide};

use pebbles_widgets::theme::{mix, theme};
use pebbles_widgets::{
    Container, Expanded, GestureDetector, Padding, TextSpan, column, gap_w, row, span, text,
    text_rich, wrap,
};
use pebbles_core::widget::{AnyWidget, IntoWidget};
use pebbles_core::{Signal, children, component_props};

// ---------------------------------------------------------------------------
// Style — the theming surface
// ---------------------------------------------------------------------------

/// Every visual knob of the Markdown renderer. Build one with
/// [`MarkdownStyle::from_theme`] and override fields, or construct it fully.
#[derive(Clone)]
pub struct MarkdownStyle {
    /// Body font size (px); headings scale from it via `heading_scale`.
    pub body_size: f32,
    pub body_color: Color,
    pub heading_color: Color,
    /// Per-level multipliers over `body_size` (H1..H6).
    pub heading_scale: [f32; 6],
    /// Optional font family for headings (`None` = the default family).
    pub heading_family: Option<String>,
    pub code_bg: Color,
    pub code_color: Color,
    /// The monospace family for code ("JetBrains Mono" is bundled).
    pub code_family: String,
    /// Syntax-highlight palette for fenced code blocks (Obsidian-style). Every
    /// color is themeable; the default scheme reads on both light and dark code
    /// backgrounds.
    pub syntax: SyntaxColors,
    pub quote_bar: Color,
    pub quote_color: Color,
    pub link_color: Color,
    pub rule_color: Color,
    pub table_border: Color,
    /// Vertical gap between blocks (px).
    pub block_gap: f64,
}

/// The token colors used to syntax-highlight fenced code blocks. Every field is
/// a plain [`Color`], so a theme can recolor the whole scheme.
#[derive(Clone)]
pub struct SyntaxColors {
    pub keyword: Color,
    pub string: Color,
    pub comment: Color,
    pub number: Color,
    /// Function/type/builtin identifiers.
    pub ident: Color,
    /// Punctuation and operators.
    pub punct: Color,
}

impl SyntaxColors {
    /// A default scheme derived from the palette — medium-saturation colors that
    /// stay legible on both a light and a dark code background.
    pub fn from_theme() -> Self {
        let c = theme().colors;
        SyntaxColors {
            keyword: palette::violet::S500,
            string: palette::emerald::S600,
            comment: c.muted_foreground,
            number: palette::amber::S600,
            ident: palette::sky::S600,
            punct: mix(c.foreground, c.background, 0.25),
        }
    }
}

impl MarkdownStyle {
    /// Defaults derived from the live theme (follows light/dark).
    pub fn from_theme() -> Self {
        let c = theme().colors;
        MarkdownStyle {
            body_size: 14.5,
            body_color: c.foreground,
            heading_color: c.foreground,
            heading_scale: [1.9, 1.55, 1.3, 1.15, 1.0, 0.9],
            heading_family: None,
            code_bg: mix(c.background, c.foreground, 0.07),
            code_color: c.foreground,
            code_family: "JetBrains Mono".to_string(),
            syntax: SyntaxColors::from_theme(),
            quote_bar: c.border,
            quote_color: c.muted_foreground,
            link_color: c.primary,
            rule_color: c.border,
            table_border: c.border,
            block_gap: 10.0,
        }
    }
}

// ---------------------------------------------------------------------------
// The parsed document model
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
struct Run {
    text: String,
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: Option<String>,
}

#[derive(Clone, PartialEq)]
enum Inline {
    Run(Run),
    Image { url: String, alt: String },
}

#[derive(Clone, PartialEq)]
struct ListItem {
    /// `Some(checked)` for GFM task items; the usize is the document-order
    /// task ordinal (the handle [`toggle_task`] flips).
    task: Option<(bool, usize)>,
    blocks: Vec<Block>,
}

#[derive(Clone, PartialEq)]
enum Block {
    Heading(u8, Vec<Inline>),
    Para(Vec<Inline>),
    Code { lang: String, text: String },
    Quote(Vec<Block>),
    List { ordered: bool, start: u64, items: Vec<ListItem> },
    Rule,
    Table { header: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
}

/// Content-keyed parse cache for FIXED sources (8-entry MRU): re-rendering a
/// static document re-uses the parsed blocks; a different string parses fresh.
fn parse_cached(s: &str) -> Rc<Vec<Block>> {
    use std::hash::{Hash, Hasher};
    thread_local! {
        static CACHE: std::cell::RefCell<Vec<(u64, Rc<Vec<Block>>)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    let key = h.finish();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(pos) = c.iter().position(|(k, _)| *k == key) {
            let hit = c.remove(pos);
            let blocks = hit.1.clone();
            c.push(hit); // MRU to the back
            return blocks;
        }
        let blocks = Rc::new(parse_blocks(s));
        if c.len() >= 8 {
            c.remove(0);
        }
        c.push((key, blocks.clone()));
        blocks
    })
}

/// Parse GFM into the block model (tables + strikethrough + task lists on).
fn parse_blocks(src: &str) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);

    // Frames of nested block containers (document / quote bodies / list items).
    let mut stack: Vec<Vec<Block>> = vec![Vec::new()];
    // In-flight list frames: (ordered, start, finished items).
    let mut lists: Vec<(bool, u64, Vec<ListItem>)> = Vec::new();
    // The pending task marker for the item currently open.
    let mut item_task: Vec<Option<(bool, usize)>> = Vec::new();
    let mut task_ordinal = 0usize;

    // Inline state.
    let mut inline: Vec<Inline> = Vec::new();
    let (mut bold, mut italic, mut strike) = (0u32, 0u32, 0u32);
    let mut link: Vec<String> = Vec::new();
    let mut heading: Option<u8> = None;
    let mut in_para = false;
    // Code blocks.
    let mut code: Option<(String, String)> = None;
    // Image capture (alt text accumulates between Start/End).
    let mut image: Option<(String, String)> = None;
    // Tables.
    let mut table: Option<(Vec<Vec<Inline>>, Vec<Vec<Vec<Inline>>>)> = None;
    let mut table_row: Vec<Vec<Inline>> = Vec::new();
    let mut in_head = false;

    let push_run = |inline: &mut Vec<Inline>,
                        text: String,
                        bold: u32,
                        italic: u32,
                        strike: u32,
                        code: bool,
                        link: &[String]| {
        if text.is_empty() {
            return;
        }
        inline.push(Inline::Run(Run {
            text,
            bold: bold > 0,
            italic: italic > 0,
            strike: strike > 0,
            code,
            link: link.last().cloned(),
        }));
    };

    for ev in Parser::new_ext(src, opts) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => in_para = true,
                Tag::Heading { level, .. } => {
                    heading = Some(match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    });
                }
                Tag::BlockQuote(_) => stack.push(Vec::new()),
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    // A nested list inside a TIGHT item follows the item's bare
                    // text — flush that text into the item frame first so it isn't
                    // swallowed. (No-op at the top level, where inline is empty.)
                    let content = std::mem::take(&mut inline);
                    if !content.is_empty()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::Para(content));
                    }
                    lists.push((start.is_some(), start.unwrap_or(1), Vec::new()));
                }
                Tag::Item => {
                    stack.push(Vec::new());
                    item_task.push(None);
                }
                Tag::Emphasis => italic += 1,
                Tag::Strong => bold += 1,
                Tag::Strikethrough => strike += 1,
                Tag::Link { dest_url, .. } => link.push(dest_url.to_string()),
                Tag::Image { dest_url, .. } => image = Some((dest_url.to_string(), String::new())),
                Tag::Table(_) => table = Some((Vec::new(), Vec::new())),
                Tag::TableHead => in_head = true,
                Tag::TableRow => table_row = Vec::new(),
                Tag::TableCell => inline.clear(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    in_para = false;
                    let content = std::mem::take(&mut inline);
                    if !content.is_empty()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::Para(content));
                    }
                }
                TagEnd::Heading(_) => {
                    let lvl = heading.take().unwrap_or(1);
                    let content = std::mem::take(&mut inline);
                    if let Some(top) = stack.last_mut() {
                        top.push(Block::Heading(lvl, content));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    let body = stack.pop().unwrap_or_default();
                    if let Some(top) = stack.last_mut() {
                        top.push(Block::Quote(body));
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, text)) = code.take()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::Code { lang, text });
                    }
                }
                TagEnd::Item => {
                    // TIGHT lists emit an item's text as bare `Text` events with no
                    // `Paragraph` wrapper, so `End(Paragraph)` never flushes it —
                    // flush the pending inline into the item here before closing it,
                    // or the text leaks into the following block.
                    let content = std::mem::take(&mut inline);
                    if !content.is_empty()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::Para(content));
                    }
                    let blocks = stack.pop().unwrap_or_default();
                    let task = item_task.pop().flatten();
                    if let Some((_, _, items)) = lists.last_mut() {
                        items.push(ListItem { task, blocks });
                    }
                }
                TagEnd::List(_) => {
                    if let Some((ordered, start, items)) = lists.pop()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::List { ordered, start, items });
                    }
                }
                TagEnd::Emphasis => italic = italic.saturating_sub(1),
                TagEnd::Strong => bold = bold.saturating_sub(1),
                TagEnd::Strikethrough => strike = strike.saturating_sub(1),
                TagEnd::Link => {
                    link.pop();
                }
                TagEnd::Image => {
                    if let Some((url, alt)) = image.take() {
                        inline.push(Inline::Image { url, alt });
                    }
                }
                TagEnd::TableCell => {
                    table_row.push(std::mem::take(&mut inline));
                }
                TagEnd::TableHead => {
                    in_head = false;
                    if let Some((header, _)) = table.as_mut() {
                        *header = std::mem::take(&mut table_row);
                    }
                }
                TagEnd::TableRow => {
                    if let Some((_, rows)) = table.as_mut() {
                        rows.push(std::mem::take(&mut table_row));
                    }
                }
                TagEnd::Table => {
                    if let Some((header, rows)) = table.take()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::Table { header, rows });
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                if let Some((_, buf)) = code.as_mut() {
                    buf.push_str(&t);
                } else if let Some((_, alt)) = image.as_mut() {
                    alt.push_str(&t);
                } else {
                    push_run(&mut inline, t.to_string(), bold, italic, strike, false, &link);
                }
            }
            Event::Code(t) => {
                push_run(&mut inline, t.to_string(), bold, italic, strike, true, &link);
            }
            Event::SoftBreak | Event::HardBreak => {
                push_run(&mut inline, " ".to_string(), bold, italic, strike, false, &link);
            }
            Event::Rule => {
                if let Some(top) = stack.last_mut() {
                    top.push(Block::Rule);
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some(slot) = item_task.last_mut() {
                    *slot = Some((checked, task_ordinal));
                    task_ordinal += 1;
                }
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                push_run(&mut inline, t.to_string(), bold, italic, strike, true, &link);
            }
            _ => {}
        }
        let _ = in_para;
        let _ = in_head;
    }
    stack.pop().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Task toggling in the SOURCE (the Obsidian behavior)
// ---------------------------------------------------------------------------

/// Flip the `ordinal`-th task checkbox (document order) in `source`, returning
/// the rewritten text — `- [ ]` ↔ `- [x]` (also `*`/`+` bullets and `1.`/`1)`
/// ordered tasks). `None` if there is no such task.
pub fn toggle_task(source: &str, ordinal: usize) -> Option<String> {
    let mut seen = 0usize;
    let mut lines: Vec<String> = source.split('\n').map(str::to_string).collect();
    for line in lines.iter_mut() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        // Bullet or ordered-list prefix.
        let after_marker = if let Some(rest) =
            trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).or_else(|| trimmed.strip_prefix("+ "))
        {
            Some((trimmed.len() - rest.len(), rest))
        } else {
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            if digits > 0
                && let Some(sep) = trimmed[digits..].strip_prefix('.').or_else(|| trimmed[digits..].strip_prefix(')'))
                && let Some(rest) = sep.strip_prefix(' ')
            {
                Some((trimmed.len() - rest.len(), rest))
            } else {
                None
            }
        };
        let Some((prefix_len, rest)) = after_marker else { continue };
        let checked = if rest.starts_with("[ ] ") || rest == "[ ]" {
            Some(false)
        } else if rest.starts_with("[x] ") || rest.starts_with("[X] ") || rest == "[x]" || rest == "[X]" {
            Some(true)
        } else {
            None
        };
        let Some(checked) = checked else { continue };
        if seen == ordinal {
            let box_at = indent + prefix_len + 1; // inside the '['
            line.replace_range(box_at..box_at + 1, if checked { " " } else { "x" });
            return Some(lines.join("\n"));
        }
        seen += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// The reader widget
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Source {
    Fixed(String),
    Bound(Signal<String>),
}

/// A rendered Markdown document. Build with [`markdown`] (fixed text) or
/// [`markdown().bind(sig)`](Markdown::bind) (live source: edits re-render, task
/// checkboxes rewrite the source).
pub struct Markdown {
    source: Source,
    style: Option<MarkdownStyle>,
    on_link: Option<Rc<dyn Fn(&str)>>,
    on_task: Option<Rc<dyn Fn(usize, bool)>>,
    virtualized: bool,
}

/// Render `source` as Markdown (GFM: tables, task lists, strikethrough).
pub fn markdown(source: impl Into<String>) -> Markdown {
    Markdown {
        source: Source::Fixed(source.into()),
        style: None,
        on_link: None,
        on_task: None,
        virtualized: false,
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
}

struct MdProps {
    source: Source,
    style: Option<MarkdownStyle>,
    on_link: Option<Rc<dyn Fn(&str)>>,
    on_task: Option<Rc<dyn Fn(usize, bool)>>,
    virtualized: bool,
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
            },
        )
        .into_widget()
    }
}

/// Render context threaded through the block renderers.
#[derive(Clone)]
struct Cx {
    style: Rc<MarkdownStyle>,
    on_link: Option<Rc<dyn Fn(&str)>>,
    on_task: Option<Rc<dyn Fn(usize, bool)>>,
    bound: Option<Signal<String>>,
    /// Text color override (block quotes mute their body).
    color: Color,
}

fn render_markdown(p: &MdProps) -> AnyWidget {
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
                Padding::new(EdgeInsets::only(0.0, 0.0, 0.0, gap), inner).into_widget()
            } else {
                inner
            }
        })
        .estimated_extent_of(move |i| estimate_block(&est_blocks[i], &est_style) + gap)
        .estimated_extent(fallback)
        .into_widget();
    }
    render_blocks(&blocks, &cx)
}

/// A cheap per-kind extent guess for a block (pre-measurement): keeps the
/// virtual list's scrollbar stable and deep jumps accurate. Real layout
/// measurements replace these as blocks scroll into view.
fn estimate_block(b: &Block, s: &MarkdownStyle) -> f64 {
    let line = f64::from(s.body_size) * 1.5;
    fn inline_len(inls: &[Inline]) -> usize {
        inls.iter()
            .map(|i| match i {
                Inline::Run(r) => r.text.len(),
                Inline::Image { .. } => 200,
            })
            .sum()
    }
    match b {
        Block::Heading(l, _) => {
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
        Block::List { items, .. } => {
            items
                .iter()
                .map(|it| {
                    it.blocks.iter().map(|b| estimate_block(b, s)).sum::<f64>().max(line) + 4.0
                })
                .sum::<f64>()
                + 8.0
        }
        Block::Rule => 1.0,
        Block::Table { header: _, rows } => {
            ((rows.len() + 1) as f64) * (f64::from(s.body_size) + 10.0) + 8.0
        }
    }
}

fn render_blocks(blocks: &[Block], cx: &Cx) -> AnyWidget {
    let kids: Vec<AnyWidget> = blocks.iter().map(|b| render_block(b, cx)).collect();
    column(kids)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .spacing(cx.style.block_gap)
        .into_widget()
}

// ---------------------------------------------------------------------------
// Syntax highlighting — a small, dependency-free lexer good enough to color the
// common languages (C-family + a few script styles), Obsidian-style. Not a full
// grammar; it tokenizes comments, strings, numbers, keywords, identifiers and
// punctuation, and colors them from the themeable `SyntaxColors`.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Tok {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    Ident,
    Punct,
}

/// Line-comment prefix and whether the language has C-style `/* */` blocks + JS-y
/// keywords, chosen from the fence's language tag.
fn lang_profile(lang: &str) -> (&'static str, bool) {
    match lang.trim().to_ascii_lowercase().as_str() {
        "python" | "py" | "ruby" | "rb" | "bash" | "sh" | "shell" | "zsh" | "yaml" | "yml"
        | "toml" | "ini" | "r" | "perl" | "makefile" => ("#", false),
        "sql" | "lua" | "haskell" | "hs" | "elm" => ("--", false),
        // C-family default: // line comments + /* */ blocks.
        _ => ("//", true),
    }
}

/// A broad, language-agnostic keyword set — covers Rust/JS/TS/Go/Java/C/Python/…
/// "well enough" for a reader without a per-language grammar.
fn is_keyword(w: &str) -> bool {
    const KW: &[&str] = &[
        "fn", "let", "mut", "const", "static", "pub", "use", "mod", "impl", "trait", "struct",
        "enum", "type", "where", "as", "dyn", "ref", "move", "match", "if", "else", "for", "while",
        "loop", "return", "break", "continue", "in", "self", "Self", "super", "crate", "async",
        "await", "unsafe", "extern", "function", "var", "def", "class", "interface", "extends",
        "implements", "import", "from", "export", "default", "new", "delete", "try", "catch",
        "finally", "throw", "throws", "switch", "case", "do", "public", "private", "protected",
        "abstract", "final", "void", "int", "long", "float", "double", "bool", "boolean", "char",
        "string", "package", "func", "go", "defer", "chan", "map", "range", "nil", "None", "True",
        "False", "true", "false", "null", "undefined", "and", "or", "not", "with", "yield", "lambda",
        "elif", "then", "end", "begin", "val", "object", "override",
    ];
    KW.contains(&w)
}

/// The UTF-8 width of the character starting at byte `i` (which MUST be a char
/// boundary). Falls back to 1 defensively — callers always pass a boundary, so
/// the cursor below only ever lands on boundaries and can never split a char.
#[inline]
fn char_len_at(line: &str, i: usize) -> usize {
    line[i..].chars().next().map_or(1, char::len_utf8)
}

/// Tokenize one line. `in_block` tracks an open `/* … */` across lines.
///
/// The cursor advances by whole CHARACTERS (never raw bytes) and every slice is
/// taken at a char boundary, so any input — accented text, em-dashes, arrows,
/// smart quotes, emoji, CJK — tokenizes safely and always terminates. (A byte
/// cursor treated a UTF-8 lead byte as an ASCII "letter" that its ASCII-only
/// inner loop never consumed, hanging on the first non-ASCII char in a code
/// block; the whole point of this reader is to survive whatever it's handed.)
fn highlight_line(line: &str, line_comment: &str, blocks: bool, in_block: &mut bool) -> Vec<(String, Tok)> {
    let mut out: Vec<(String, Tok)> = Vec::new();
    let b = line.as_bytes();
    let len = line.len();
    let mut i = 0; // ALWAYS a char boundary
    let push = |out: &mut Vec<(String, Tok)>, s: &str, t: Tok| {
        if !s.is_empty() {
            out.push((s.to_string(), t));
        }
    };
    while i < len {
        if *in_block {
            let start = i;
            while i < len {
                if b[i] == b'*' && i + 1 < len && b[i + 1] == b'/' {
                    i += 2;
                    *in_block = false;
                    break;
                }
                i += char_len_at(line, i);
            }
            push(&mut out, &line[start..i.min(len)], Tok::Comment);
            continue;
        }
        let c = line[i..].chars().next().unwrap_or('\0');
        // Whitespace → plain (kept so indentation survives).
        if c == ' ' || c == '\t' {
            let start = i;
            while i < len && (b[i] == b' ' || b[i] == b'\t') {
                i += 1;
            }
            push(&mut out, &line[start..i], Tok::Plain);
            continue;
        }
        // Line comment.
        if line[i..].starts_with(line_comment) {
            push(&mut out, &line[i..], Tok::Comment);
            break;
        }
        // Block comment start (the `/*` is captured by the `in_block` arm above).
        if blocks && line[i..].starts_with("/*") {
            *in_block = true;
            continue;
        }
        // String / char literal.
        if c == '"' || c == '\'' || c == '`' {
            let start = i;
            i += c.len_utf8();
            while i < len {
                let d = line[i..].chars().next().unwrap_or('\0');
                if d == '\\' {
                    // Skip the escape and the whole escaped char (never a byte).
                    i += 1;
                    if i < len {
                        i += char_len_at(line, i);
                    }
                    continue;
                }
                i += d.len_utf8();
                if d == c {
                    break;
                }
            }
            push(&mut out, &line[start..i.min(len)], Tok::Str);
            continue;
        }
        // Number.
        if c.is_ascii_digit() {
            let start = i;
            while i < len && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            push(&mut out, &line[start..i], Tok::Number);
            continue;
        }
        // Identifier / keyword — Unicode-aware, so a non-ASCII word (`café`, a
        // CJK identifier) is one plain token, not an infinite loop.
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < len {
                let d = line[i..].chars().next().unwrap_or('\0');
                if d.is_alphanumeric() || d == '_' {
                    i += d.len_utf8();
                } else {
                    break;
                }
            }
            let w = &line[start..i];
            let kind = if is_keyword(w) {
                Tok::Keyword
            } else if i < len && b[i] == b'(' {
                Tok::Ident // a call → function-ish
            } else {
                Tok::Plain
            };
            push(&mut out, w, kind);
            continue;
        }
        // Punctuation / operator — one whole character (any non-ASCII symbol too).
        let start = i;
        i += c.len_utf8();
        push(&mut out, &line[start..i], Tok::Punct);
    }
    out
}

/// Highlight a whole code block into per-line colored runs.
fn highlight(code: &str, lang: &str, s: &SyntaxColors, base: Color) -> Vec<Vec<(String, Color)>> {
    let (line_comment, blocks) = lang_profile(lang);
    let mut in_block = false;
    code.lines()
        .map(|line| {
            highlight_line(line, line_comment, blocks, &mut in_block)
                .into_iter()
                .map(|(text, tok)| {
                    let color = match tok {
                        Tok::Keyword => s.keyword,
                        Tok::Str => s.string,
                        Tok::Comment => s.comment,
                        Tok::Number => s.number,
                        Tok::Ident => s.ident,
                        Tok::Punct => s.punct,
                        Tok::Plain => base,
                    };
                    (text, color)
                })
                .collect()
        })
        .collect()
}

fn render_block(b: &Block, cx: &Cx) -> AnyWidget {
    let s = &cx.style;
    match b {
        Block::Heading(lvl, inlines) => {
            let size = s.body_size * s.heading_scale[(*lvl as usize - 1).min(5)];
            inline_flow(inlines, cx, size, s.heading_color, true)
        }
        Block::Para(inlines) => inline_flow(inlines, cx, s.body_size, cx.color, false),
        Block::Code { lang, text: code } => {
            let mut kids: Vec<AnyWidget> = Vec::new();
            if !lang.is_empty() {
                kids.push(
                    text(lang.clone())
                        .size(s.body_size * 0.78)
                        .color(mix(s.code_color, s.code_bg, 0.45))
                        .font_family(s.code_family.clone())
                        .into_widget(),
                );
            }
            // ONE rich paragraph for the whole block: per-token color spans, real
            // newlines as hard breaks, no soft wrap — indentation is genuine
            // whitespace inside a single shaped layout, not widget arithmetic.
            let size = s.body_size * 0.92;
            let lines = highlight(code.trim_end(), lang, &s.syntax, s.code_color);
            let mut spans: Vec<TextSpan> = Vec::new();
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    spans.push(span("\n"));
                }
                for (t, color) in line {
                    spans.push(span(t.clone()).color(*color));
                }
            }
            if spans.is_empty() {
                spans.push(span(" "));
            }
            kids.push(
                text_rich(spans)
                    .size(size)
                    .color(s.code_color)
                    .font_family(s.code_family.clone())
                    .soft_wrap(false)
                    .into_widget(),
            );
            Container::new()
                .decoration(BoxDecoration::new().color(s.code_bg).radius(BorderRadius::all(6.0)))
                .padding(EdgeInsets::all(10.0))
                .child(
                    column(kids)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .main_axis_size(MainAxisSize::Min)
                        .spacing(4.0),
                )
                .into_widget()
        }
        Block::Quote(body) => {
            let quoted = Cx { color: s.quote_color, ..cx.clone() };
            row(children![
                Container::new().width(3.0).color(s.quote_bar),
                gap_w(10.0),
                Expanded::new(render_blocks(body, &quoted)),
            ])
            .main_axis_size(MainAxisSize::Min)
            .into_widget()
        }
        Block::List { ordered, start, items } => {
            let mut rows: Vec<AnyWidget> = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let marker: AnyWidget = match item.task {
                    Some((checked, ordinal)) => {
                        let (bound, on_task) = (cx.bound, cx.on_task.clone());
                        GestureDetector::new(
                            pebbles_widgets::components::icon(if checked {
                                lucide::SQUARE_CHECK
                            } else {
                                lucide::SQUARE
                            })
                            .size(15.0)
                            .color(if checked { s.link_color } else { cx.color }),
                        )
                        .cursor(Cursor::Pointer)
                        .on_tap(move || {
                            // Rewrite the bound source (Obsidian behavior), then report.
                            if let Some(sig) = bound
                                && let Some(new) = toggle_task(&sig.peek(), ordinal)
                            {
                                sig.set(new);
                            }
                            if let Some(f) = &on_task {
                                f(ordinal, !checked);
                            }
                        })
                        .into_widget()
                    }
                    None if *ordered => text(format!("{}.", start.saturating_add(i as u64)))
                        .size(s.body_size)
                        .color(cx.color)
                        .into_widget(),
                    None => text("•").size(s.body_size).color(cx.color).into_widget(),
                };
                rows.push(
                    row(children![
                        Container::new().width(22.0).child(marker),
                        Expanded::new(render_blocks(&item.blocks, cx)),
                    ])
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min)
                    .into_widget(),
                );
            }
            Padding::new(
                EdgeInsets::only(8.0, 0.0, 0.0, 0.0),
                column(rows)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .main_axis_size(MainAxisSize::Min)
                    .spacing(4.0),
            )
            .into_widget()
        }
        Block::Rule => Container::new().height(1.0).color(s.rule_color).into_widget(),
        Block::Table { header, rows } => {
            let cell = |inlines: &Vec<Inline>, bold: bool| -> AnyWidget {
                Padding::new(
                    EdgeInsets::symmetric(8.0, 5.0),
                    inline_flow(inlines, cx, cx.style.body_size * 0.95, cx.color, bold),
                )
                .into_widget()
            };
            let mut lines: Vec<AnyWidget> = Vec::new();
            let head_cells: Vec<AnyWidget> =
                header.iter().map(|c| Expanded::new(cell(c, true)).into_widget()).collect();
            lines.push(
                Container::new()
                    .color(mix(theme().colors.background, theme().colors.foreground, 0.04))
                    .child(row(head_cells).main_axis_size(MainAxisSize::Min))
                    .into_widget(),
            );
            for r in rows {
                let cells: Vec<AnyWidget> =
                    r.iter().map(|c| Expanded::new(cell(c, false)).into_widget()).collect();
                lines.push(
                    Container::new()
                        .decoration(BoxDecoration::new().border(Border::new(s.table_border, 0.5)))
                        .child(row(cells).main_axis_size(MainAxisSize::Min))
                        .into_widget(),
                );
            }
            Container::new()
                .decoration(
                    BoxDecoration::new()
                        .border(Border::new(s.table_border, 1.0))
                        .radius(BorderRadius::all(6.0)),
                )
                .child(
                    column(lines)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .main_axis_size(MainAxisSize::Min),
                )
                .into_widget()
        }
    }
}

/// Lay inline runs out as ONE rich paragraph — bold/italic/strike/links/inline
/// code are per-range spans over a single shaped layout, so word wrapping,
/// inter-word spacing, and BiDi are the text engine's job (never a widget per
/// word). Images split the flow: text segments and images compose in a wrap
/// (the rare mixed case).
fn inline_flow(inlines: &[Inline], cx: &Cx, size: f32, color: Color, bold_all: bool) -> AnyWidget {
    let s = &cx.style;
    let heading_family = (size != s.body_size).then(|| s.heading_family.clone()).flatten();

    let mut parts: Vec<AnyWidget> = Vec::new();
    let mut spans: Vec<TextSpan> = Vec::new();
    let flush = |spans: &mut Vec<TextSpan>, parts: &mut Vec<AnyWidget>| {
        if spans.is_empty() {
            return;
        }
        let mut rich = text_rich(std::mem::take(spans)).size(size).color(color);
        if bold_all {
            rich = rich.weight(600.0);
        }
        if let Some(fam) = &heading_family {
            rich = rich.font_family(fam.clone());
        }
        if let Some(f) = &cx.on_link {
            let f = f.clone();
            rich = rich.on_link(move |u| f(u));
        }
        parts.push(rich.into_widget());
    };
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                let mut sp = span(run.text.clone());
                if run.code {
                    sp = sp
                        .chip(s.code_bg)
                        .color(s.code_color)
                        .font_family(s.code_family.clone())
                        .size(size * 0.9);
                } else {
                    if run.bold {
                        sp = sp.semibold();
                    }
                    if run.italic {
                        sp = sp.italic();
                    }
                    if run.strike {
                        sp = sp.strikethrough();
                    }
                }
                if let Some(url) = &run.link {
                    sp = sp.link(url.clone()).underline().color(s.link_color);
                }
                spans.push(sp);
            }
            Inline::Image { url, alt } => {
                flush(&mut spans, &mut parts);
                parts.push(render_image(url, alt, cx));
            }
        }
    }
    flush(&mut spans, &mut parts);
    match parts.len() {
        0 => text_rich(vec![span("")]).size(size).color(color).into_widget(),
        1 => parts.pop().expect("one part"),
        _ => wrap(parts).spacing(4.0).run_spacing(3.0).into_widget(),
    }
}

#[cfg(feature = "image-view")]
fn render_image(url: &str, _alt: &str, _cx: &Cx) -> AnyWidget {
    let view = if url.starts_with("http://") || url.starts_with("https://") {
        pebbles_widgets::ImageView::network(url)
    } else {
        pebbles_widgets::ImageView::asset(url)
    };
    view.height(200.0).radius(BorderRadius::all(6.0)).into_widget()
}

#[cfg(not(feature = "image-view"))]
fn render_image(_url: &str, alt: &str, cx: &Cx) -> AnyWidget {
    // Without the image stack, show the alt text as a quiet placeholder.
    text(format!("[{alt}]"))
        .size(cx.style.body_size)
        .color(cx.style.quote_color)
        .italic()
        .into_widget()
}

// ---------------------------------------------------------------------------
// The editor
// ---------------------------------------------------------------------------

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
    on_link: Option<Rc<dyn Fn(&str)>>,
    lines: u32,
}

/// A Markdown editor over a live `Signal<String>` (default mode: Split).
pub fn markdown_editor(source: Signal<String>) -> MarkdownEditor {
    MarkdownEditor { source, mode: None, style: None, on_link: None, lines: 16 }
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
}

struct EdProps {
    source: Signal<String>,
    mode: Option<Signal<MarkdownMode>>,
    style: Option<MarkdownStyle>,
    on_link: Option<Rc<dyn Fn(&str)>>,
    lines: u32,
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
            },
        )
        .into_widget()
    }
}

fn render_editor(p: &EdProps) -> AnyWidget {
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
            Expanded::new(editor()),
            gap_w(12.0),
            Expanded::new(preview(preview_src, false)),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .main_axis_size(MainAxisSize::Min)
        .into_widget(),
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    fn inline_text(inls: &[Inline]) -> String {
        inls.iter()
            .filter_map(|i| match i {
                Inline::Run(r) => Some(r.text.clone()),
                Inline::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    fn first_para_text(b: &Block) -> String {
        match b {
            Block::Para(inls) | Block::Heading(_, inls) => inline_text(inls),
            _ => String::new(),
        }
    }

    // Regression: a TIGHT list emits item text as bare `Text` (no Paragraph), so
    // the item text must be flushed into the item — not leaked into the next block.
    #[test]
    fn tight_list_items_keep_their_text_and_dont_leak_into_the_next_heading() {
        let blocks = parse_blocks("- [x] alpha\n- [ ] beta\n\n## Heading\n");
        assert_eq!(blocks.len(), 2, "one list + one heading, got {}", blocks.len());
        let Block::List { items, .. } = &blocks[0] else { panic!("first block should be a list") };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].task.map(|(c, _)| c), Some(true));
        assert_eq!(items[1].task.map(|(c, _)| c), Some(false));
        assert!(first_para_text(&items[0].blocks[0]).contains("alpha"), "item 0 keeps its text");
        assert!(first_para_text(&items[1].blocks[0]).contains("beta"), "item 1 keeps its text");
        let Block::Heading(_, inls) = &blocks[1] else { panic!("second block should be a heading") };
        assert_eq!(inline_text(inls).trim(), "Heading", "heading must not absorb the list text");
    }

    #[test]
    fn syntax_highlight_colors_the_common_tokens() {
        let (red, green, gray, orange, blue, black, white) = (
            Color::from_rgba8(255, 0, 0, 255),
            Color::from_rgba8(0, 255, 0, 255),
            Color::from_rgba8(128, 128, 128, 255),
            Color::from_rgba8(255, 128, 0, 255),
            Color::from_rgba8(0, 0, 255, 255),
            Color::from_rgba8(0, 0, 0, 255),
            Color::from_rgba8(255, 255, 255, 255),
        );
        let s = SyntaxColors {
            keyword: red,
            string: green,
            comment: gray,
            number: orange,
            ident: blue,
            punct: black,
        };
        let code = "fn main() {\n    let x = \"hi\"; // note\n    let n = 42;\n}";
        let flat: Vec<(String, Color)> =
            highlight(code, "rust", &s, white).into_iter().flatten().collect();
        let colored = |needle: &str| flat.iter().find(|(t, _)| t == needle).map(|(_, c)| *c);
        assert_eq!(colored("fn"), Some(red), "keyword");
        assert_eq!(colored("let"), Some(red), "keyword");
        assert_eq!(colored("\"hi\""), Some(green), "string");
        assert_eq!(colored("// note"), Some(gray), "comment");
        assert_eq!(colored("42"), Some(orange), "number");
        assert_eq!(colored("main"), Some(blue), "function-call identifier");
        // Indentation is preserved as a plain (base-colored) whitespace run.
        assert!(flat.iter().any(|(t, c)| t.trim().is_empty() && !t.is_empty() && *c == white));
    }

    #[test]
    fn nested_list_text_stays_in_its_item() {
        let blocks = parse_blocks("1. Ordered\n2. Nested\n   - child\n");
        let Block::List { items, .. } = &blocks[0] else { panic!("list") };
        assert_eq!(items.len(), 2);
        assert!(first_para_text(&items[0].blocks[0]).contains("Ordered"));
        assert!(first_para_text(&items[1].blocks[0]).contains("Nested"), "nested item keeps its text");
        assert!(
            items[1].blocks.iter().any(|b| matches!(b, Block::List { .. })),
            "the nested list is inside item 1"
        );
    }

    // Regression: the byte-cursor highlighter hung (infinite loop) or panicked on
    // the FIRST non-ASCII character in a code block — a UTF-8 lead byte read as an
    // ASCII "letter" its ASCII-only inner loop never consumed. The char-cursor
    // version must terminate on anything and round-trip the text exactly.
    #[test]
    fn highlight_survives_non_ascii_code() {
        let base = Color::from_rgba8(1, 2, 3, 255);
        let colors = SyntaxColors {
            keyword: base, string: base, comment: base, number: base, ident: base, punct: base,
        };
        // Accents, em-dash, arrow, smart quotes, emoji, CJK, math — each was a
        // freeze; each must now tokenize AND reproduce the source verbatim.
        let cases = [
            "let café = 1; // caffè",
            "x — y → z",
            "let 数 = \"文字列\";",
            "let party = \"🎉🎊\"; /* 世界 */",
            "const naïve = `üñïçödé`;",
            "a ± b ≤ c ∈ 𝕊",
            "/* block ünïçödé without close",
        ];
        for src in cases {
            let lines = highlight(src, "rust", &colors, base);
            // Concatenating every token of every line reproduces the input exactly
            // (nothing dropped, no char split) — and, crucially, it RETURNED.
            let round: String =
                lines.iter().flat_map(|l| l.iter().map(|(t, _)| t.as_str())).collect();
            assert_eq!(round, src, "highlight round-trips {src:?} without loss");
        }
    }

    // A block comment left open at end-of-line carries across lines; unicode
    // inside it must not split a char at the line join.
    #[test]
    fn highlight_open_block_comment_with_unicode() {
        let base = Color::from_rgba8(0, 0, 0, 255);
        let colors = SyntaxColors {
            keyword: base, string: base, comment: base, number: base, ident: base, punct: base,
        };
        let src = "before /* café\n世界 */ after";
        let lines = highlight(src, "rust", &colors, base);
        let round: String =
            lines.iter().flat_map(|l| l.iter().map(|(t, _)| t.as_str())).collect();
        assert_eq!(round, "before /* café世界 */ after");
    }

    // An absurd (>9-digit) ordered start isn't a valid list marker in CommonMark,
    // so it degrades to a PARAGRAPH — the "show it as text" fallback — rather than
    // crashing. A valid max-width start (999999999) renders its markers with no
    // overflow (the renderer's `start + index` is saturating regardless).
    #[test]
    fn absurd_ordered_start_degrades_to_text() {
        let blocks = parse_blocks("18446744073709551615. one\n2. two\n");
        assert!(
            !matches!(blocks.first(), Some(Block::List { .. })),
            "a 20-digit start is not a list marker — it stays plain text"
        );
        // A valid 9-digit start really is a list, and computing markers for it
        // must not overflow.
        let valid = parse_blocks("999999999. one\n1. two\n");
        let Some(Block::List { start, items, .. }) = valid.first() else {
            panic!("9-digit start is a valid ordered list");
        };
        for (i, _) in items.iter().enumerate() {
            let _ = start.saturating_add(i as u64); // mirrors the renderer; no panic
        }
    }
}
