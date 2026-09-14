//! Block & inline rendering — turns the parsed [`model`](crate::model) into the
//! themed Pebbles widget tree: code blocks, callouts, `<details>`, tables,
//! images, embeds, footnotes, and the inline flow (emphasis, links, tags, math).
//! Together with [`widget`](crate::widget) this is where reader widgets are built.

use pebbles_foundation::{Color, CrossAxisAlignment, EdgeInsets, MainAxisSize, TextAlign, palette};
#[cfg(feature = "image-view")]
use pebbles_render::ImageFit;
use pebbles_render::{Border, BorderRadius, BoxDecoration, Cursor, lucide};

use pebbles_core::widget::{AnyWidget, IntoWidget};
use pebbles_core::{children, component_props};
use pebbles_widgets::theme::{mix, theme};
use pebbles_widgets::{
    SemanticsRole, SingleChildScrollView, TableColumnWidth, TextSpan, column, container, expanded,
    gap_w, gesture_detector, layout_table, padding, row, semantics, span, text, text_rich, wrap,
};

use crate::*;

/// Render a fenced code block: a header (language label + copy button) above the
/// highlighted code, with optional line numbers and highlighted lines.
pub(crate) fn render_code(info: &str, code: &str, cx: &Cx) -> AnyWidget {
    let s = &cx.style;
    let (lang, hl) = parse_fence_info(info);
    // A registered handler may render the block itself (e.g. a diagram).
    if !lang.is_empty()
        && let Some(handler) = &cx.fenced
        && let Some(widget) = handler(&lang, code)
    {
        return widget;
    }
    let size = s.body_size * 0.92;
    let lines = highlight(code.trim_end(), &lang, &s.syntax, s.code_color);

    let body: AnyWidget = if s.code_line_numbers || !hl.is_empty() {
        render_code_lines(&lines, s, &hl, s.code_line_numbers, size)
    } else {
        // Fast path: ONE rich paragraph — indentation is genuine whitespace in a
        // single shaped layout, no widget-per-line.
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
        text_rich(spans)
            .size(size)
            .color(s.code_color)
            .font_family(s.code_family.clone())
            .soft_wrap(false)
            .into_widget()
    };

    container()
        .decoration(
            BoxDecoration::new()
                .color(s.code_bg)
                .radius(BorderRadius::all(6.0)),
        )
        .padding(EdgeInsets::all(10.0))
        .child(
            column(children![code_header(&lang, code, cx), body])
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min)
                .spacing(6.0),
        )
        .into_widget()
}

/// The header row of a code block: language label (if any) on the left, a
/// copy-to-clipboard button on the right.
pub(crate) fn code_header(lang: &str, code: &str, cx: &Cx) -> AnyWidget {
    let s = &cx.style;
    let mut cells: Vec<AnyWidget> = Vec::new();
    if !lang.is_empty() {
        cells.push(
            text(lang.to_string())
                .size(s.body_size * 0.78)
                .weight(600.0)
                .color(mix(s.code_color, s.code_bg, 0.45))
                .font_family(s.code_family.clone())
                .into_widget(),
        );
    }
    cells.push(expanded(gap_w(0.0)).into_widget());
    cells.push(
        component_props(
            render_copy,
            CopyProps {
                code: code.to_string(),
                color: mix(s.code_color, s.code_bg, 0.4),
                size: s.body_size * 0.95,
            },
        )
        .into_widget(),
    );
    row(cells)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .main_axis_size(MainAxisSize::Min)
        .into_widget()
}

/// Props for the per-code-block copy button (its own component so the "copied!"
/// flash is per-widget state).
pub(crate) struct CopyProps {
    code: String,
    color: Color,
    size: f32,
}

pub(crate) fn render_copy(p: &CopyProps) -> AnyWidget {
    let copied = pebbles_core::create_signal(false);
    let is_copied = copied.get();
    let id = pebbles_core::owner_id().unwrap_or(0) ^ 0xC0DE_0000_0000_0000;
    pebbles_core::create_cleanup(move || pebbles_core::clear_timeout(id));
    let code = p.code.clone();
    let (ic, col) = if is_copied {
        (lucide::CLIPBOARD_CHECK, palette::emerald::S500)
    } else {
        (lucide::COPY, p.color)
    };
    gesture_detector(
        pebbles_widgets::components::icon(ic)
            .size(f64::from(p.size))
            .color(col),
    )
    .cursor(Cursor::Pointer)
    .on_tap(move || {
        pebbles_core::clipboard::write(&code);
        copied.set(true);
        // Revert the check-mark after a short flash.
        pebbles_core::set_timeout(id, 1.5, move || copied.set(false));
    })
    .into_widget()
}

