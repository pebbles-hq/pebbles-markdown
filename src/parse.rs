//! Parsing — Markdown source → the block/inline [`model`](crate::model), plus
//! HTML export ([`to_html`]). Wraps pulldown-cmark with this crate's option set
//! and the in-tree extensions: front matter, `:::` containers, `#tags`, `%%
//! comments %%`, bare-URL/email autolinks, emoji, and heading slugs.

use std::rc::Rc;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::*;

/// Content-keyed parse cache for FIXED sources (8-entry MRU): re-rendering a
/// static document re-uses the parsed blocks; a different string parses fresh.
pub(crate) fn parse_cached(s: &str) -> Rc<Vec<Block>> {
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

/// A nested block container in the main parse: the document, a quote body, or a
/// list item (carrying the index of the list it belongs to, for loose-detection).
pub(crate) enum MdFrame {
    Doc,
    Quote,
    Item(usize),
    /// A footnote definition body (collected to the end, not rendered inline).
    Footnote,
    /// A definition-list definition body.
    Def,
    /// A `<details>` body spanning multiple markdown blocks.
    DetailsMain,
}

/// Map a `:::type` container name to a GFM alert kind.
pub(crate) fn container_alert(ty: &str) -> &'static str {
    match ty
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "tip" | "hint" => "TIP",
        "important" => "IMPORTANT",
        "warning" | "attention" => "WARNING",
        "caution" | "danger" | "error" => "CAUTION",
        _ => "NOTE",
    }
}

/// Expand `:::type … :::` fenced containers into GFM callout blockquotes so they
/// render as admonitions. Content inside ``` / ~~~ code fences is left untouched;
/// one nesting level (nested `:::` fall through as body text).
pub(crate) fn expand_containers(src: &str) -> String {
    if !src.contains(":::") {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len() + 16);
    let mut code_fence: Option<char> = None;
    let mut in_container = false;
    for line in src.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let t = content.trim_start();
        let fence_char = t.chars().next().filter(|c| matches!(c, '`' | '~'));
        let is_code_fence =
            fence_char.is_some() && t.chars().take_while(|c| Some(*c) == fence_char).count() >= 3;
        // Track code fences — never touch `:::` inside code.
        if let Some(fc) = code_fence {
            if is_code_fence && fence_char == Some(fc) {
                code_fence = None;
            }
            out.push_str(line);
            continue;
        }
        if is_code_fence {
            code_fence = fence_char;
            out.push_str(line);
            continue;
        }
        if !in_container {
            if let Some(rest) = t.strip_prefix(":::")
                && !rest.trim().is_empty()
            {
                out.push_str("> [!");
                out.push_str(container_alert(rest.trim()));
                out.push_str("]\n");
                in_container = true;
                continue;
            }
            out.push_str(line);
            continue;
        }
        // Inside a container: a bare `:::` closes it; other lines become quote body.
        if t == ":::" {
            in_container = false;
            out.push('\n');
            continue;
        }
        out.push_str("> ");
        out.push_str(line);
    }
    out
}

/// The pulldown-cmark option set this crate parses (and exports) with.
pub(crate) fn md_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_GFM); // blockquote alert kinds (callouts)
    opts.insert(Options::ENABLE_MATH); // `$…$` / `$$…$$`
    opts.insert(Options::ENABLE_SUBSCRIPT); // `~x~`
    opts.insert(Options::ENABLE_SUPERSCRIPT); // `^x^`
    opts.insert(Options::ENABLE_DEFINITION_LIST);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION); // curly quotes, en/em-dash, …
    opts.insert(Options::ENABLE_WIKILINKS); // [[Page]], [[Page|alias]], ![[embed]]
    opts
}

/// Render Markdown `source` to an HTML string (CommonMark + the enabled GFM /
/// extension set), for export / interop. Front matter is stripped and emoji
/// shortcodes + `%% comments %%` are resolved first; the in-tree-only niceties
/// (`#tags`, TeX→Unicode math) are left as their source text.
pub fn to_html(source: &str) -> String {
    let (_front, body) = split_front_matter(source);
    let pre = strip_comments(&crate::advanced::replace_emoji(&expand_containers(body)));
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, Parser::new_ext(&pre, md_options()));
    out
}

