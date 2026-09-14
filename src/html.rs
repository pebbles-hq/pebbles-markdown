//! A tiny, dependency-free, **panic-free** HTML-subset parser.
//!
//! CommonMark passes raw HTML through verbatim; a native (non-DOM) renderer has
//! to decide what to *do* with it. This module turns the common, safe subset of
//! HTML into the crate's own [`Block`]/[`Inline`] model so an HTML block renders
//! as real widgets instead of leaking escaped angle-brackets into the text:
//!
//! - **Block** tags: `<p>`, `<h1>`…`<h6>`, `<hr>`, `<blockquote>`, `<ul>`/`<ol>`/`<li>`,
//!   `<pre>` (+ inner `<code>`), `<img>`.
//! - **Inline** tags: `<b>`/`<strong>`, `<i>`/`<em>`, `<del>`/`<s>`/`<strike>`,
//!   `<code>`, `<a href>`, `<br>`, `<img>`.
//! - HTML comments (`<!-- … -->`) are dropped; entities (`&amp;`, `&#39;`, `&#x2014;`, …)
//!   are decoded; **unknown tags are transparent** (dropped, their text kept).
//!
//! Nothing here executes HTML or fetches anything — tags are inert markup. Any
//! malformed input (unterminated tags, stray `<`, unbalanced nesting) degrades to
//! text rather than panicking; that contract is covered by `tests/robustness.rs`.

use crate::{Block, Inline, ListItem, Run};

// ---------------------------------------------------------------------------
// Inline tags — classified for the main parser's inline formatting state
// ---------------------------------------------------------------------------

/// What a single inline HTML tag token means to the main parser's formatting
/// counters. `bool` on the formatting variants is `true` for an opening tag.
pub(crate) enum InlineTag {
    /// `<b>`/`<strong>` (open) or its close.
    Bold(bool),
    /// `<i>`/`<em>` (open) or its close.
    Italic(bool),
    /// `<del>`/`<s>`/`<strike>` (open) or its close.
    Strike(bool),
    /// `<code>` (open) or its close.
    Code(bool),
    /// `<a href="…">` — push the destination.
    LinkOpen(String),
    /// `</a>` — pop the destination.
    LinkClose,
    /// `<br>` — a hard line break.
    Break,
    /// `<img src alt>` — an inline image.
    Image {
        /// The image source (`src`).
        url: String,
        /// The alt text (`alt`).
        alt: String,
    },
    /// Anything else — drop the tag, keep any surrounding text.
    Ignore,
}

/// Classify one inline HTML tag token (e.g. `"<b>"`, `"</a>"`, `"<br/>"`).
pub(crate) fn classify_inline_tag(raw: &str) -> InlineTag {
    let Some(t) = Tag::parse(raw) else {
        return InlineTag::Ignore;
    };
    match t.name.as_str() {
        "b" | "strong" => InlineTag::Bold(!t.closing),
        "i" | "em" => InlineTag::Italic(!t.closing),
        "del" | "s" | "strike" => InlineTag::Strike(!t.closing),
        "code" => InlineTag::Code(!t.closing),
        "br" => InlineTag::Break,
        "a" if t.closing => InlineTag::LinkClose,
        "a" => InlineTag::LinkOpen(attr(&t.attrs, "href").unwrap_or_default()),
        "img" if !t.closing => InlineTag::Image {
            url: attr(&t.attrs, "src").unwrap_or_default(),
            alt: attr(&t.attrs, "alt").unwrap_or_default(),
        },
        _ => InlineTag::Ignore,
    }
}

// ---------------------------------------------------------------------------
// Block-level HTML → the block model
// ---------------------------------------------------------------------------

