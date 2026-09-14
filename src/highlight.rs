//! Syntax highlighting — a small, dependency-free lexer good enough to color the
//! common languages (C-family + a few script styles). Not a full grammar; it
//! tokenizes comments, strings, numbers, keywords, identifiers and punctuation,
//! and colors them from the themeable [`SyntaxColors`].

use pebbles_foundation::Color;

use crate::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tok {
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
pub(crate) fn lang_profile(lang: &str) -> (&'static str, bool) {
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
pub(crate) fn is_keyword(w: &str) -> bool {
    const KW: &[&str] = &[
        "fn",
        "let",
        "mut",
        "const",
        "static",
        "pub",
        "use",
        "mod",
        "impl",
        "trait",
        "struct",
        "enum",
        "type",
        "where",
        "as",
        "dyn",
        "ref",
        "move",
        "match",
        "if",
        "else",
        "for",
        "while",
        "loop",
        "return",
        "break",
        "continue",
        "in",
        "self",
        "Self",
        "super",
        "crate",
        "async",
        "await",
        "unsafe",
        "extern",
        "function",
        "var",
        "def",
        "class",
        "interface",
        "extends",
        "implements",
        "import",
        "from",
        "export",
        "default",
        "new",
        "delete",
        "try",
        "catch",
        "finally",
        "throw",
        "throws",
        "switch",
        "case",
        "do",
        "public",
        "private",
        "protected",
        "abstract",
        "final",
        "void",
        "int",
        "long",
        "float",
        "double",
        "bool",
        "boolean",
        "char",
        "string",
        "package",
        "func",
        "go",
        "defer",
        "chan",
        "map",
        "range",
        "nil",
        "None",
        "True",
        "False",
        "true",
        "false",
        "null",
        "undefined",
        "and",
        "or",
        "not",
        "with",
        "yield",
        "lambda",
        "elif",
        "then",
        "end",
        "begin",
        "val",
        "object",
        "override",
    ];
    KW.contains(&w)
}

/// The UTF-8 width of the character starting at byte `i` (which MUST be a char
/// boundary). Falls back to 1 defensively — callers always pass a boundary, so
/// the cursor below only ever lands on boundaries and can never split a char.
#[inline]
pub(crate) fn char_len_at(line: &str, i: usize) -> usize {
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
pub(crate) fn highlight_line(
    line: &str,
    line_comment: &str,
    blocks: bool,
    in_block: &mut bool,
) -> Vec<(String, Tok)> {
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
pub(crate) fn highlight(
    code: &str,
    lang: &str,
    s: &SyntaxColors,
    base: Color,
) -> Vec<Vec<(String, Color)>> {
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

/// Parse a fence info string into `(language, highlighted 1-based lines)`. A
/// `{2,4-6}` group anywhere marks highlighted lines; the language is the first
/// remaining whitespace-delimited token (so ` ```rust {2,4-6} ` works).
pub(crate) fn parse_fence_info(info: &str) -> (String, Vec<u32>) {
    let mut hl = Vec::new();
    let cleaned = match (info.find('{'), info.rfind('}')) {
        (Some(open), Some(close)) if close > open => {
            hl = parse_line_ranges(&info[open + 1..close]);
            format!("{}{}", &info[..open], &info[close + 1..])
        }
        _ => info.to_string(),
    };
    let lang = cleaned.split_whitespace().next().unwrap_or("").to_string();
    (lang, hl)
}

/// Parse `2,4-6` into `[2,4,5,6]` (ranges capped so a huge span can't blow up).
pub(crate) fn parse_line_ranges(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim().trim_start_matches('.').trim();
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                let (lo, hi) = (a.min(b), a.max(b));
                for n in lo..=hi.min(lo.saturating_add(10_000)) {
                    out.push(n);
                }
            }
        } else if let Ok(n) = part.parse::<u32>() {
            out.push(n);
        }
    }
    out
}