/// Parse GFM into the block model (tables + strikethrough + task lists on),
/// after stripping any `---`/`+++` front matter into a leading properties block.
pub(crate) fn parse_blocks(src: &str) -> Vec<Block> {
    let (front, body) = split_front_matter(src);
    let body = expand_containers(body);
    let opts = md_options();

    // Frames of nested block containers (document / quote bodies / list items).
    let mut stack: Vec<Vec<Block>> = vec![Vec::new()];
    // What each `stack` frame is — parallel to `stack`, for loose-list detection.
    let mut frames: Vec<MdFrame> = vec![MdFrame::Doc];
    // In-flight list frames: (ordered, start, loose, finished items).
    let mut lists: Vec<(bool, u64, bool, Vec<ListItem>)> = Vec::new();
    // The pending task marker for the item currently open.
    let mut item_task: Vec<Option<(bool, usize)>> = Vec::new();
    let mut task_ordinal = 0usize;

    // Inline state (`code`/`sub`/`sup` are depths so nested tags close cleanly).
    let mut inline: Vec<Inline> = Vec::new();
    let (mut bold, mut italic, mut strike, mut code_depth) = (0u32, 0u32, 0u32, 0u32);
    let (mut sub_depth, mut sup_depth) = (0u32, 0u32);
    let mut link: Vec<String> = Vec::new();
    let mut heading: Option<u8> = None;
    // Code blocks.
    let mut code: Option<(String, String)> = None;
    // Image capture: (url, alt, is-wikilink-embed) — alt accumulates Start→End.
    let mut image: Option<(String, String, bool)> = None;
    // Tables.
    let mut table: Option<TableAcc> = None;
    let mut table_aligns: Vec<CellAlign> = Vec::new();
    let mut table_row: Vec<Vec<Inline>> = Vec::new();
    let mut in_head = false;
    // Accumulator for a run of block-level HTML (flushed when non-HTML arrives).
    let mut html_buf = String::new();
    // Footnotes: definitions collected to the end, numbered by first reference.
    let mut footnotes: Vec<(String, Vec<Block>)> = Vec::new();
    let mut footnote_labels: Vec<String> = Vec::new();
    let mut footnote_capture: Vec<String> = Vec::new();
    // Blockquote kinds (Some = callout), pushed per open quote.
    let mut quote_kind: Vec<Option<CalloutKind>> = Vec::new();
    // Definition-list accumulators (nesting-safe) + used heading slugs (for dedup).
    let mut deflists: Vec<Vec<(Vec<Inline>, Vec<Block>)>> = Vec::new();
    let mut used_slugs: Vec<String> = Vec::new();
    // Open `<details>` frames spanning multiple blocks: (open, summary inlines).
    let mut details_main: Vec<(bool, Vec<Inline>)> = Vec::new();

    let push_run = |inline: &mut Vec<Inline>, text: String, f: Flags, link: &[String]| {
        if text.is_empty() {
            return;
        }
        inline.push(Inline::Run(run_of(text, f, link.last().cloned())));
    };

    for ev in Parser::new_ext(&body, opts) {
        // A block-HTML run ends as soon as any non-HTML event arrives.
        if !matches!(ev, Event::Html(_)) {
            flush_html_run(&mut html_buf, &mut stack, &mut frames, &mut details_main);
        }
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    // A paragraph as the DIRECT child of a list item marks that
                    // list loose (blank-line-separated) — tight items emit bare text.
                    if let Some(MdFrame::Item(li)) = frames.last()
                        && let Some(l) = lists.get_mut(*li)
                    {
                        l.2 = true;
                    }
                }
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
                Tag::BlockQuote(kind) => {
                    stack.push(Vec::new());
                    frames.push(MdFrame::Quote);
                    quote_kind.push(kind.map(|k| match k {
                        pulldown_cmark::BlockQuoteKind::Note => CalloutKind::Note,
                        pulldown_cmark::BlockQuoteKind::Tip => CalloutKind::Tip,
                        pulldown_cmark::BlockQuoteKind::Important => CalloutKind::Important,
                        pulldown_cmark::BlockQuoteKind::Warning => CalloutKind::Warning,
                        pulldown_cmark::BlockQuoteKind::Caution => CalloutKind::Caution,
                    }));
                }
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
                    lists.push((start.is_some(), start.unwrap_or(1), false, Vec::new()));
                }
                Tag::Item => {
                    stack.push(Vec::new());
                    frames.push(MdFrame::Item(lists.len().saturating_sub(1)));
                    item_task.push(None);
                }
                Tag::Emphasis => italic += 1,
                Tag::Strong => bold += 1,
                Tag::Strikethrough => strike += 1,
                Tag::Subscript => sub_depth += 1,
                Tag::Superscript => sup_depth += 1,
                Tag::DefinitionList => deflists.push(Vec::new()),
                Tag::DefinitionListTitle => inline.clear(),
                Tag::DefinitionListDefinition => {
                    stack.push(Vec::new());
                    frames.push(MdFrame::Def);
                }
                Tag::Link { dest_url, .. } => link.push(dest_url.to_string()),
                Tag::Image {
                    dest_url,
                    link_type,
                    ..
                } => {
                    let wiki = matches!(link_type, pulldown_cmark::LinkType::WikiLink { .. });
                    image = Some((dest_url.to_string(), String::new(), wiki));
                }
                Tag::Table(aligns) => {
                    table = Some((Vec::new(), Vec::new()));
                    table_aligns = aligns
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Center => CellAlign::Center,
                            pulldown_cmark::Alignment::Right => CellAlign::End,
                            _ => CellAlign::Start,
                        })
                        .collect();
                }
                Tag::FootnoteDefinition(label) => {
                    stack.push(Vec::new());
                    frames.push(MdFrame::Footnote);
                    footnote_capture.push(label.to_string());
                }
                Tag::TableHead => in_head = true,
                Tag::TableRow => table_row = Vec::new(),
                Tag::TableCell => inline.clear(),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
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
                    let slug = unique_slug(
                        &crate::advanced::slugify(&inline_plain(&content)),
                        &mut used_slugs,
                    );
                    if let Some(top) = stack.last_mut() {
                        top.push(Block::Heading(lvl, content, slug));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    let body = stack.pop().unwrap_or_default();
                    frames.pop();
                    let kind = quote_kind.pop().flatten();
                    if let Some(top) = stack.last_mut() {
                        top.push(match kind {
                            Some(kind) => Block::Callout { kind, body },
                            None => Block::Quote(body),
                        });
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
                    frames.pop();
                    let task = item_task.pop().flatten();
                    if let Some((_, _, _, items)) = lists.last_mut() {
                        items.push(ListItem { task, blocks });
                    }
                }
                TagEnd::List(_) => {
                    if let Some((ordered, start, loose, items)) = lists.pop()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::List {
                            ordered,
                            start,
                            items,
                            tight: !loose,
                        });
                    }
                }
                TagEnd::Emphasis => italic = italic.saturating_sub(1),
                TagEnd::Strong => bold = bold.saturating_sub(1),
                TagEnd::Strikethrough => strike = strike.saturating_sub(1),
                TagEnd::Subscript => sub_depth = sub_depth.saturating_sub(1),
                TagEnd::Superscript => sup_depth = sup_depth.saturating_sub(1),
                TagEnd::DefinitionListTitle => {
                    let title = std::mem::take(&mut inline);
                    if let Some(dl) = deflists.last_mut() {
                        dl.push((title, Vec::new()));
                    }
                }
                TagEnd::DefinitionListDefinition => {
                    let content = std::mem::take(&mut inline);
                    if !content.is_empty()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::Para(content));
                    }
                    let mut blocks = stack.pop().unwrap_or_default();
                    frames.pop();
                    if let Some(dl) = deflists.last_mut()
                        && let Some(entry) = dl.last_mut()
                    {
                        entry.1.append(&mut blocks);
                    }
                }
                TagEnd::DefinitionList => {
                    if let Some(entries) = deflists.pop()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::DefList(entries));
                    }
                }
                TagEnd::Link => {
                    link.pop();
                }
                TagEnd::Image => {
                    if let Some((url, alt, embed)) = image.take() {
                        inline.push(Inline::Image { url, alt, embed });
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
                        top.push(Block::Table {
                            header,
                            aligns: std::mem::take(&mut table_aligns),
                            rows,
                        });
                    }
                }
                TagEnd::FootnoteDefinition => {
                    // Flush the definition's trailing inline (tight text), then
                    // stash its body keyed by label — it renders at the very end.
                    let content = std::mem::take(&mut inline);
                    if !content.is_empty()
                        && let Some(top) = stack.last_mut()
                    {
                        top.push(Block::Para(content));
                    }
                    let blocks = stack.pop().unwrap_or_default();
                    frames.pop();
                    let label = footnote_capture.pop().unwrap_or_default();
                    footnotes.push((label, blocks));
                }
                _ => {}
            },
            Event::Text(t) => {
                let flags =
                    Flags::from_counts(bold, italic, strike, code_depth, sub_depth, sup_depth);
                if let Some((_, buf)) = code.as_mut() {
                    buf.push_str(&t);
                } else if let Some((_, alt, _)) = image.as_mut() {
                    alt.push_str(&t);
                } else if link.is_empty() && code_depth == 0 {
                    // Plain body text: emoji shortcodes, drop `%% comments %%`, then
                    // `==highlight==`, `#tag`s, and bare-URL/email autolinks — each
                    // emitted as its own run.
                    let replaced = strip_comments(&crate::advanced::replace_emoji(&t));
                    emit_text_runs(&replaced, flags, &mut inline);
                } else {
                    // Inside an explicit link or inline `<code>`: keep verbatim.
                    push_run(&mut inline, t.to_string(), flags, &link);
                }
            }
            Event::Code(t) => {
                let mut flags =
                    Flags::from_counts(bold, italic, strike, code_depth, sub_depth, sup_depth);
                flags.code = true;
                push_run(&mut inline, t.to_string(), flags, &link);
            }
            Event::SoftBreak | Event::HardBreak => {
                let flags =
                    Flags::from_counts(bold, italic, strike, code_depth, sub_depth, sup_depth);
                push_run(&mut inline, " ".to_string(), flags, &link);
            }
            Event::InlineMath(tex) => {
                inline.push(Inline::Math {
                    tex: tex.to_string(),
                    display: false,
                });
            }
            Event::DisplayMath(tex) => {
                inline.push(Inline::Math {
                    tex: tex.to_string(),
                    display: true,
                });
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
            Event::FootnoteReference(label) => {
                let n = footnote_number(&mut footnote_labels, &label);
                inline.push(Inline::Footnote(n));
            }
            // Block-level HTML accumulates; it is parsed to native blocks at the
            // next non-HTML event (or end of document).
            Event::Html(t) => html_buf.push_str(&t),
            // Inline HTML tags adjust the inline formatting state in place.
            Event::InlineHtml(t) => match crate::html::classify_inline_tag(&t) {
                crate::html::InlineTag::Bold(open) => bold = step(bold, open),
                crate::html::InlineTag::Italic(open) => italic = step(italic, open),
                crate::html::InlineTag::Strike(open) => strike = step(strike, open),
                crate::html::InlineTag::Code(open) => code_depth = step(code_depth, open),
                crate::html::InlineTag::LinkOpen(url) => link.push(url),
                crate::html::InlineTag::LinkClose => {
                    link.pop();
                }
                crate::html::InlineTag::Break => {
                    let flags =
                        Flags::from_counts(bold, italic, strike, code_depth, sub_depth, sup_depth);
                    push_run(&mut inline, "\n".to_string(), flags, &link);
                }
                crate::html::InlineTag::Image { url, alt } => inline.push(Inline::Image {
                    url,
                    alt,
                    embed: false,
                }),
                crate::html::InlineTag::Ignore => {}
            },
        }
        let _ = in_head;
    }
    // Flush a trailing block-HTML run.
    flush_html_run(&mut html_buf, &mut stack, &mut frames, &mut details_main);
    // Close any `<details>` left open at end of document.
    while matches!(frames.last(), Some(MdFrame::DetailsMain)) {
        let body = stack.pop().unwrap_or_default();
        frames.pop();
        let (open, summary) = details_main.pop().unwrap_or((false, Vec::new()));
        if let Some(top) = stack.last_mut() {
            top.push(Block::Details {
                summary,
                open,
                body,
            });
        }
    }

    let mut out = stack.pop().unwrap_or_default();
    // Footnotes render last: referenced definitions first (in reference order),
    // then any defined-but-unreferenced ones.
    if !footnotes.is_empty() {
        let mut entries: Vec<(usize, Vec<Block>)> = Vec::new();
        for (i, label) in footnote_labels.iter().enumerate() {
            if let Some(pos) = footnotes.iter().position(|(l, _)| l == label) {
                entries.push((i + 1, footnotes[pos].1.clone()));
            }
        }
        let mut next = footnote_labels.len() + 1;
        for (label, blocks) in &footnotes {
            if !footnote_labels.iter().any(|l| l == label) {
                entries.push((next, blocks.clone()));
                next += 1;
            }
        }
        out.push(Block::Footnotes(entries));
    }
    // Replace a `[toc]` / `[[toc]]` marker paragraph with a generated ToC.
    apply_toc(&mut out);
    if !front.is_empty() {
        out.insert(0, Block::FrontMatter(front));
    }
    out
}