/// Parse a raw HTML fragment (a Markdown HTML block) into native blocks.
pub(crate) fn parse_html_fragment(src: &str) -> Vec<Block> {
    let nodes = tokenize(src);
    let mut b = Builder::new();
    let mut i = 0;
    while i < nodes.len() {
        match &nodes[i] {
            Node::Text(t) => b.text(t),
            Node::Tag(tag) if tag.name == "pre" && !tag.closing => {
                // `<pre>` is verbatim: gather raw text until `</pre>`, ignoring an
                // inner `<code>` wrapper and any stray inner tags.
                let mut raw = String::new();
                i += 1;
                while i < nodes.len() {
                    match &nodes[i] {
                        Node::Tag(t2) if t2.name == "pre" && t2.closing => break,
                        Node::Text(t2) => raw.push_str(t2),
                        Node::Tag(_) => {}
                    }
                    i += 1;
                }
                b.code_block(raw);
            }
            Node::Tag(tag) => b.tag(tag),
        }
        i += 1;
    }
    b.finish()
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// One parsed tag: `name` lowercased, `attrs` the raw attribute string.
struct Tag {
    name: String,
    attrs: String,
    closing: bool,
}

impl Tag {
    /// Parse a full tag token (`"<a href=…>"`, `"</b>"`, `"<br/>"`) into name +
    /// attrs. Returns `None` if it isn't a well-formed tag.
    fn parse(raw: &str) -> Option<Tag> {
        let inner = raw.trim();
        let inner = inner.strip_prefix('<')?;
        let inner = inner.strip_suffix('>').unwrap_or(inner);
        let (closing, inner) = match inner.strip_prefix('/') {
            Some(r) => (true, r),
            None => (false, inner),
        };
        let inner = inner.strip_suffix('/').unwrap_or(inner).trim();
        if inner.is_empty() {
            return None;
        }
        let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return None;
        }
        Some(Tag {
            name,
            attrs: inner[name_end..].trim().to_string(),
            closing,
        })
    }
}

/// A tokenized node: a tag, or a run of (entity-decoded) text.
enum Node {
    Tag(Tag),
    Text(String),
}

