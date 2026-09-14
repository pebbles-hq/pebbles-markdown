//! A small, dependency-free 2-D math layout engine for **display** math
//! (`$$…$$`). It parses a practical TeX subset into a box tree and lays it out
//! with Pebbles' flex widgets — real stacked fractions, roots, matrices, and
//! super/subscripts, rather than a single line of Unicode.
//!
//! It is not a full TeX typesetter (no OpenType MATH metrics, so spacing is
//! approximated with the flex model), but it renders the common constructs
//! correctly and legibly; atoms and symbols go through
//! [`crate::advanced::tex_to_unicode`], and anything it can't structure degrades
//! to that same Unicode transform. Panic-free on any input.

use pebbles_foundation::{Color, CrossAxisAlignment, EdgeInsets, MainAxisSize, TextAlign};
use pebbles_render::{BorderRadius, BoxDecoration};
use pebbles_widgets::{TableColumnWidth, column, container, layout_table, padding, row, text};

use pebbles_core::widget::{AnyWidget, IntoWidget};

use crate::advanced::tex_to_unicode;

/// The parsed math box tree.
enum Node {
    /// A leaf run of symbols (already TeX→Unicode transformed at render).
    Atom(String),
    /// A horizontal sequence.
    Row(Vec<Node>),
    /// `\frac{a}{b}`.
    Frac(Box<Node>, Box<Node>),
    /// `\sqrt{x}`.
    Sqrt(Box<Node>),
    /// `base` with optional superscript / subscript.
    Scripts {
        base: Box<Node>,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    /// A matrix: rows of cells.
    Matrix(Vec<Vec<Node>>),
}

/// Render a display-math TeX string to a widget.
pub(crate) fn render_display(tex: &str, size: f32, color: Color) -> AnyWidget {
    let node = parse(&mut Lexer::new(tex));
    render(&node, size, color)
}

// ---------------------------------------------------------------------------
// Lexer — whitespace-insensitive TeX tokens
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum Tok {
    Open,        // {
    Close,       // }
    Sup,         // ^
    Sub,         // _
    Amp,         // &  (matrix column sep)
    Rows,        // \\ (matrix row sep)
    Cmd(String), // \frac, \sqrt, \begin, \alpha, …
    Sym(String), // a run of ordinary characters
}

struct Lexer {
    toks: Vec<Tok>,
    pos: usize,
}

impl Lexer {
    fn new(src: &str) -> Self {
        Lexer {
            toks: tokenize(src),
            pos: 0,
        }
    }
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
}

fn tokenize(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Vec<Tok>| {
        if !run.is_empty() {
            out.push(Tok::Sym(std::mem::take(run)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                flush(&mut run, &mut out);
                out.push(Tok::Open);
            }
            '}' => {
                flush(&mut run, &mut out);
                out.push(Tok::Close);
            }
            '^' => {
                flush(&mut run, &mut out);
                out.push(Tok::Sup);
            }
            '_' => {
                flush(&mut run, &mut out);
                out.push(Tok::Sub);
            }
            '&' => {
                flush(&mut run, &mut out);
                out.push(Tok::Amp);
            }
            '\\' => {
                flush(&mut run, &mut out);
                if chars.peek() == Some(&'\\') {
                    chars.next();
                    out.push(Tok::Rows);
                } else {
                    let mut name = String::new();
                    if let Some(&n) = chars.peek()
                        && n.is_ascii_alphabetic()
                    {
                        while let Some(&n) = chars.peek() {
                            if n.is_ascii_alphabetic() {
                                name.push(n);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    } else if let Some(n) = chars.next() {
                        name.push(n); // escaped single char command, e.g. `\{`
                    }
                    out.push(Tok::Cmd(name));
                }
            }
            c if c.is_whitespace() => flush(&mut run, &mut out),
            c => run.push(c),
        }
    }
    flush(&mut run, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Parser — tokens → box tree
// ---------------------------------------------------------------------------

/// Parse a full sequence until end / a closing brace / a matrix terminator.
fn parse(lx: &mut Lexer) -> Node {
    let mut items: Vec<Node> = Vec::new();
    while let Some(t) = lx.peek() {
        match t {
            Tok::Close | Tok::Amp | Tok::Rows => break,
            Tok::Cmd(c) if c == "end" => break,
            Tok::Sup | Tok::Sub => {
                let is_sup = *t == Tok::Sup;
                lx.next();
                let script = parse_group(lx);
                let base = items.pop().unwrap_or(Node::Atom(String::new()));
                items.push(attach_script(base, script, is_sup));
            }
            _ => {
                if let Some(node) = parse_element(lx) {
                    items.push(node);
                }
            }
        }
    }
    if items.len() == 1 {
        items.pop().unwrap()
    } else {
        Node::Row(items)
    }
}

/// Attach a script to `base`, merging when a base already has one side.
fn attach_script(base: Node, script: Node, is_sup: bool) -> Node {
    match base {
        Node::Scripts { base, sup, sub } => {
            if is_sup {
                Node::Scripts {
                    base,
                    sup: Some(Box::new(script)),
                    sub,
                }
            } else {
                Node::Scripts {
                    base,
                    sup,
                    sub: Some(Box::new(script)),
                }
            }
        }
        other => {
            if is_sup {
                Node::Scripts {
                    base: Box::new(other),
                    sup: Some(Box::new(script)),
                    sub: None,
                }
            } else {
                Node::Scripts {
                    base: Box::new(other),
                    sup: None,
                    sub: Some(Box::new(script)),
                }
            }
        }
    }
}

/// Parse one element: a command construct, a `{group}`, or a run of atoms.
fn parse_element(lx: &mut Lexer) -> Option<Node> {
    match lx.peek()?.clone() {
        Tok::Open => Some(parse_group(lx)),
        Tok::Cmd(name) => {
            lx.next();
            match name.as_str() {
                "frac" | "dfrac" | "tfrac" => {
                    let a = parse_group(lx);
                    let b = parse_group(lx);
                    Some(Node::Frac(Box::new(a), Box::new(b)))
                }
                "sqrt" => Some(Node::Sqrt(Box::new(parse_group(lx)))),
                "begin" => {
                    // `\begin{env}` … `\end{env}` — treat any env as a matrix.
                    let _env = parse_group_raw(lx);
                    Some(parse_matrix(lx))
                }
                // Delimiters / spacing commands with no glyph of their own.
                "left" | "right" | "big" | "Big" | "bigg" | "Bigg" => {
                    // The following delimiter char (if any) is a normal atom.
                    parse_element(lx)
                }
                // Everything else → a symbol atom via the Unicode transform.
                _ => Some(Node::Atom(tex_to_unicode(&format!("\\{name}")))),
            }
        }
        Tok::Sym(s) => {
            lx.next();
            Some(Node::Atom(tex_to_unicode(&s)))
        }
        _ => None,
    }
}

/// Parse the next group: a braced `{...}` (recursed) or a single element.
fn parse_group(lx: &mut Lexer) -> Node {
    if lx.peek() == Some(&Tok::Open) {
        lx.next();
        let inner = parse(lx);
        if lx.peek() == Some(&Tok::Close) {
            lx.next();
        }
        inner
    } else {
        parse_element(lx).unwrap_or(Node::Atom(String::new()))
    }
}

/// Consume a `{word}` group as raw text (for `\begin{env}`), ignoring content.
fn parse_group_raw(lx: &mut Lexer) -> String {
    let mut s = String::new();
    if lx.peek() == Some(&Tok::Open) {
        lx.next();
        while let Some(t) = lx.next() {
            match t {
                Tok::Close => break,
                Tok::Sym(x) => s.push_str(&x),
                _ => {}
            }
        }
    }
    s
}

/// Parse matrix rows (cells split by `&`, rows by `\\`) until `\end{…}`.
fn parse_matrix(lx: &mut Lexer) -> Node {
    let mut rows: Vec<Vec<Node>> = vec![Vec::new()];
    loop {
        // A cell is a sequence up to `&`, `\\`, or `\end`.
        let cell = parse(lx);
        rows.last_mut().unwrap().push(cell);
        match lx.peek().cloned() {
            Some(Tok::Amp) => {
                lx.next();
            }
            Some(Tok::Rows) => {
                lx.next();
                rows.push(Vec::new());
            }
            Some(Tok::Cmd(c)) if c == "end" => {
                lx.next();
                let _ = parse_group_raw(lx);
                break;
            }
            None => break,
            _ => break,
        }
    }
    // Drop a trailing empty row (from a final `\\`).
    if rows.last().is_some_and(|r| r.iter().all(is_empty_node)) {
        rows.pop();
    }
    Node::Matrix(rows)
}

fn is_empty_node(n: &Node) -> bool {
    matches!(n, Node::Atom(s) if s.is_empty())
}

// ---------------------------------------------------------------------------
// Render — box tree → widgets
// ---------------------------------------------------------------------------

fn render(node: &Node, size: f32, color: Color) -> AnyWidget {
    match node {
        Node::Atom(s) => {
            let shown = if s.is_empty() { " " } else { s.as_str() };
            text(shown.to_string())
                .size(size)
                .color(color)
                .into_widget()
        }
        Node::Row(items) => {
            let kids: Vec<AnyWidget> = items.iter().map(|n| render(n, size, color)).collect();
            row(kids)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .main_axis_size(MainAxisSize::Min)
                .into_widget()
        }
        Node::Frac(a, b) => {
            // Numerator over a rule over denominator, both centered.
            let thick = (f64::from(size) * 0.06).max(1.0);
            let rule = container().height(thick).decoration(
                BoxDecoration::new()
                    .color(color)
                    .radius(BorderRadius::all(0.5)),
            );
            column(vec![
                render(a, size * 0.92, color),
                padding(EdgeInsets::symmetric(0.0, 2.0), rule).into_widget(),
                render(b, size * 0.92, color),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .main_axis_size(MainAxisSize::Min)
            .into_widget()
        }
        Node::Sqrt(inner) => {
            // A radical glyph beside the body under an overline (a thin top rule).
            let thick = (f64::from(size) * 0.06).max(1.0);
            let overline = container()
                .height(thick)
                .decoration(BoxDecoration::new().color(color));
            let body = column(vec![
                overline.into_widget(),
                padding(
                    EdgeInsets::only(0.0, 2.0, 0.0, 0.0),
                    render(inner, size, color),
                )
                .into_widget(),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .main_axis_size(MainAxisSize::Min);
            row(vec![
                text("√".to_string())
                    .size(size * 1.1)
                    .color(color)
                    .into_widget(),
                body.into_widget(),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .main_axis_size(MainAxisSize::Min)
            .into_widget()
        }
        Node::Scripts { base, sup, sub } => {
            let base_w = render(base, size, color);
            let script_size = size * 0.7;
            let scripts: AnyWidget = match (sup, sub) {
                (Some(up), Some(dn)) => column(vec![
                    render(up, script_size, color),
                    render(dn, script_size, color),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .main_axis_size(MainAxisSize::Min)
                .into_widget(),
                (Some(up), None) => render(up, script_size, color),
                (None, Some(dn)) => render(dn, script_size, color),
                (None, None) => return base_w,
            };
            // Superscript-only rides high, subscript-only rides low, both straddle.
            let align = match (sup.is_some(), sub.is_some()) {
                (true, false) => CrossAxisAlignment::Start,
                (false, true) => CrossAxisAlignment::End,
                _ => CrossAxisAlignment::Center,
            };
            row(vec![base_w, scripts])
                .cross_axis_alignment(align)
                .main_axis_size(MainAxisSize::Min)
                .into_widget()
        }
        Node::Matrix(rows) => {
            let ncols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let table_rows: Vec<Vec<AnyWidget>> = rows
                .iter()
                .map(|r| {
                    (0..ncols)
                        .map(|i| {
                            let cell = r.get(i);
                            let inner = match cell {
                                Some(n) => render(n, size, color),
                                None => text(" ".to_string()).size(size).color(color).into_widget(),
                            };
                            padding(EdgeInsets::symmetric(6.0, 2.0), inner).into_widget()
                        })
                        .collect()
                })
                .collect();
            let table =
                layout_table(table_rows).column_widths(vec![TableColumnWidth::Intrinsic; ncols]);
            // Flank with tall brackets.
            row(vec![
                bracket("[", size, color, rows.len()),
                table.into_widget(),
                bracket("]", size, color, rows.len()),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .main_axis_size(MainAxisSize::Min)
            .into_widget()
        }
    }
}

fn bracket(ch: &str, size: f32, color: Color, rows: usize) -> AnyWidget {
    text(ch.to_string())
        .size(size * (1.0 + 0.5 * rows.min(6) as f32))
        .color(color)
        .align(TextAlign::Center)
        .into_widget()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Vec<Tok> {
        tokenize(s)
    }

    #[test]
    fn tokenizes_commands_and_structure() {
        assert_eq!(t("a^2").len(), 3); // Sym(a) Sup Sym(2)
        assert!(matches!(t("\\frac").first(), Some(Tok::Cmd(c)) if c == "frac"));
        assert!(t("\\\\").contains(&Tok::Rows));
    }

    #[test]
    fn parses_without_panicking_on_any_input() {
        // A grab-bag of well-formed and malformed inputs; parsing must terminate.
        for s in [
            "\\frac{a}{b}",
            "x^2 + y_1",
            "\\sqrt{2}",
            "\\begin{matrix} a & b \\\\ c & d \\end{matrix}",
            "\\frac{1}{\\frac{1}{x}}",
            "\\left( \\frac{a}{b} \\right)",
            "e^{i\\pi}+1=0",
            "{{{unbalanced",
            "^^__&&\\\\",
            "\\frac{}{}",
        ] {
            let _ = parse(&mut Lexer::new(s));
        }
    }

    #[test]
    fn superscript_attaches_to_previous_atom() {
        let n = parse(&mut Lexer::new("x^2"));
        assert!(matches!(
            n,
            Node::Scripts {
                sup: Some(_),
                sub: None,
                ..
            }
        ));
    }

    #[test]
    fn frac_parses_numerator_and_denominator() {
        let n = parse(&mut Lexer::new("\\frac{a}{b}"));
        assert!(matches!(n, Node::Frac(_, _)));
    }

    #[test]
    fn matrix_parses_rows_and_cols() {
        let n = parse(&mut Lexer::new(
            "\\begin{matrix} a & b \\\\ c & d \\end{matrix}",
        ));
        let Node::Matrix(rows) = n else {
            panic!("matrix")
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
    }
}