/// Flush an accumulated block-HTML run. `<details>`/`</details>` open/close a
/// frame in the MAIN block stack so markdown between them (blank-line separated)
/// nests inside the disclosure; everything else parses via the HTML subset parser.
pub(crate) fn flush_html_run(
    buf: &mut String,
    stack: &mut Vec<Vec<Block>>,
    frames: &mut Vec<MdFrame>,
    details_main: &mut Vec<(bool, Vec<Inline>)>,
) {
    if buf.is_empty() {
        return;
    }
    let frag = std::mem::take(buf);
    let trimmed = frag.trim();
    let lower = trimmed.to_ascii_lowercase();

    // A standalone `</details>` closes the nearest open details frame.
    if lower == "</details>" {
        if matches!(frames.last(), Some(MdFrame::DetailsMain)) {
            let body = stack.pop().unwrap_or_default();
            frames.pop();
            let (open, summary) = details_main.pop().unwrap_or((false, Vec::new()));
            if let Some(top) = stack.last_mut() {
                top.push(Block::Details {
                    summary,
                    open,
                    body,
                });
            }
        }
        return;
    }
    // A run that opens `<details>` but doesn't close it in the same run: open a
    // main-stack frame, reusing the HTML parser to lift the inline `<summary>`.
    if lower.starts_with("<details") && !lower.contains("</details>") {
        let (open, summary) = match crate::html::parse_html_fragment(&frag).into_iter().next() {
            Some(Block::Details { open, summary, .. }) => (open, summary),
            _ => (false, Vec::new()),
        };
        details_main.push((open, summary));
        stack.push(Vec::new());
        frames.push(MdFrame::DetailsMain);
        return;
    }
    // Everything else: native HTML blocks (self-contained `<details>` included).
    for blk in crate::html::parse_html_fragment(&frag) {
        if let Some(top) = stack.last_mut() {
            top.push(blk);
        }
    }
}