/// Render code as one row per line: an optional right-aligned line-number gutter,
/// then the line's colored spans, with a subtle background on highlighted lines.
pub(crate) fn render_code_lines(
    lines: &[Vec<(String, Color)>],
    s: &MarkdownStyle,
    hl: &[u32],
    numbers: bool,
    size: f32,
) -> AnyWidget {
    let digits = lines.len().max(1).to_string().len();
    let gutter_w = f64::from(size) * 0.62 * digits as f64 + 12.0;
    let num_color = mix(s.code_color, s.code_bg, 0.5);
    let hl_bg = mix(s.code_bg, s.body_color, 0.08);
    let mut rows: Vec<AnyWidget> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let n = (i + 1) as u32;
        let mut cells: Vec<AnyWidget> = Vec::new();
        if numbers {
            cells.push(
                container()
                    .width(gutter_w)
                    .child(
                        text(n.to_string())
                            .size(size)
                            .color(num_color)
                            .font_family(s.code_family.clone())
                            .align(TextAlign::Right),
                    )
                    .into_widget(),
            );
            cells.push(gap_w(8.0).into_widget());
        }
        let spans: Vec<TextSpan> = if line.is_empty() {
            vec![span(" ")]
        } else {
            line.iter()
                .map(|(t, c)| span(t.clone()).color(*c))
                .collect()
        };
        cells.push(
            expanded(
                text_rich(spans)
                    .size(size)
                    .color(s.code_color)
                    .font_family(s.code_family.clone())
                    .soft_wrap(false),
            )
            .into_widget(),
        );
        let r = row(cells)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_size(MainAxisSize::Min);
        if hl.contains(&n) {
            rows.push(container().color(hl_bg).child(r).into_widget());
        } else {
            rows.push(r.into_widget());
        }
    }
    column(rows)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .into_widget()
}