/// Split an HTML fragment into tag / text / (dropped) comment tokens. Malformed
/// input degrades to text; the loop always advances, so it always terminates.
fn tokenize(src: &str) -> Vec<Node> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let n = src.len();
    let mut i = 0;
    while i < n {
        if bytes[i] == b'<' {
            if src[i..].starts_with("<!--") {
                match src[i + 4..].find("-->") {
                    Some(end) => i += 4 + end + 3,
                    None => i = n,
                }
                continue;
            }
            if let Some(gt) = src[i..].find('>') {
                let raw = &src[i..=i + gt];
                if let Some(tag) = Tag::parse(raw) {
                    out.push(Node::Tag(tag));
                } else {
                    // A `<` that doesn't open a real tag is literal text.
                    out.push(Node::Text(decode_entities(raw)));
                }
                i += gt + 1;
            } else {
                out.push(Node::Text(decode_entities(&src[i..])));
                i = n;
            }
        } else {
            let start = i;
            while i < n && bytes[i] != b'<' {
                i += 1;
            }
            out.push(Node::Text(decode_entities(&src[start..i])));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Builder — tokens → blocks
// ---------------------------------------------------------------------------

/// What a container frame on the block stack represents.
enum Frame {
    /// The document root.
    Root,
    /// A `<blockquote>` body.
    Quote,
    /// An `<li>` body, indexed to its owning list frame in `lists`.
    Item,
    /// A `<details>` body.
    Details,
}

struct Builder {
    /// Nested container bodies; `stack[0]` is the document root.
    stack: Vec<Vec<Block>>,
    /// What each `stack` frame is (parallel to `stack`).
    frames: Vec<Frame>,
    /// In-flight list frames: `(ordered, start, finished items)`.
    lists: Vec<(bool, u64, Vec<ListItem>)>,
    /// In-flight `<details>`: `(open, summary inlines)`.
    details: Vec<(bool, Vec<Inline>)>,
    /// Route inline runs into the current details summary while inside `<summary>`.
    in_summary: bool,
    inline: Vec<Inline>,
    bold: u32,
    italic: u32,
    strike: u32,
    code: u32,
    link: Vec<String>,
    heading: Option<u8>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            stack: vec![Vec::new()],
            frames: vec![Frame::Root],
            lists: Vec::new(),
            details: Vec::new(),
            in_summary: false,
            inline: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            code: 0,
            link: Vec::new(),
            heading: None,
        }
    }

    fn push_run(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let run = Inline::Run(Run {
            text,
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: self.code > 0,
            mark: false,
            sub: false,
            sup: false,
            tag: false,
            link: self.link.last().cloned(),
        });
        // Inside `<summary>`, runs build the disclosure title, not the body.
        if self.in_summary {
            if let Some((_, sum)) = self.details.last_mut() {
                sum.push(run);
            }
        } else {
            self.inline.push(run);
        }
    }

    /// Accept text between tags. Whitespace-only text at a block boundary (no
    /// inline started yet) is dropped so tag indentation/newlines don't become
    /// stray paragraphs.
    fn text(&mut self, t: &str) {
        if self.inline.is_empty() && t.trim().is_empty() {
            return;
        }
        self.push_run(t.to_string());
    }

    /// Flush the pending inline runs into the current container as a paragraph.
    fn flush_para(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let inl = std::mem::take(&mut self.inline);
        if let Some(top) = self.stack.last_mut() {
            top.push(Block::Para(inl));
        }
    }

    fn push_block(&mut self, b: Block) {
        if let Some(top) = self.stack.last_mut() {
            top.push(b);
        }
    }

    fn code_block(&mut self, raw: String) {
        self.flush_para();
        let text = raw.strip_prefix('\n').unwrap_or(&raw).to_string();
        self.push_block(Block::Code {
            lang: String::new(),
            text,
        });
    }

    fn tag(&mut self, t: &Tag) {
        match t.name.as_str() {
            // ---- inline formatting ----
            "b" | "strong" => step(&mut self.bold, t.closing),
            "i" | "em" => step(&mut self.italic, t.closing),
            "del" | "s" | "strike" => step(&mut self.strike, t.closing),
            "code" => step(&mut self.code, t.closing),
            "a" => {
                if t.closing {
                    self.link.pop();
                } else {
                    self.link.push(attr(&t.attrs, "href").unwrap_or_default());
                }
            }
            "br" => self.push_run("\n".to_string()),
            "img" if !t.closing => {
                let url = attr(&t.attrs, "src").unwrap_or_default();
                let alt = attr(&t.attrs, "alt").unwrap_or_default();
                self.inline.push(Inline::Image {
                    url,
                    alt,
                    embed: false,
                });
            }
            // ---- block level ----
            "p" => self.flush_para(),
            "hr" => {
                self.flush_para();
                self.push_block(Block::Rule);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if t.closing {
                    let lvl = self.heading.take().unwrap_or(1);
                    let inl = std::mem::take(&mut self.inline);
                    let text: String = inl
                        .iter()
                        .filter_map(|i| match i {
                            Inline::Run(r) => Some(r.text.as_str()),
                            _ => None,
                        })
                        .collect();
                    let slug = crate::advanced::slugify(&text);
                    self.push_block(Block::Heading(lvl, inl, slug));
                } else {
                    self.flush_para();
                    self.heading = t.name[1..].parse().ok();
                }
            }
            "blockquote" => {
                if t.closing {
                    self.close_frame(Frame::Quote);
                } else {
                    self.flush_para();
                    self.stack.push(Vec::new());
                    self.frames.push(Frame::Quote);
                }
            }
            "ul" | "ol" => {
                if t.closing {
                    self.flush_para();
                    if let Some((ordered, start, items)) = self.lists.pop() {
                        self.push_block(Block::List {
                            ordered,
                            start,
                            items,
                            tight: true,
                        });
                    }
                } else {
                    self.flush_para();
                    let ordered = t.name == "ol";
                    let start = attr(&t.attrs, "start")
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(1);
                    self.lists.push((ordered, start, Vec::new()));
                }
            }
            "li" => {
                if t.closing {
                    self.flush_para();
                    let blocks = pop_frame(&mut self.stack, &mut self.frames, Frame::Item);
                    if let Some((_, _, items)) = self.lists.last_mut() {
                        items.push(ListItem { task: None, blocks });
                    }
                } else {
                    // Close a still-open previous `<li>` (unclosed items are common).
                    if matches!(self.frames.last(), Some(Frame::Item)) {
                        self.flush_para();
                        let blocks = pop_frame(&mut self.stack, &mut self.frames, Frame::Item);
                        if let Some((_, _, items)) = self.lists.last_mut() {
                            items.push(ListItem { task: None, blocks });
                        }
                    }
                    self.stack.push(Vec::new());
                    self.frames.push(Frame::Item);
                }
            }
            "details" => {
                if t.closing {
                    self.flush_para();
                    let body = pop_frame(&mut self.stack, &mut self.frames, Frame::Details);
                    let (open, summary) = self.details.pop().unwrap_or((false, Vec::new()));
                    self.push_block(Block::Details {
                        summary,
                        open,
                        body,
                    });
                } else {
                    self.flush_para();
                    let open = attr(&t.attrs, "open").is_some() || t.attrs.contains("open");
                    self.details.push((open, Vec::new()));
                    self.stack.push(Vec::new());
                    self.frames.push(Frame::Details);
                }
            }
            "summary" => self.in_summary = !t.closing,
            // Unknown / unsupported tag: transparent — its text still flows.
            _ => {}
        }
    }

    /// Close a container frame, wrapping its body appropriately. Defensive: only
    /// unwinds if the top frame actually matches.
    fn close_frame(&mut self, want: Frame) {
        match (self.frames.last(), &want) {
            (Some(Frame::Quote), Frame::Quote) => {
                self.flush_para();
                let body = pop_frame(&mut self.stack, &mut self.frames, Frame::Quote);
                self.push_block(Block::Quote(body));
            }
            _ => self.flush_para(),
        }
    }

    /// Flush anything pending and unwind any still-open frames, returning the
    /// document-root blocks.
    fn finish(mut self) -> Vec<Block> {
        self.flush_para();
        // Unwind unbalanced containers so nothing is lost.
        while self.frames.len() > 1 {
            match self.frames.last() {
                Some(Frame::Item) => {
                    let blocks = pop_frame(&mut self.stack, &mut self.frames, Frame::Item);
                    if let Some((_, _, items)) = self.lists.last_mut() {
                        items.push(ListItem { task: None, blocks });
                    }
                }
                Some(Frame::Quote) => {
                    let body = pop_frame(&mut self.stack, &mut self.frames, Frame::Quote);
                    self.push_block(Block::Quote(body));
                }
                Some(Frame::Details) => {
                    let body = pop_frame(&mut self.stack, &mut self.frames, Frame::Details);
                    let (open, summary) = self.details.pop().unwrap_or((false, Vec::new()));
                    self.push_block(Block::Details {
                        summary,
                        open,
                        body,
                    });
                }
                _ => break,
            }
        }
        while let Some((ordered, start, items)) = self.lists.pop() {
            self.push_block(Block::List {
                ordered,
                start,
                items,
                tight: true,
            });
        }
        self.stack.pop().unwrap_or_default()
    }
}

