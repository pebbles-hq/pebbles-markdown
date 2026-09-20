//! Unit tests for the parser + pure helpers (they reach crate-internal
//! items, so they live in-crate rather than in `tests/`).

use pebbles_foundation::Color;

use super::*;

fn inline_text(inls: &[Inline]) -> String {
    inls.iter()
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r.text.clone()),
            Inline::Image { .. } | Inline::Footnote(_) | Inline::Math { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn first_para_text(b: &Block) -> String {
    match b {
        Block::Para(inls) | Block::Heading(_, inls, _) => inline_text(inls),
        _ => String::new(),
    }
}

/// The inline runs of a paragraph/heading block.
fn runs(b: &Block) -> Vec<Run> {
    match b {
        Block::Para(inls) | Block::Heading(_, inls, _) => inls
            .iter()
            .filter_map(|i| match i {
                Inline::Run(r) => Some(r.clone()),
                Inline::Image { .. } | Inline::Footnote(_) | Inline::Math { .. } => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

// §1: a TIGHT list (no blank lines) and a LOOSE list (blank-line-separated)
// must be distinguished so the renderer can space them differently.
#[test]
fn tight_and_loose_lists_are_distinguished() {
    let tight = parse_blocks("- a\n- b\n- c\n");
    let Some(Block::List {
        tight: t, items, ..
    }) = tight.first()
    else {
        panic!("tight list");
    };
    assert!(*t, "no blank lines between items → tight");
    assert_eq!(items.len(), 3);

    let loose = parse_blocks("- a\n\n- b\n\n- c\n");
    let Some(Block::List {
        tight: t, items, ..
    }) = loose.first()
    else {
        panic!("loose list");
    };
    assert!(!*t, "blank lines between items → loose");
    assert_eq!(items.len(), 3);
}

// §1: `---`/`+++` front matter is parsed into a properties block and stripped
// from the body — it must NOT render as a thematic break + setext heading.
#[test]
fn front_matter_is_parsed_and_stripped() {
    let blocks = parse_blocks("---\ntitle: Hello World\ntags: notes\n---\n# Body\n");
    let Some(Block::FrontMatter(props)) = blocks.first() else {
        panic!(
            "first block is front matter, got {:?}",
            blocks.first().map(std::mem::discriminant)
        );
    };
    assert_eq!(props.len(), 2);
    assert_eq!(props[0], ("title".to_string(), "Hello World".to_string()));
    assert_eq!(props[1], ("tags".to_string(), "notes".to_string()));
    // The body heading survives; nothing became a stray Rule.
    assert!(blocks.iter().any(|b| matches!(b, Block::Heading(1, ..))));
    assert!(
        !blocks.iter().any(|b| matches!(b, Block::Rule)),
        "no stray thematic break"
    );

    // TOML front matter (`+++`, `key = "value"`).
    let toml = parse_blocks("+++\ntitle = \"T\"\n+++\ntext\n");
    assert!(matches!(toml.first(), Some(Block::FrontMatter(_))));
}

// §1 guard: a setext H2 (`text` underlined by `---`) that merely LOOKS like a
// front-matter fence must not be eaten — it has no `key: value`, so it stays.
#[test]
fn setext_lookalike_is_not_treated_as_front_matter() {
    let blocks = parse_blocks("---\nSome heading\n---\nmore\n");
    assert!(
        !matches!(blocks.first(), Some(Block::FrontMatter(_))),
        "no key/value pairs → not front matter"
    );
    // The text is preserved somewhere (not dropped).
    let all: String = blocks.iter().map(first_para_text).collect();
    assert!(all.contains("Some heading") || all.contains("more"));
}

// §1: block-level HTML renders as real blocks, not escaped inline text.
#[test]
fn html_blocks_become_native_blocks() {
    assert!(
        parse_blocks("<hr>\n")
            .iter()
            .any(|b| matches!(b, Block::Rule)),
        "<hr> → thematic break"
    );

    let h = parse_blocks("<h2>Hello</h2>\n");
    let Some(Block::Heading(2, inls, _)) = h.first() else {
        panic!("<h2> → Heading(2)")
    };
    assert_eq!(inline_text(inls).trim(), "Hello");

    let list = parse_blocks("<ul><li>one</li><li>two</li></ul>\n");
    let Some(Block::List { items, .. }) = list.first() else {
        panic!("<ul> → List")
    };
    assert_eq!(items.len(), 2);

    let quote = parse_blocks("<blockquote>quoted</blockquote>\n");
    assert!(
        quote.iter().any(|b| matches!(b, Block::Quote(_))),
        "<blockquote> → Quote"
    );
}

// §1: inline HTML tags drive the inline formatting state.
#[test]
fn inline_html_formats_runs() {
    let blocks = parse_blocks("a <b>bold</b> <i>it</i> <a href=\"u\">link</a> <code>c</code>\n");
    let rs = runs(&blocks[0]);
    assert!(
        rs.iter().any(|r| r.text == "bold" && r.bold),
        "<b> → bold run"
    );
    assert!(
        rs.iter().any(|r| r.text == "it" && r.italic),
        "<i> → italic run"
    );
    assert!(
        rs.iter()
            .any(|r| r.text == "link" && r.link.as_deref() == Some("u")),
        "<a href> → link run"
    );
    assert!(
        rs.iter().any(|r| r.text == "c" && r.code),
        "<code> → code run"
    );

    // <br> becomes a hard line break.
    let br = parse_blocks("x<br>y\n");
    assert!(
        runs(&br[0]).iter().any(|r| r.text == "\n"),
        "<br> → newline run"
    );
}

// §2: bare URLs, `www.` hosts, and emails become links (GFM extended autolinks).
#[test]
fn bare_urls_and_emails_autolink() {
    let rs = runs(&parse_blocks("Visit https://example.com/page now\n")[0]);
    assert!(
        rs.iter().any(|r| r.text == "https://example.com/page"
            && r.link.as_deref() == Some("https://example.com/page")),
        "bare https URL becomes a link"
    );
    assert!(
        rs.iter()
            .any(|r| r.text.contains("Visit") && r.link.is_none())
    );

    // `www.` gets an `http://` scheme; the trailing period is not part of it.
    let rs = runs(&parse_blocks("see www.example.com.\n")[0]);
    let link = rs.iter().find(|r| r.link.is_some()).expect("a link");
    assert_eq!(link.text, "www.example.com");
    assert_eq!(link.link.as_deref(), Some("http://www.example.com"));
    assert!(
        rs.iter().any(|r| r.text == "." && r.link.is_none()),
        "trailing '.' stays text"
    );

    // Bare email → mailto:.
    let rs = runs(&parse_blocks("mail me@sample.org please\n")[0]);
    assert!(
            rs.iter()
                .any(|r| r.text == "me@sample.org"
                    && r.link.as_deref() == Some("mailto:me@sample.org")),
            "bare email becomes a mailto link"
        );
}

// §2: autolinking respects parens, inline code, explicit links, and word boundaries.
#[test]
fn autolink_is_scoped_correctly() {
    // A wrapping `)` is not swallowed into the link.
    let rs = runs(&parse_blocks("(https://x.com/a)\n")[0]);
    assert_eq!(
        rs.iter()
            .find(|r| r.link.is_some())
            .map(|r| r.text.as_str()),
        Some("https://x.com/a")
    );

    // Inline code is never autolinked.
    let rs = runs(&parse_blocks("`https://x.com`\n")[0]);
    assert!(
        rs.iter().all(|r| r.link.is_none()),
        "code is never autolinked"
    );

    // An explicit link's destination wins; its text is not re-linked.
    let rs = runs(&parse_blocks("[text https://inner.com](https://outer.com)\n")[0]);
    assert!(
        rs.iter()
            .all(|r| r.link.as_deref() == Some("https://outer.com")),
        "explicit destination kept, inner text not re-linked"
    );

    // Mid-word (no boundary) is not a link.
    let rs = runs(&parse_blocks("xhttps://y.com\n")[0]);
    assert!(
        rs.iter().all(|r| r.link.is_none()),
        "no boundary → not a link"
    );
}

/// The footnote-reference numbers inside a paragraph/heading, in order.
fn footnote_refs(b: &Block) -> Vec<usize> {
    match b {
        Block::Para(inls) | Block::Heading(_, inls, _) => inls
            .iter()
            .filter_map(|i| match i {
                Inline::Footnote(n) => Some(*n),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

// §3: GFM table column alignment is captured from the `:--:` header row.
#[test]
fn table_column_alignment_is_captured() {
    let blocks = parse_blocks("| L | C | R |\n|:--|:-:|--:|\n| a | b | c |\n");
    let Some(Block::Table { aligns, rows, .. }) = blocks.first() else {
        panic!("a table");
    };
    assert!(
        matches!(
            aligns.as_slice(),
            [CellAlign::Start, CellAlign::Center, CellAlign::End]
        ),
        "left / center / right captured, got {aligns:?}"
    );
    assert_eq!(rows.len(), 1);
}

// §3: footnotes are collected to the end, numbered by first-reference order.
#[test]
fn footnotes_are_numbered_and_collected() {
    let blocks =
        parse_blocks("A ref[^a] and another[^b].\n\n[^a]: First note.\n[^b]: Second note.\n");
    assert_eq!(
        footnote_refs(&blocks[0]),
        vec![1, 2],
        "refs numbered in order"
    );

    let Some(Block::Footnotes(entries)) = blocks.last() else {
        panic!("a footnotes section at the end");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0, 1);
    assert_eq!(entries[1].0, 2);
    assert!(first_para_text(&entries[0].1[0]).contains("First"));
    assert!(first_para_text(&entries[1].1[0]).contains("Second"));
}

// §3: numbering follows REFERENCE order even when definitions are out of order.
#[test]
fn footnotes_number_by_reference_order() {
    let blocks = parse_blocks("See[^z] then[^a].\n\n[^a]: A body.\n[^z]: Z body.\n");
    assert_eq!(
        footnote_refs(&blocks[0]),
        vec![1, 2],
        "z referenced first → 1"
    );

    let Some(Block::Footnotes(entries)) = blocks.last() else {
        panic!("footnotes");
    };
    // Entry 1 is z (first referenced), entry 2 is a — not definition order.
    assert_eq!(entries[0].0, 1);
    assert!(first_para_text(&entries[0].1[0]).contains("Z body"));
    assert_eq!(entries[1].0, 2);
    assert!(first_para_text(&entries[1].1[0]).contains("A body"));
}

// §6: wikilinks (+ aliases + heading refs) become links.
#[test]
fn wikilinks_and_aliases() {
    let rs = runs(&parse_blocks("See [[Some Page]] here\n")[0]);
    assert!(
        rs.iter()
            .any(|r| r.text == "Some Page" && r.link.as_deref() == Some("Some Page")),
        "[[Page]] → link to Page"
    );
    let rs = runs(&parse_blocks("See [[Page|Nice Name]]\n")[0]);
    assert!(
        rs.iter()
            .any(|r| r.text == "Nice Name" && r.link.as_deref() == Some("Page")),
        "[[Page|alias]] → alias text, Page dest"
    );
}

// §6: `#tag`s become tag runs; `#123` (no letter) is not a tag.
#[test]
fn tags_are_detected() {
    let rs = runs(&parse_blocks("a #project and #status/active b\n")[0]);
    assert!(rs.iter().any(|r| r.text == "#project" && r.tag));
    assert!(rs.iter().any(|r| r.text == "#status/active" && r.tag));

    let rs = runs(&parse_blocks("issue #123 here\n")[0]);
    assert!(rs.iter().all(|r| !r.tag), "#123 is not a tag");
}

// §6: `%% … %%` comments are stripped from reading view.
#[test]
fn comments_are_hidden() {
    let rs = runs(&parse_blocks("before %% hidden note %% after\n")[0]);
    let text: String = rs.iter().map(|r| r.text.clone()).collect();
    assert!(!text.contains("hidden"), "comment text removed: {text:?}");
    assert!(text.contains("before") && text.contains("after"));
}

// §6: `![[…]]` embeds — note refs vs image embeds are distinguished.
#[test]
fn embeds_are_flagged() {
    assert!(is_image_path("pic.png"));
    assert!(is_image_path("dir/Photo.JPEG"));
    assert!(!is_image_path("Some Note"));

    let blocks = parse_blocks("![[Some Note]]\n");
    let Block::Para(inls) = &blocks[0] else {
        panic!("paragraph")
    };
    assert!(
        inls.iter()
            .any(|i| matches!(i, Inline::Image { embed: true, url, .. } if url == "Some Note")),
        "![[Note]] → wikilink embed"
    );
    let blocks = parse_blocks("![[pic.png]]\n");
    let Block::Para(inls) = &blocks[0] else {
        panic!("paragraph")
    };
    assert!(
        inls.iter()
            .any(|i| matches!(i, Inline::Image { embed: true, .. }))
    );
}

// §9: heading fold hides a section until the next same-or-higher heading.
#[test]
fn heading_fold_hides_section() {
    let blocks = parse_blocks("# A\n\npara1\n\n## B\n\npara2\n\n# C\n\npara3\n");
    // Collapsing H1 "A" hides everything up to the next H1 ("C").
    assert_eq!(
        hidden_flags(&blocks, &["a".to_string()]),
        vec![false, true, true, true, false, false]
    );
    // Collapsing H2 "B" hides only its paragraph (the next H1 ends the section).
    assert_eq!(
        hidden_flags(&blocks, &["b".to_string()]),
        vec![false, false, false, true, false, false]
    );
    // Nothing collapsed → nothing hidden.
    assert!(hidden_flags(&blocks, &[]).iter().all(|h| !h));
}

// §9: find splits text into case-insensitive match / non-match segments.
#[test]
fn find_splits_matches() {
    assert_eq!(
        split_matches("Hello World", "world"),
        vec![("Hello ".to_string(), false), ("World".to_string(), true)]
    );
    assert_eq!(
        split_matches("aXaXa", "x"),
        vec![
            ("a".to_string(), false),
            ("X".to_string(), true),
            ("a".to_string(), false),
            ("X".to_string(), true),
            ("a".to_string(), false),
        ]
    );
    assert_eq!(
        split_matches("none here", "zzz"),
        vec![("none here".to_string(), false)]
    );
    assert_eq!(split_matches("abc", ""), vec![("abc".to_string(), false)]);
}

// §10: inline wrap/unwrap + line-prefix toggle source transforms.
#[test]
fn format_helpers_wrap_and_prefix() {
    // Wrap a selection in bold, adjusting the selection to the inner text.
    let (s, a, b) = wrap_selection("hello world", 6, 11, "**");
    assert_eq!(s, "hello **world**");
    assert_eq!((a, b), (8, 13));
    // Toggling again unwraps it.
    let (s2, a2, b2) = wrap_selection(&s, a, b, "**");
    assert_eq!(s2, "hello world");
    assert_eq!((a2, b2), (6, 11));
    // Italic on the whole string.
    assert_eq!(wrap_selection("ab", 0, 2, "*").0, "*ab*");

    // Add a quote prefix to every touched line, then remove it.
    let (q, _, _) = toggle_line_prefix("a\nb", 0, 3, "> ");
    assert_eq!(q, "> a\n> b");
    assert_eq!(toggle_line_prefix(&q, 0, q.len(), "> ").0, "a\nb");
    // Bullet on a single line shifts the caret past the marker.
    let (bl, a, _) = toggle_line_prefix("item", 0, 4, "- ");
    assert_eq!(bl, "- item");
    assert_eq!(a, 2);
}

// §4: `:::type … :::` containers expand into typed callouts; code is untouched.
#[test]
fn containers_expand_to_callouts() {
    assert!(matches!(
        parse_blocks(":::warning\nBe careful\n:::\n").first(),
        Some(Block::Callout {
            kind: CalloutKind::Warning,
            ..
        })
    ));
    assert!(matches!(
        parse_blocks(":::tip\nHandy\n:::\n").first(),
        Some(Block::Callout {
            kind: CalloutKind::Tip,
            ..
        })
    ));
    assert!(matches!(
        parse_blocks(":::info\nplain\n:::\n").first(),
        Some(Block::Callout {
            kind: CalloutKind::Note,
            ..
        })
    ));
    // A `:::` inside a code fence stays literal (a code block, not a callout).
    assert!(matches!(
        parse_blocks("```\n:::note\nx\n:::\n```\n").first(),
        Some(Block::Code { .. })
    ));
}

// §13: link-scheme allowlist — dangerous schemes are not "safe".
#[test]
fn link_scheme_guard() {
    assert!(is_safe_link("https://x.com"));
    assert!(is_safe_link("http://x.com"));
    assert!(is_safe_link("mailto:a@b.com"));
    assert!(is_safe_link("tel:+123"));
    assert!(is_safe_link("/relative/path"));
    assert!(is_safe_link("#fragment"));
    assert!(is_safe_link("Some Wikilink")); // scheme-less
    assert!(!is_safe_link("javascript:alert(1)"));
    assert!(!is_safe_link("data:text/html,<x>"));
    assert!(!is_safe_link("vbscript:x"));
    assert!(!is_safe_link("file:///etc/passwd"));
    // Control/whitespace smuggling: browsers strip these before resolving the
    // scheme, so a disallowed scheme must not sneak past the allowlist by
    // hiding a newline/tab/space/other control char inside it.
    assert!(!is_safe_link("java\nscript:alert(1)"));
    assert!(!is_safe_link("ja\tvascript:alert(1)"));
    assert!(!is_safe_link("j a v a s c r i p t:alert(1)"));
    assert!(!is_safe_link("java\rscript:alert(1)"));
    assert!(!is_safe_link("javas\u{0}cript:alert(1)"));
    assert!(!is_safe_link(" javascript:alert(1)")); // leading whitespace
    assert!(!is_safe_link("da\tta:text/html,<x>"));
    assert!(!is_safe_link("vb\nscript:x"));
    // Clean allowlisted schemes still resolve after normalization.
    assert!(is_safe_link("https://x.com"));
    assert!(is_safe_link("mailto:a@b.com"));
}

// §15: HTML export renders the CommonMark/GFM core, strips front matter, and
// resolves emoji.
#[test]
fn html_export() {
    let html = to_html("# Hi\n\n**bold** and `code`\n");
    assert!(html.contains("<h1>"), "{html}");
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<code>code</code>"));

    let html = to_html("---\ntitle: Secret\n---\n# Body\n");
    assert!(html.contains("<h1>"));
    assert!(!html.contains("Secret"), "front matter is stripped: {html}");

    assert!(to_html("go :rocket:\n").contains('🚀'));
}

// §7: image alt parses into an optional caption + width/height.
#[test]
fn image_alt_size_and_caption() {
    assert_eq!(parse_image_alt("200"), (None, Some(200.0), None));
    assert_eq!(parse_image_alt("200x100"), (None, Some(200.0), Some(100.0)));
    assert_eq!(
        parse_image_alt("A caption"),
        (Some("A caption".to_string()), None, None)
    );
    assert_eq!(
        parse_image_alt("A caption|300"),
        (Some("A caption".to_string()), Some(300.0), None)
    );
    assert_eq!(parse_image_alt(""), (None, None, None));
}

// §5: a fence info string parses into a language + highlighted line numbers.
#[test]
fn fence_info_parses_language_and_highlights() {
    assert_eq!(parse_fence_info("rust"), ("rust".to_string(), vec![]));
    assert_eq!(
        parse_fence_info("rust {2,4-6}"),
        ("rust".to_string(), vec![2, 4, 5, 6])
    );
    assert_eq!(parse_fence_info("js{2}"), ("js".to_string(), vec![2]));
    assert_eq!(parse_fence_info("{1,3}"), (String::new(), vec![1, 3]));
    assert_eq!(parse_fence_info(""), (String::new(), vec![]));

    // The info string survives parsing on the Code block; render splits it.
    let blocks = parse_blocks("```rust {2,4}\na\nb\nc\nd\n```\n");
    let Some(Block::Code { lang, .. }) = blocks.first() else {
        panic!("a code block");
    };
    let (l, hl) = parse_fence_info(lang);
    assert_eq!(l, "rust");
    assert_eq!(hl, vec![2, 4]);
}

// §4: `> [!NOTE]` blockquotes become typed callouts; plain quotes stay quotes.
#[test]
fn callouts_are_recognized() {
    assert!(matches!(
        parse_blocks("> [!NOTE]\n> Body text\n").first(),
        Some(Block::Callout {
            kind: CalloutKind::Note,
            ..
        })
    ));
    assert!(matches!(
        parse_blocks("> [!WARNING]\n> Careful\n").first(),
        Some(Block::Callout {
            kind: CalloutKind::Warning,
            ..
        })
    ));
    assert!(
        matches!(
            parse_blocks("> just a quote\n").first(),
            Some(Block::Quote(_))
        ),
        "a plain blockquote stays a quote"
    );
}

// §4: sub/superscript, `==highlight==`, and emoji shortcodes flag/replace runs.
#[test]
fn subscript_superscript_highlight_and_emoji() {
    // pulldown requires the `~`/`^` delimiters to be space-flanked.
    let rs = runs(&parse_blocks("water H ~2~ O\n")[0]);
    assert!(rs.iter().any(|r| r.text == "2" && r.sub), "~2~ → subscript");
    let rs = runs(&parse_blocks("area x ^2^ end\n")[0]);
    assert!(
        rs.iter().any(|r| r.text == "2" && r.sup),
        "^2^ → superscript"
    );

    let rs = runs(&parse_blocks("a ==marked== b\n")[0]);
    assert!(
        rs.iter().any(|r| r.text == "marked" && r.mark),
        "==x== → highlight"
    );

    let rs = runs(&parse_blocks("go :rocket:\n")[0]);
    assert!(rs.iter().any(|r| r.text.contains('🚀')), ":rocket: → emoji");
}

// §4: inline math is captured (rendered via a TeX→Unicode transform).
#[test]
fn inline_math_is_captured() {
    let blocks = parse_blocks("Euler: $e^{i\\pi}+1=0$ done\n");
    let Block::Para(inls) = &blocks[0] else {
        panic!("paragraph")
    };
    assert!(
        inls.iter()
            .any(|i| matches!(i, Inline::Math { display: false, .. })),
        "$…$ → inline math"
    );
}

// §4: definition lists parse to term/definition pairs.
#[test]
fn definition_lists_parse() {
    let blocks = parse_blocks("Term\n: The definition.\n");
    let Some(Block::DefList(entries)) = blocks.first() else {
        panic!(
            "a definition list, got {:?}",
            blocks.first().map(std::mem::discriminant)
        );
    };
    assert_eq!(entries.len(), 1);
    assert!(inline_text(&entries[0].0).contains("Term"));
    assert!(!entries[0].1.is_empty(), "the term has a definition body");
}

// §4: headings get GitHub-style slugs (deduped), and `[[toc]]` builds a ToC.
#[test]
fn heading_slugs_and_toc() {
    let blocks = parse_blocks("# Hello World\n\n## Sub Section\n\n[toc]\n");
    let Some(Block::Heading(1, _, slug)) = blocks.first() else {
        panic!("first heading");
    };
    assert_eq!(slug, "hello-world");

    let toc = blocks
        .iter()
        .find_map(|b| if let Block::Toc(e) = b { Some(e) } else { None })
        .expect("a generated ToC");
    assert_eq!(toc.len(), 2);
    assert_eq!(toc[0].2, "hello-world");
    assert_eq!(toc[1].2, "sub-section");

    // Duplicate headings get `-2`, `-3`, … suffixes.
    let dup = parse_blocks("# Dup\n\n# Dup\n");
    let slugs: Vec<String> = dup
        .iter()
        .filter_map(|b| match b {
            Block::Heading(_, _, s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(slugs, vec!["dup".to_string(), "dup-2".to_string()]);
}

// §4: `<details>`/`<summary>` becomes a collapsible block.
#[test]
fn details_summary_becomes_collapsible() {
    let blocks = parse_blocks("<details><summary>More</summary>\n\nHidden body.\n\n</details>\n");
    let Some(Block::Details { summary, body, .. }) = blocks.first() else {
        panic!(
            "a details block, got {:?}",
            blocks.first().map(std::mem::discriminant)
        );
    };
    assert!(inline_text(summary).contains("More"), "summary captured");
    assert!(!body.is_empty(), "body captured");
}

// §1 hardening: malformed / unknown HTML never panics, decodes entities, and
// keeps the visible text (unknown tags are transparent).
#[test]
fn malformed_html_is_safe_and_transparent() {
    let src = "<b>open <i>nested <a href= broken <unknowntag>café</unknowntag> &amp; &#x2014; end";
    let blocks = parse_blocks(src);
    let text: String = blocks.iter().map(first_para_text).collect();
    assert!(text.contains("café"), "text inside unknown tags is kept");
    assert!(text.contains('&'), "&amp; decodes to &");
    assert!(text.contains('\u{2014}'), "&#x2014; decodes to em-dash");
}

// Regression: a TIGHT list emits item text as bare `Text` (no Paragraph), so
// the item text must be flushed into the item — not leaked into the next block.
#[test]
fn tight_list_items_keep_their_text_and_dont_leak_into_the_next_heading() {
    let blocks = parse_blocks("- [x] alpha\n- [ ] beta\n\n## Heading\n");
    assert_eq!(
        blocks.len(),
        2,
        "one list + one heading, got {}",
        blocks.len()
    );
    let Block::List { items, .. } = &blocks[0] else {
        panic!("first block should be a list")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].task.map(|(c, _)| c), Some(true));
    assert_eq!(items[1].task.map(|(c, _)| c), Some(false));
    assert!(
        first_para_text(&items[0].blocks[0]).contains("alpha"),
        "item 0 keeps its text"
    );
    assert!(
        first_para_text(&items[1].blocks[0]).contains("beta"),
        "item 1 keeps its text"
    );
    let Block::Heading(_, inls, _) = &blocks[1] else {
        panic!("second block should be a heading")
    };
    assert_eq!(
        inline_text(inls).trim(),
        "Heading",
        "heading must not absorb the list text"
    );
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
    let flat: Vec<(String, Color)> = highlight(code, "rust", &s, white)
        .into_iter()
        .flatten()
        .collect();
    let colored = |needle: &str| flat.iter().find(|(t, _)| t == needle).map(|(_, c)| *c);
    assert_eq!(colored("fn"), Some(red), "keyword");
    assert_eq!(colored("let"), Some(red), "keyword");
    assert_eq!(colored("\"hi\""), Some(green), "string");
    assert_eq!(colored("// note"), Some(gray), "comment");
    assert_eq!(colored("42"), Some(orange), "number");
    assert_eq!(colored("main"), Some(blue), "function-call identifier");
    // Indentation is preserved as a plain (base-colored) whitespace run.
    assert!(
        flat.iter()
            .any(|(t, c)| t.trim().is_empty() && !t.is_empty() && *c == white)
    );
}

#[test]
fn nested_list_text_stays_in_its_item() {
    let blocks = parse_blocks("1. Ordered\n2. Nested\n   - child\n");
    let Block::List { items, .. } = &blocks[0] else {
        panic!("list")
    };
    assert_eq!(items.len(), 2);
    assert!(first_para_text(&items[0].blocks[0]).contains("Ordered"));
    assert!(
        first_para_text(&items[1].blocks[0]).contains("Nested"),
        "nested item keeps its text"
    );
    assert!(
        items[1]
            .blocks
            .iter()
            .any(|b| matches!(b, Block::List { .. })),
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
        keyword: base,
        string: base,
        comment: base,
        number: base,
        ident: base,
        punct: base,
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
        let round: String = lines
            .iter()
            .flat_map(|l| l.iter().map(|(t, _)| t.as_str()))
            .collect();
        assert_eq!(round, src, "highlight round-trips {src:?} without loss");
    }
}

// A block comment left open at end-of-line carries across lines; unicode
// inside it must not split a char at the line join.
#[test]
fn highlight_open_block_comment_with_unicode() {
    let base = Color::from_rgba8(0, 0, 0, 255);
    let colors = SyntaxColors {
        keyword: base,
        string: base,
        comment: base,
        number: base,
        ident: base,
        punct: base,
    };
    let src = "before /* café\n世界 */ after";
    let lines = highlight(src, "rust", &colors, base);
    let round: String = lines
        .iter()
        .flat_map(|l| l.iter().map(|(t, _)| t.as_str()))
        .collect();
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