pub(crate) fn render_block(b: &Block, cx: &Cx) -> AnyWidget {
    let s = &cx.style;
    // A host block-renderer may replace any block's rendering.
    if let Some(f) = &cx.block_renderer
        && let Some(w) = f(block_kind(b), &block_plain(b))
    {
        return w;
    }
    match b {
        Block::FrontMatter(props) => {
            // A compact properties table: muted keys, themed values, in a subtle box.
            let bg = mix(theme().colors.background, theme().colors.foreground, 0.03);
            let mut rows: Vec<AnyWidget> = Vec::with_capacity(props.len());
            for (k, v) in props {
                rows.push(
                    row(children![
                        container().width(120.0).child(
                            text(k.clone())
                                .size(s.body_size * 0.9)
                                .weight(600.0)
                                .color(s.quote_color),
                        ),
                        gap_w(10.0),
                        expanded(text(v.clone()).size(s.body_size * 0.9).color(cx.color)),
                    ])
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min)
                    .into_widget(),
                );
            }
            container()
                .decoration(
                    BoxDecoration::new()
                        .color(bg)
                        .radius(BorderRadius::all(6.0))
                        .border(Border::new(s.table_border, 1.0)),
                )
                .padding(EdgeInsets::all(10.0))
                .child(
                    column(rows)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .main_axis_size(MainAxisSize::Min)
                        .spacing(4.0),
                )
                .into_widget()
        }
        Block::Heading(lvl, inlines, _slug) => {
            let size = s.body_size * s.heading_scale[(*lvl as usize - 1).min(5)];
            let w = inline_flow(inlines, cx, size, s.heading_color, true, TextAlign::Start);
            // Announce the heading + its level to screen readers.
            semantics(
                SemanticsRole::Label,
                format!("Heading {lvl}, {}", inline_plain(inlines)),
                w,
            )
            .into_widget()
        }
        Block::Para(inlines) => {
            inline_flow(inlines, cx, s.body_size, cx.color, false, TextAlign::Start)
        }
        Block::Code { lang, text: code } => render_code(lang, code, cx),
        Block::Quote(body) => {
            let quoted = Cx {
                color: s.quote_color,
                ..cx.clone()
            };
            row(children![
                container().width(3.0).color(s.quote_bar),
                gap_w(10.0),
                expanded(render_blocks(body, &quoted)),
            ])
            .main_axis_size(MainAxisSize::Min)
            .into_widget()
        }
        Block::Callout { kind, body } => render_callout(*kind, body, cx),
        Block::DefList(entries) => {
            let mut rows: Vec<AnyWidget> = Vec::new();
            for (term, defs) in entries {
                rows.push(inline_flow(
                    term,
                    cx,
                    s.body_size,
                    cx.color,
                    true,
                    TextAlign::Start,
                ));
                rows.push(
                    padding(
                        EdgeInsets::only(16.0, 2.0, 0.0, 0.0),
                        render_blocks(defs, cx),
                    )
                    .into_widget(),
                );
            }
            column(rows)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min)
                .spacing(4.0)
                .into_widget()
        }
        Block::Details {
            summary,
            open,
            body,
        } => component_props(
            render_details,
            DetailsProps {
                summary: summary.clone(),
                body: body.clone(),
                open: *open,
                cx: cx.clone(),
            },
        )
        .into_widget(),
        Block::Toc(entries) => {
            let mut rows: Vec<AnyWidget> = Vec::new();
            let top = entries.iter().map(|(l, ..)| *l).min().unwrap_or(1);
            for (level, title, slug) in entries {
                let indent = f64::from(level.saturating_sub(top)) * 14.0;
                let on_link = cx.on_link.clone();
                let target = format!("#{slug}");
                rows.push(
                    padding(
                        EdgeInsets::only(indent, 0.0, 0.0, 0.0),
                        gesture_detector(text(title.clone()).size(s.body_size).color(s.link_color))
                            .cursor(Cursor::Pointer)
                            .on_tap(move || {
                                if let Some(f) = &on_link {
                                    f(&target);
                                }
                            }),
                    )
                    .into_widget(),
                );
            }
            column(rows)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .main_axis_size(MainAxisSize::Min)
                .spacing(3.0)
                .into_widget()
        }
        Block::List {
            ordered,
            start,
            items,
            tight,
        } => {
            let mut rows: Vec<AnyWidget> = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let marker: AnyWidget = match item.task {
                    Some((checked, ordinal)) => {
                        let (bound, on_task) = (cx.bound, cx.on_task.clone());
                        gesture_detector(
                            pebbles_widgets::components::icon(if checked {
                                lucide::SQUARE_CHECK
                            } else {
                                lucide::SQUARE
                            })
                            .size(15.0)
                            .color(if checked {
                                s.link_color
                            } else {
                                cx.color
                            }),
                        )
                        .cursor(Cursor::Pointer)
                        .on_tap(move || {
                            // Rewrite the bound source, then report.
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
                let item_label = item.blocks.first().map(block_plain).unwrap_or_default();
                rows.push(
                    semantics(
                        SemanticsRole::ListItem,
                        item_label,
                        row(children![
                            container().width(22.0).child(marker),
                            expanded(render_blocks(&item.blocks, cx)),
                        ])
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .main_axis_size(MainAxisSize::Min),
                    )
                    .into_widget(),
                );
            }
            // Tight lists (no blank lines) render compact; loose lists get
            // block-level spacing between items.
            let item_gap = if *tight { 4.0 } else { s.block_gap };
            let list = padding(
                EdgeInsets::only(8.0, 0.0, 0.0, 0.0),
                column(rows)
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .main_axis_size(MainAxisSize::Min)
                    .spacing(item_gap),
            );
            semantics(SemanticsRole::List, "", list).into_widget()
        }
        Block::Rule => container().height(1.0).color(s.rule_color).into_widget(),
        Block::Table {
            header,
            aligns,
            rows,
        } => {
            // Content-sized, alignment-aware columns via the framework's table
            // layout, wrapped in a horizontal scroll so a WIDE table scrolls
            // instead of squishing (columns capped so one huge cell can't dominate).
            let ncols = header
                .len()
                .max(rows.iter().map(Vec::len).max().unwrap_or(0))
                .max(1);
            let col_align = |i: usize| match aligns.get(i) {
                Some(CellAlign::Center) => TextAlign::Center,
                Some(CellAlign::End) => TextAlign::Right,
                _ => TextAlign::Left,
            };
            let empty: Vec<Inline> = Vec::new();
            let header_bg = mix(theme().colors.background, theme().colors.foreground, 0.04);
            let cell =
                |inlines: &[Inline], bold: bool, align: TextAlign, head: bool| -> AnyWidget {
                    let content = padding(
                        EdgeInsets::symmetric(9.0, 6.0),
                        inline_flow(
                            inlines,
                            cx,
                            cx.style.body_size * 0.95,
                            cx.color,
                            bold,
                            align,
                        ),
                    );
                    if head {
                        container().color(header_bg).child(content).into_widget()
                    } else {
                        content.into_widget()
                    }
                };

            let mut table_rows: Vec<Vec<AnyWidget>> = Vec::with_capacity(rows.len() + 1);
            table_rows.push(
                (0..ncols)
                    .map(|i| cell(header.get(i).unwrap_or(&empty), true, col_align(i), true))
                    .collect(),
            );
            for r in rows {
                table_rows.push(
                    (0..ncols)
                        .map(|i| cell(r.get(i).unwrap_or(&empty), false, col_align(i), false))
                        .collect(),
                );
            }

            let table = layout_table(table_rows)
                .column_widths(vec![TableColumnWidth::IntrinsicMax(420.0); ncols])
                .divider(s.table_border, 0.7);
            container()
                .decoration(
                    BoxDecoration::new()
                        .border(Border::new(s.table_border, 1.0))
                        .radius(BorderRadius::all(6.0)),
                )
                .child(SingleChildScrollView::horizontal(table))
                .into_widget()
        }
        Block::Footnotes(entries) => {
            // A separator rule, then each definition prefixed with its number.
            let mut kids: Vec<AnyWidget> =
                vec![container().height(1.0).color(s.rule_color).into_widget()];
            for (n, blocks) in entries {
                kids.push(
                    row(children![
                        container().width(28.0).child(
                            text(format!("{n}."))
                                .size(s.body_size * 0.9)
                                .color(s.quote_color),
                        ),
                        expanded(render_blocks(blocks, cx)),
                    ])
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .main_axis_size(MainAxisSize::Min)
                    .into_widget(),
                );
            }
            column(kids)
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min)
                .spacing(6.0)
                .into_widget()
        }
        Block::Backlinks(links) => {
            // A separator + "Backlinks" heading + clickable entries.
            let mut kids: Vec<AnyWidget> = vec![
                container().height(1.0).color(s.rule_color).into_widget(),
                text("Backlinks".to_string())
                    .size(s.body_size * 1.05)
                    .weight(700.0)
                    .color(s.heading_color)
                    .into_widget(),
            ];
            for (label, target) in links {
                let on_link = cx.on_link.clone();
                let target = target.clone();
                kids.push(
                    gesture_detector(text(label.clone()).size(s.body_size).color(s.link_color))
                        .cursor(Cursor::Pointer)
                        .on_tap(move || {
                            if let Some(f) = &on_link {
                                f(&target);
                            }
                        })
                        .into_widget(),
                );
            }
            column(kids)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .main_axis_size(MainAxisSize::Min)
                .spacing(4.0)
                .into_widget()
        }
    }
}