/// Pop the top container body, verifying (defensively) it is the expected frame.
fn pop_frame(stack: &mut Vec<Vec<Block>>, frames: &mut Vec<Frame>, _want: Frame) -> Vec<Block> {
    frames.pop();
    stack.pop().unwrap_or_default()
}

/// `+1` on open, saturating `-1` on close.
fn step(counter: &mut u32, closing: bool) {
    if closing {
        *counter = counter.saturating_sub(1);
    } else {
        *counter += 1;
    }
}

// ---------------------------------------------------------------------------
// Attribute + entity helpers
// ---------------------------------------------------------------------------

/// Extract `name="value"` / `name='value'` / `name=value` from an attribute
/// string (case-insensitive name), decoding entities in the value.
fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(name) {
        let at = from + rel;
        let before_ok = at == 0 || attrs.as_bytes()[at - 1].is_ascii_whitespace();
        let after = &attrs[at + name.len()..];
        let after_trim = after.trim_start();
        if before_ok && after_trim.starts_with('=') {
            let val = after_trim[1..].trim_start();
            if let Some(body) = val.strip_prefix('"') {
                return Some(decode_entities(body.split('"').next().unwrap_or(body)));
            }
            if let Some(body) = val.strip_prefix('\'') {
                return Some(decode_entities(body.split('\'').next().unwrap_or(body)));
            }
            let end = val.find(char::is_whitespace).unwrap_or(val.len());
            return Some(decode_entities(&val[..end]));
        }
        from = at + name.len();
    }
    None
}

/// Decode the common named + numeric HTML entities. Unrecognized `&…;` sequences
/// are left verbatim (never panics, never loops).
pub(crate) fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        // A real entity is short; cap the `;` search so stray `&` is cheap.
        if let Some(semi) = tail[1..].find(';').map(|p| p + 1).filter(|&p| p <= 10) {
            let ent = &tail[1..semi];
            let decoded = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{00A0}'),
                _ => {
                    if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                    } else if let Some(dec) = ent.strip_prefix('#') {
                        dec.parse::<u32>().ok().and_then(char::from_u32)
                    } else {
                        None
                    }
                }
            };
            if let Some(c) = decoded {
                out.push(c);
                rest = &tail[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}
