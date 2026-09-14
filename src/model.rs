//! The parsed document model — the block/inline tree the parser produces and
//! the renderer consumes.

#[derive(Clone, PartialEq)]
pub(crate) struct Run {
    pub(crate) text: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strike: bool,
    pub(crate) code: bool,
    /// `==highlight==`.
    pub(crate) mark: bool,
    /// `~subscript~`.
    pub(crate) sub: bool,
    /// `^superscript^`.
    pub(crate) sup: bool,
    /// Obsidian `#tag` — rendered as a chip.
    pub(crate) tag: bool,
    pub(crate) link: Option<String>,
}

/// The active inline formatting when a run of text is emitted (built from the
/// parser's depth counters). Kept as one value to avoid many boolean arguments.
#[derive(Clone, Copy, Default)]
pub(crate) struct Flags {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strike: bool,
    pub(crate) code: bool,
    pub(crate) sub: bool,
    pub(crate) sup: bool,
    pub(crate) mark: bool,
    pub(crate) tag: bool,
}

impl Flags {
    pub(crate) fn from_counts(
        bold: u32,
        italic: u32,
        strike: u32,
        code: u32,
        sub: u32,
        sup: u32,
    ) -> Self {
        Flags {
            bold: bold > 0,
            italic: italic > 0,
            strike: strike > 0,
            code: code > 0,
            sub: sub > 0,
            sup: sup > 0,
            mark: false,
            tag: false,
        }
    }
}

/// Build a [`Run`] from `text`, the active [`Flags`], and an optional link.
pub(crate) fn run_of(text: String, f: Flags, link: Option<String>) -> Run {
    Run {
        text,
        bold: f.bold,
        italic: f.italic,
        strike: f.strike,
        code: f.code,
        mark: f.mark,
        sub: f.sub,
        sup: f.sup,
        tag: f.tag,
        link,
    }
}

#[derive(Clone, PartialEq)]
pub(crate) enum Inline {
    Run(Run),
    Image {
        url: String,
        alt: String,
        /// `![[…]]` wikilink embed (vs a normal `![alt](url)` image).
        embed: bool,
    },
    /// A footnote reference, rendered as a superscript marker (its 1-based number).
    Footnote(usize),
    /// Math (`$…$` inline, `$$…$$` display), rendered via a TeX→Unicode transform.
    Math {
        tex: String,
        display: bool,
    },
}

/// A GitHub/Obsidian callout (admonition) kind, from `> [!NOTE]` etc.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

/// A table column's horizontal alignment (from the GFM `:---:` header row).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum CellAlign {
    Start,
    Center,
    End,
}

#[derive(Clone, PartialEq)]
pub(crate) struct ListItem {
    /// `Some(checked)` for GFM task items; the usize is the document-order
    /// task ordinal (the handle [`toggle_task`] flips).
    pub(crate) task: Option<(bool, usize)>,
    pub(crate) blocks: Vec<Block>,
}

#[derive(Clone, PartialEq)]
pub(crate) enum Block {
    /// Document front matter (`---`/`+++` fenced) parsed to `(key, value)` pairs.
    FrontMatter(Vec<(String, String)>),
    /// `(level, inlines, slug)` — the slug is a GitHub-style anchor id.
    Heading(u8, Vec<Inline>, String),
    Para(Vec<Inline>),
    Code {
        lang: String,
        text: String,
    },
    Quote(Vec<Block>),
    /// A GFM/Obsidian callout (`> [!NOTE]` …).
    Callout {
        kind: CalloutKind,
        body: Vec<Block>,
    },
    /// A definition list: each entry is `(term inlines, definition blocks)`.
    DefList(Vec<(Vec<Inline>, Vec<Block>)>),
    /// A collapsible `<details>` disclosure: summary + body, initially `open`.
    Details {
        summary: Vec<Inline>,
        open: bool,
        body: Vec<Block>,
    },
    /// A generated table of contents: `(level, title, slug)` per heading.
    Toc(Vec<(u8, String, String)>),
    /// `tight` (no blank lines between items) renders compact; a loose list gets
    /// block-level spacing between items.
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
        tight: bool,
    },
    Rule,
    Table {
        header: Vec<Vec<Inline>>,
        /// Per-column alignment (may be shorter than the widest row).
        aligns: Vec<CellAlign>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// The collected footnote definitions, rendered as a section at the document
    /// end. Each entry is `(1-based number, body blocks)`.
    Footnotes(Vec<(usize, Vec<Block>)>),
    /// A host-supplied backlinks section: `(label, target)` clickable entries.
    Backlinks(Vec<(String, String)>),
}

/// A table accumulated during parsing: `(header cells, body rows)`, each cell a
/// run of inlines. Aliased to keep `parse_blocks`' local readable + clippy-clean.
pub(crate) type TableAcc = (Vec<Vec<Inline>>, Vec<Vec<Vec<Inline>>>);