/// Return the 1-based number for `label`, assigning the next one on first sight
/// (footnotes are numbered by first-reference order).
pub(crate) fn footnote_number(labels: &mut Vec<String>, label: &str) -> usize {
    if let Some(pos) = labels.iter().position(|l| l == label) {
        pos + 1
    } else {
        labels.push(label.to_string());
        labels.len()
    }
}

/// The plain text of a block (for a11y labels + the block-renderer hook).
pub(crate) fn block_plain(b: &Block) -> String {
    match b {
        Block::Para(inls) | Block::Heading(_, inls, _) => inline_plain(inls),
        Block::Code { text, .. } => text.clone(),
        Block::Quote(body) | Block::Callout { body, .. } | Block::Details { body, .. } => {
            body.first().map(block_plain).unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// A stable kind tag for a block (for the block-renderer hook).
pub(crate) fn block_kind(b: &Block) -> &'static str {
    match b {
        Block::FrontMatter(_) => "frontmatter",
        Block::Heading(..) => "heading",
        Block::Para(_) => "paragraph",
        Block::Code { .. } => "code",
        Block::Quote(_) => "quote",
        Block::Callout { .. } => "callout",
        Block::DefList(_) => "deflist",
        Block::Details { .. } => "details",
        Block::Toc(_) => "toc",
        Block::List { .. } => "list",
        Block::Rule => "rule",
        Block::Table { .. } => "table",
        Block::Footnotes(_) => "footnotes",
        Block::Backlinks(_) => "backlinks",
    }
}

/// The concatenated plain text of a run of inlines (for slugs / ToC titles).
pub(crate) fn inline_plain(inls: &[Inline]) -> String {
    inls.iter()
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r.text.as_str()),
            _ => None,
        })
        .collect()
}