/// Render a number as Unicode superscript digits (`12` → `¹²`).
pub(crate) fn superscript(n: usize) -> String {
    const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    n.to_string()
        .chars()
        .map(|c| c.to_digit(10).map_or(c, |d| SUP[d as usize]))
        .collect()
}

/// Render a GFM/Obsidian callout: an accent bar + tinted background, a titled
/// icon row, then the body.
pub(crate) fn render_callout(kind: CalloutKind, body: &[Block], cx: &Cx) -> AnyWidget {
    let (label, accent) = match kind {
        CalloutKind::Note => ("Note", palette::sky::S500),
        CalloutKind::Tip => ("Tip", palette::emerald::S500),
        CalloutKind::Important => ("Important", palette::violet::S500),
        CalloutKind::Warning => ("Warning", palette::amber::S500),
        CalloutKind::Caution => ("Caution", palette::rose::S500),
    };
    let ic = match kind {
        CalloutKind::Note => lucide::INFO,
        CalloutKind::Tip => lucide::LIGHTBULB,
        CalloutKind::Important => lucide::CIRCLE_ALERT,
        CalloutKind::Warning => lucide::TRIANGLE_ALERT,
        CalloutKind::Caution => lucide::OCTAGON_ALERT,
    };
    let s = &cx.style;
    let bg = mix(theme().colors.background, accent, 0.10);
    let title = row(children![
        pebbles_widgets::components::icon(ic)
            .size(16.0)
            .color(accent),
        gap_w(6.0),
        text(label.to_string())
            .size(s.body_size * 0.95)
            .weight(700.0)
            .color(accent),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .main_axis_size(MainAxisSize::Min);
    let inner = column(children![title.into_widget(), render_blocks(body, cx)])
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .spacing(6.0);
    container()
        .decoration(
            BoxDecoration::new()
                .color(bg)
                .radius(BorderRadius::all(6.0)),
        )
        .child(
            row(children![
                container().width(3.0).color(accent),
                expanded(padding(EdgeInsets::all(10.0), inner)),
            ])
            .main_axis_size(MainAxisSize::Min),
        )
        .into_widget()
}

/// Props for the collapsible `<details>` component (its own component so the
/// open/closed signal is per-widget state, per the framework's hook rules).
pub(crate) struct DetailsProps {
    summary: Vec<Inline>,
    body: Vec<Block>,
    open: bool,
    cx: Cx,
}

pub(crate) fn render_details(p: &DetailsProps) -> AnyWidget {
    let open = pebbles_core::create_signal(p.open);
    let is_open = open.get();
    let cx = &p.cx;
    let s = &cx.style;
    let chevron = if is_open {
        lucide::CHEVRON_DOWN
    } else {
        lucide::CHEVRON_RIGHT
    };
    let header = gesture_detector(
        row(children![
            pebbles_widgets::components::icon(chevron)
                .size(16.0)
                .color(cx.color),
            gap_w(4.0),
            expanded(inline_flow(
                &p.summary,
                cx,
                s.body_size,
                cx.color,
                true,
                TextAlign::Start
            )),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .main_axis_size(MainAxisSize::Min),
    )
    .cursor(Cursor::Pointer)
    .on_tap(move || open.set(!open.peek()));
    let mut kids: Vec<AnyWidget> = vec![header.into_widget()];
    if is_open {
        kids.push(
            padding(
                EdgeInsets::only(20.0, 4.0, 0.0, 0.0),
                render_blocks(&p.body, cx),
            )
            .into_widget(),
        );
    }
    column(kids)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .main_axis_size(MainAxisSize::Min)
        .spacing(4.0)
        .into_widget()
}

/// Lay inline runs out as ONE rich paragraph — bold/italic/strike/links/inline
/// code are per-range spans over a single shaped layout, so word wrapping,
/// inter-word spacing, and BiDi are the text engine's job (never a widget per
/// word). Images split the flow: text segments and images compose in a wrap
/// (the rare mixed case).
pub(crate) fn inline_flow(
    inlines: &[Inline],
    cx: &Cx,
    size: f32,
    color: Color,
    bold_all: bool,
    align: TextAlign,
) -> AnyWidget {
    let s = &cx.style;
    let heading_family = (size != s.body_size)
        .then(|| s.heading_family.clone())
        .flatten();

    let mut parts: Vec<AnyWidget> = Vec::new();
    let mut spans: Vec<TextSpan> = Vec::new();
    let flush = |spans: &mut Vec<TextSpan>, parts: &mut Vec<AnyWidget>| {
        if spans.is_empty() {
            return;
        }
        // Keyboard-nav applies only to paragraphs that actually carry a link.
        let has_link = spans.iter().any(|sp| sp.link.is_some());
        let mut rich = text_rich(std::mem::take(spans))
            .size(size)
            .color(color)
            .align(align);
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
        if let Some(f) = &cx.on_link_hover {
            let f = f.clone();
            rich = rich.on_link_hover(move |u| f(u));
        }
        if cx.keyboard && has_link && cx.on_link.is_some() {
            // A link paragraph becomes a Tab focus stop with arrow-key traversal;
            // it opts out of drag-selection so focus stays clean.
            rich = rich.keyboard_nav();
        } else if cx.selectable {
            match &cx.sel_group {
                // Document-wide selection: each leaf joins the group at the next index.
                Some(group) => {
                    let idx = cx.sel_index.get();
                    cx.sel_index.set(idx + 1);
                    rich = rich.selectable_in(*group, idx);
                }
                None => rich = rich.selectable(),
            }
        }
        parts.push(rich.into_widget());
    };
    for inline in inlines {
        match inline {
            Inline::Run(run) => {
                // Split the run on find matches (never inside inline code) so each
                // match gets the find-highlight chip; otherwise one segment.
                let segments = match &cx.find {
                    Some(q) if !run.code => split_matches(&run.text, q),
                    _ => vec![(run.text.clone(), false)],
                };
                for (seg, is_match) in segments {
                    let mut sp = span(seg);
                    if run.tag {
                        // An Obsidian `#tag`: a clickable chip (no link underline).
                        sp = sp.chip(s.tag_bg).color(s.tag_fg).size(size * 0.92);
                        if let Some(url) = &run.link
                            && (cx.allow_all_schemes || is_safe_link(url))
                        {
                            sp = sp.link(url.clone());
                        }
                        if is_match {
                            sp = sp.chip(s.find_bg).color(s.find_fg);
                        }
                        spans.push(sp);
                        continue;
                    }
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
                        if run.mark {
                            sp = sp.chip(s.mark_bg).color(s.mark_fg);
                        }
                        // Sub/superscript are approximated as a smaller size (the text
                        // engine exposes no baseline shift on a span).
                        if run.sub || run.sup {
                            sp = sp.size(size * 0.75);
                        }
                    }
                    if let Some(url) = &run.link
                        && (cx.allow_all_schemes || is_safe_link(url))
                    {
                        sp = sp.link(url.clone()).underline().color(s.link_color);
                    }
                    if is_match {
                        sp = sp.chip(s.find_bg).color(s.find_fg);
                    }
                    spans.push(sp);
                }
            }
            Inline::Image { url, alt, embed } => {
                flush(&mut spans, &mut parts);
                if *embed && !is_image_path(url) {
                    // A `![[Note]]` transclusion: real vault-content embedding needs
                    // a resolver, so render a clickable reference to the target.
                    parts.push(render_embed_ref(url, cx));
                } else if *embed {
                    // An image embed `![[pic.png|200]]`: the alt is the filename or a
                    // size — keep only a size spec (never the filename as a caption).
                    let a = if parse_image_alt(alt).1.is_some() {
                        alt.clone()
                    } else {
                        String::new()
                    };
                    parts.push(render_image(url, &a, cx));
                } else {
                    parts.push(render_image(url, alt, cx));
                }
            }
            Inline::Footnote(n) => {
                // A superscript reference marker in the link color (no navigation
                // target — the definition renders at the document end).
                spans.push(span(superscript(*n)).color(s.link_color).size(size * 0.85));
            }
            Inline::Math { tex, display } => {
                if *display {
                    // Display math gets the 2-D layout engine (fractions, roots, …).
                    flush(&mut spans, &mut parts);
                    parts.push(crate::math::render_display(tex, size * 1.05, color));
                } else {
                    // Inline math stays a compact single-line Unicode transform.
                    spans.push(
                        span(crate::advanced::tex_to_unicode(tex))
                            .italic()
                            .color(mix(color, s.link_color, 0.2)),
                    );
                }
            }
        }
    }
    flush(&mut spans, &mut parts);
    match parts.len() {
        0 => text_rich(vec![span("")])
            .size(size)
            .color(color)
            .into_widget(),
        1 => parts.pop().expect("one part"),
        _ => wrap(parts).spacing(4.0).run_spacing(3.0).into_widget(),
    }
}

#[cfg(feature = "image-view")]
pub(crate) fn render_image(url: &str, alt: &str, cx: &Cx) -> AnyWidget {
    let (caption, w, h) = parse_image_alt(alt);
    let mut view = if url.starts_with("http://") || url.starts_with("https://") {
        pebbles_widgets::ImageView::network(url)
    } else {
        pebbles_widgets::ImageView::asset(url)
    }
    .radius(BorderRadius::all(6.0))
    .fit(ImageFit::Contain);
    view = match (w, h) {
        (Some(w), Some(h)) => view.size(w, h),
        (Some(w), None) => view.width(w),
        _ => view.height(220.0),
    };
    let img = view.into_widget();
    let a11y = caption.clone().unwrap_or_else(|| "image".to_string());
    let w = match caption {
        Some(cap) => column(children![
            img,
            text(cap)
                .size(cx.style.body_size * 0.85)
                .color(cx.style.quote_color)
                .italic()
                .align(TextAlign::Center),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .main_axis_size(MainAxisSize::Min)
        .spacing(4.0)
        .into_widget(),
        None => img,
    };
    semantics(SemanticsRole::Image, a11y, w).into_widget()
}

#[cfg(not(feature = "image-view"))]
pub(crate) fn render_image(_url: &str, alt: &str, cx: &Cx) -> AnyWidget {
    // Without the image stack, show the caption (or a generic label) as a quiet
    // placeholder — never the raw size spec.
    let label = parse_image_alt(alt)
        .0
        .unwrap_or_else(|| "image".to_string());
    let w = text(format!("[{label}]"))
        .size(cx.style.body_size)
        .color(cx.style.quote_color)
        .italic();
    semantics(SemanticsRole::Image, label, w).into_widget()
}

/// A `![[Note]]` embed. With a [`Markdown::resolver`], the target's content is
/// transcluded inline (bounded against cycles) in a titled card; otherwise a
/// clickable reference chip is shown. Either way the target previews on hover.
pub(crate) fn render_embed_ref(target: &str, cx: &Cx) -> AnyWidget {
    let s = &cx.style;
    let name = target.to_string();

    // Transclude resolved content when a resolver is wired and we're within depth.
    if let Some(resolver) = &cx.resolver
        && cx.embed_depth < MAX_EMBED_DEPTH
        && let Some(content) = resolver(target)
    {
        let inner_cx = Cx {
            embed_depth: cx.embed_depth + 1,
            ..cx.clone()
        };
        let blocks = parse_blocks(&content);
        let header = row(children![
            pebbles_widgets::components::icon(lucide::FILE_TEXT)
                .size(f64::from(s.body_size * 0.85))
                .color(s.quote_color),
            gap_w(6.0),
            text(target.to_string())
                .size(s.body_size * 0.85)
                .weight(600.0)
                .color(s.quote_color),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .main_axis_size(MainAxisSize::Min);
        let card = container()
            .decoration(
                BoxDecoration::new()
                    .color(mix(theme().colors.background, s.link_color, 0.04))
                    .radius(BorderRadius::all(6.0))
                    .border(Border::new(s.table_border, 1.0)),
            )
            .padding(EdgeInsets::all(10.0))
            .child(
                column(children![
                    header.into_widget(),
                    render_blocks(&blocks, &inner_cx)
                ])
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .main_axis_size(MainAxisSize::Min)
                .spacing(6.0),
            );
        return pebbles_widgets::components::tooltip(format!("Embedded note: {target}"), card)
            .into_widget();
    }

    // Fallback: a clickable reference chip that previews the target on hover.
    let on_link = cx.on_link.clone();
    let hover = cx
        .resolver
        .as_ref()
        .and_then(|r| r(target))
        .map(|c| preview_snippet(&c))
        .unwrap_or_else(|| name.clone());
    let inner = row(children![
        pebbles_widgets::components::icon(lucide::FILE_TEXT)
            .size(f64::from(s.body_size * 0.9))
            .color(s.link_color),
        gap_w(6.0),
        text(target.to_string())
            .size(s.body_size)
            .color(s.link_color),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .main_axis_size(MainAxisSize::Min);
    let chip = gesture_detector(
        container()
            .decoration(
                BoxDecoration::new()
                    .color(mix(theme().colors.background, s.link_color, 0.06))
                    .radius(BorderRadius::all(6.0))
                    .border(Border::new(s.table_border, 1.0)),
            )
            .padding(EdgeInsets::symmetric(10.0, 6.0))
            .child(inner),
    )
    .cursor(Cursor::Pointer)
    .on_tap(move || {
        if let Some(f) = &on_link {
            f(&name);
        }
    });
    pebbles_widgets::components::tooltip(hover, chip).into_widget()
}

/// A one-line preview of Markdown content (first non-empty line, capped), for
/// hover tooltips.
pub(crate) fn preview_snippet(content: &str) -> String {
    let line = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() > 120 {
        let cut: String = line.chars().take(117).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

// ---------------------------------------------------------------------------
