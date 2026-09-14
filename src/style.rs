//! The theming surface — every visual knob of the Markdown renderer plus the
//! syntax-highlight palette. Build one from the live theme (light/dark aware)
//! via [`MarkdownStyle::from_theme`], or construct it fully by hand.

use pebbles_foundation::{Color, palette};
use pebbles_widgets::theme::{mix, theme};

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
    /// Syntax-highlight palette for fenced code blocks. Every
    /// color is themeable; the default scheme reads on both light and dark code
    /// backgrounds.
    pub syntax: SyntaxColors,
    pub quote_bar: Color,
    pub quote_color: Color,
    pub link_color: Color,
    pub rule_color: Color,
    pub table_border: Color,
    /// `==highlight==` background.
    pub mark_bg: Color,
    /// `==highlight==` text color.
    pub mark_fg: Color,
    /// Obsidian `#tag` chip background.
    pub tag_bg: Color,
    /// Obsidian `#tag` chip text color.
    pub tag_fg: Color,
    /// Find-in-document match background.
    pub find_bg: Color,
    /// Find-in-document match text color.
    pub find_fg: Color,
    /// Show a line-number gutter in fenced code blocks.
    pub code_line_numbers: bool,
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
            mark_bg: mix(c.background, palette::amber::S400, 0.55),
            mark_fg: c.foreground,
            tag_bg: mix(c.background, c.primary, 0.16),
            tag_fg: c.primary,
            find_bg: palette::amber::S400,
            find_fg: palette::neutral::S950,
            code_line_numbers: false,
            block_gap: 10.0,
        }
    }
}

// ---------------------------------------------------------------------------