/// Make `base` unique against `used` by suffixing `-2`, `-3`, … (GitHub-style).
pub(crate) fn unique_slug(base: &str, used: &mut Vec<String>) -> String {
    let base = if base.is_empty() { "section" } else { base };
    let mut candidate = base.to_string();
    let mut n = 1;
    while used.contains(&candidate) {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    used.push(candidate.clone());
    candidate
}

/// Emit plain body text as runs: `==highlight==` spans become marked runs; the
/// rest goes through the `#tag` + autolink scanner. (Emoji/comments already handled.)
pub(crate) fn emit_text_runs(text: &str, flags: Flags, out: &mut Vec<Inline>) {
    split_highlight(text, |seg, is_mark| {
        if seg.is_empty() {
            return;
        }
        if is_mark {
            out.push(Inline::Run(run_of(
                seg.to_string(),
                Flags {
                    mark: true,
                    ..flags
                },
                None,
            )));
        } else {
            emit_tags_and_links(seg, flags, out);
        }
    });
}

/// Scan a plain segment for Obsidian `#tag`s (emitted as tag runs) and, between
/// them, bare-URL/email autolinks.
pub(crate) fn emit_tags_and_links(text: &str, flags: Flags, out: &mut Vec<Inline>) {
    let b = text.as_bytes();
    let n = text.len();
    let mut i = 0;
    let mut seg = 0;
    while i < n {
        if b[i] == b'#'
            && (i == 0 || is_tag_boundary(b[i - 1]))
            && let Some(end) = scan_tag(text, i)
        {
            if i > seg {
                autolink_text(&text[seg..i], |s, href| {
                    if !s.is_empty() {
                        out.push(Inline::Run(run_of(s.to_string(), flags, href)));
                    }
                });
            }
            let name = &text[i..end];
            out.push(Inline::Run(run_of(
                name.to_string(),
                Flags { tag: true, ..flags },
                Some(name.to_string()),
            )));
            i = end;
            seg = end;
            continue;
        }
        i += char_len_at(text, i);
    }
    if seg < n {
        autolink_text(&text[seg..], |s, href| {
            if !s.is_empty() {
                out.push(Inline::Run(run_of(s.to_string(), flags, href)));
            }
        });
    }
}

/// A `#tag` may start after whitespace or common punctuation (not mid-word).
pub(crate) fn is_tag_boundary(prev: u8) -> bool {
    prev.is_ascii_whitespace() || matches!(prev, b'(' | b'[' | b'{' | b'>' | b'"' | b'\'' | b',')
}

/// Scan a `#tag` starting at `start` (the `#`). Returns the end byte if it is a
/// valid Obsidian tag: `[A-Za-z0-9_/-]+` with at least one non-digit (so `#123`
/// is not a tag). `None` otherwise.
pub(crate) fn scan_tag(text: &str, start: usize) -> Option<usize> {
    let b = text.as_bytes();
    let n = text.len();
    let mut i = start + 1;
    let mut has_alpha = false;
    while i < n {
        let c = b[i];
        if c.is_ascii_alphabetic() || matches!(c, b'_' | b'/' | b'-') {
            has_alpha = true;
            i += 1;
        } else if c.is_ascii_digit() {
            i += 1;
        } else {
            break;
        }
    }
    // Must have at least one char after `#`, include a letter, and not end on `/`/`-`.
    if i > start + 1 && has_alpha {
        let trimmed = text[..i].trim_end_matches(['/', '-']);
        let end = trimmed.len();
        (end > start + 1).then_some(end)
    } else {
        None
    }
}

/// Remove Obsidian `%% … %%` comments from a text run. An unterminated `%%` is
/// left literal so nothing is silently eaten.
pub(crate) fn strip_comments(s: &str) -> String {
    if !s.contains("%%") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find("%%") {
        out.push_str(&rest[..open]);
        match rest[open + 2..].find("%%") {
            Some(close) => rest = &rest[open + 2 + close + 2..],
            None => {
                out.push_str(&rest[open..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether a link URL is safe to make clickable. Scheme-less links (relative
/// paths, `#fragment`, wikilink targets) are safe; among schemed links only a
/// short allowlist is (so `javascript:` / `data:` / `vbscript:` / `file:` are
/// rendered as inert text).
pub(crate) fn is_safe_link(url: &str) -> bool {
    let u = url.trim();
    match u.find(':') {
        None => true,
        Some(colon) => {
            let before = &u[..colon];
            // A `/`, `#` or `?` before the colon means it's a path, not a scheme.
            if before.is_empty()
                || before.contains(['/', '#', '?'])
                || !before
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
            {
                return true;
            }
            matches!(
                before.to_ascii_lowercase().as_str(),
                "http" | "https" | "mailto" | "tel" | "ftp" | "ftps" | "sms"
            )
        }
    }
}

/// Whether a URL/path looks like an image (for `![[…]]` embeds).
pub(crate) fn is_image_path(url: &str) -> bool {
    let u = url
        .split(['#', '?'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif",
    ]
    .iter()
    .any(|ext| u.ends_with(ext))
}

/// Split `text` into `(segment, is_match)` runs against a lowercased `query`
/// (case-insensitive, all occurrences). Defensive: if lowercasing changes the
/// byte length or a match wouldn't land on char boundaries, the whole text is
/// returned unmatched — never panics or splits a char.
pub(crate) fn split_matches(text: &str, query: &str) -> Vec<(String, bool)> {
    if query.is_empty() {
        return vec![(text.to_string(), false)];
    }
    let hay = text.to_lowercase();
    if hay.len() != text.len() || !hay.contains(query) {
        return vec![(text.to_string(), false)];
    }
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        match hay[i..].find(query) {
            Some(rel) => {
                let start = i + rel;
                let end = start + query.len();
                if !(text.is_char_boundary(start) && text.is_char_boundary(end)) {
                    out.push((text[i..].to_string(), false));
                    return out;
                }
                if start > i {
                    out.push((text[i..start].to_string(), false));
                }
                out.push((text[start..end].to_string(), true));
                i = end;
            }
            None => {
                out.push((text[i..].to_string(), false));
                break;
            }
        }
    }
    if out.is_empty() {
        out.push((text.to_string(), false));
    }
    out
}

/// Parse an image's alt text into `(caption, width, height)`. Supports a size
/// suffix — `200`, `200x100`, or `caption|200` / `caption|200x100` (Obsidian
/// style). A pure size yields no caption; plain text is the caption.
pub(crate) fn parse_image_alt(alt: &str) -> (Option<String>, Option<f64>, Option<f64>) {
    fn size_of(s: &str) -> Option<(f64, Option<f64>)> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Some((w, h)) = s.split_once(['x', 'X', '×']) {
            Some((w.trim().parse().ok()?, Some(h.trim().parse().ok()?)))
        } else {
            Some((s.parse().ok()?, None))
        }
    }
    if let Some((left, right)) = alt.rsplit_once('|')
        && let Some((w, h)) = size_of(right)
    {
        let cap = left.trim();
        return ((!cap.is_empty()).then(|| cap.to_string()), Some(w), h);
    }
    if let Some((w, h)) = size_of(alt) {
        return (None, Some(w), h);
    }
    let cap = alt.trim();
    ((!cap.is_empty()).then(|| cap.to_string()), None, None)
}

/// Split text into `==highlight==` and normal segments, calling `emit(text, is_mark)`.
pub(crate) fn split_highlight(text: &str, mut emit: impl FnMut(&str, bool)) {
    let b = text.as_bytes();
    let n = text.len();
    let mut i = 0;
    let mut seg = 0;
    while i + 1 < n {
        if b[i] == b'='
            && b[i + 1] == b'='
            && let Some(close) = find_double_eq(text, i + 2)
        {
            if i > seg {
                emit(&text[seg..i], false);
            }
            emit(&text[i + 2..close], true);
            i = close + 2;
            seg = i;
            continue;
        }
        i += 1;
    }
    if seg < n {
        emit(&text[seg..], false);
    }
}

/// Index of the next non-empty `==` after `from`, or `None`.
pub(crate) fn find_double_eq(text: &str, from: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = from;
    while i + 1 < text.len() {
        if b[i] == b'=' && b[i + 1] == b'=' {
            return (i > from).then_some(i);
        }
        i += 1;
    }
    None
}

/// Replace a `[toc]` / `[[toc]]` marker paragraph with a table of contents built
/// from every heading in `blocks`. No-op when there is no marker.
pub(crate) fn apply_toc(blocks: &mut [Block]) {
    fn is_marker(b: &Block) -> bool {
        matches!(b, Block::Para(inls) if {
            let t = inline_plain(inls).trim().to_lowercase();
            t == "[toc]" || t == "[[toc]]"
        })
    }
    if !blocks.iter().any(is_marker) {
        return;
    }
    let entries: Vec<(u8, String, String)> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading(l, inls, slug) => {
                Some((*l, inline_plain(inls).trim().to_string(), slug.clone()))
            }
            _ => None,
        })
        .collect();
    for b in blocks.iter_mut() {
        if is_marker(b) {
            *b = Block::Toc(entries.clone());
        }
    }
}

/// `+1` on an opening tag, saturating `-1` on a close.
pub(crate) fn step(counter: u32, open: bool) -> u32 {
    if open {
        counter + 1
    } else {
        counter.saturating_sub(1)
    }
}

// ---------------------------------------------------------------------------
// GFM extended autolinks — bare URLs / emails inside plain text
// ---------------------------------------------------------------------------

/// Split plain text, promoting bare `http(s)://…`, `www.…`, and `foo@bar.tld`
/// (GFM extended autolinks) into link pieces. `emit(text, href)` receives each
/// piece in order (`href` is `Some(_)` for a link). The cursor advances by whole
/// characters and always moves forward, so it terminates on any input.
pub(crate) fn autolink_text(text: &str, mut emit: impl FnMut(&str, Option<String>)) {
    let bytes = text.as_bytes();
    let n = text.len();
    let mut i = 0;
    let mut seg = 0;
    while i < n {
        // A candidate may only start at a boundary (doc start or after a
        // non-alphanumeric), so `ahttps://x` inside a word is not a link.
        let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if boundary && let Some((href, end)) = match_autolink(text, i) {
            if i > seg {
                emit(&text[seg..i], None);
            }
            emit(&text[i..end], Some(href));
            i = end;
            seg = end;
            continue;
        }
        i += char_len_at(text, i);
    }
    if seg < n {
        emit(&text[seg..], None);
    }
}

/// Case-insensitive ASCII prefix test.
pub(crate) fn starts_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Trim trailing punctuation GFM excludes from an autolink (`. , ; : ! ? * _ ~ ' "`)
/// and an unbalanced closing `)`. Only ASCII bytes are trimmed, so every returned
/// slice stays on a char boundary.
pub(crate) fn trim_url_end(url: &str) -> &str {
    let mut end = url.len();
    while end > 0 {
        let b = url.as_bytes();
        match b[end - 1] {
            b'?' | b'!' | b'.' | b',' | b':' | b';' | b'*' | b'_' | b'~' | b'\'' | b'"' => end -= 1,
            b')' => {
                let opens = url[..end].bytes().filter(|&c| c == b'(').count();
                let closes = url[..end].bytes().filter(|&c| c == b')').count();
                if closes > opens {
                    end -= 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    &url[..end]
}

/// Match a bare URL or email starting at `start`; returns `(href, end_byte)`.
pub(crate) fn match_autolink(s: &str, start: usize) -> Option<(String, usize)> {
    let rest = &s[start..];
    let (plen, www) = if starts_ci(rest, "https://") {
        (8, false)
    } else if starts_ci(rest, "http://") {
        (7, false)
    } else if starts_ci(rest, "www.") {
        (4, true)
    } else {
        return match_email(s, start);
    };
    // Consume the URL body up to whitespace or `<`.
    let mut j = plen;
    while j < rest.len() {
        match rest.as_bytes()[j] {
            b' ' | b'\t' | b'\n' | b'\r' | b'<' => break,
            _ => j += char_len_at(rest, j),
        }
    }
    let trimmed = trim_url_end(&rest[..j]);
    // Require a host (something before the first `/`) that contains a dot.
    let host = &trimmed[plen..];
    let host_end = host.find('/').unwrap_or(host.len());
    if host[..host_end].is_empty() || !host[..host_end].contains('.') {
        return None;
    }
    let href = if www {
        format!("http://{trimmed}")
    } else {
        trimmed.to_string()
    };
    Some((href, start + trimmed.len()))
}

/// Match a bare email address (`local@domain.tld`) at `start`.
pub(crate) fn match_email(s: &str, start: usize) -> Option<(String, usize)> {
    let b = s.as_bytes();
    let n = s.len();
    let mut i = start;
    while i < n
        && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'.' | b'_' | b'%' | b'+' | b'-'))
    {
        i += 1;
    }
    if i == start || i >= n || b[i] != b'@' {
        return None;
    }
    i += 1;
    let dom_start = i;
    while i < n && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'.' | b'-' | b'_')) {
        i += 1;
    }
    // Trim trailing separators, then require a dot in the remaining domain.
    let mut end = i;
    while end > dom_start && matches!(b[end - 1], b'.' | b'-' | b'_') {
        end -= 1;
    }
    if end <= dom_start || !s[dom_start..end].contains('.') {
        return None;
    }
    Some((format!("mailto:{}", &s[start..end]), end))
}

// ---------------------------------------------------------------------------
// Front matter (`---` YAML / `+++` TOML)
// ---------------------------------------------------------------------------

/// Split a leading `---`/`+++` front-matter block off the source, returning its
/// parsed `(key, value)` pairs and the Markdown body after it. Requires the fence
/// on its own first line AND a matching closing fence — otherwise the source is
/// returned untouched (so a lone leading `---` is still a thematic break).
pub(crate) fn split_front_matter(src: &str) -> (Vec<(String, String)>, &str) {
    let toml = if src.starts_with("---") {
        false
    } else if src.starts_with("+++") {
        true
    } else {
        return (Vec::new(), src);
    };
    let fence = if toml { "+++" } else { "---" };
    let first_nl = match src.find('\n') {
        Some(i) => i,
        None => return (Vec::new(), src),
    };
    if src[..first_nl].trim_end() != fence {
        return (Vec::new(), src);
    }
    let rest = &src[first_nl + 1..];
    // Find the closing fence on its own line.
    let mut off = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == fence {
            let props = parse_front_matter(&rest[..off], toml);
            // Only claim it as front matter if it actually parsed as a key/value
            // map — otherwise `---\nSome text\n---` (a setext H2) would be eaten.
            if props.is_empty() {
                return (Vec::new(), src);
            }
            return (props, &rest[off + line.len()..]);
        }
        off += line.len();
    }
    (Vec::new(), src)
}

/// Parse simple `key: value` (YAML) / `key = value` (TOML) lines into pairs.
/// Complex/nested values are kept as their raw right-hand-side text; comment and
/// blank lines are skipped. Never panics.
pub(crate) fn parse_front_matter(body: &str, toml: bool) -> Vec<(String, String)> {
    let sep = if toml { '=' } else { ':' };
    let mut out = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((k, v)) = trimmed.split_once(sep) else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
        out.push((key.to_string(), val));
    }
    out
}

// ---------------------------------------------------------------------------
