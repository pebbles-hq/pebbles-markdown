//! Small, dependency-free, **panic-free** helpers for the extended/advanced
//! markup layer: emoji shortcodes, a heuristic TeX→Unicode transform for math,
//! and GitHub-style heading slugs. Each operates on whole characters and always
//! terminates, so any input is safe.

/// Replace `:shortcode:` emoji with their Unicode character. Unknown codes are
/// left verbatim (including the surrounding colons).
pub(crate) fn replace_emoji(s: &str) -> String {
    if !s.contains(':') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find(':') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // A shortcode is a short run of `[a-z0-9_+-]` between colons.
        if let Some(close) = after.find(':').filter(|&c| c <= 32)
            && after[..close]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
            && !after[..close].is_empty()
            && let Some(emoji) = emoji_for(&after[..close])
        {
            out.push_str(emoji);
            rest = &after[close + 1..];
            continue;
        }
        out.push(':');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The emoji for a shortcode name (without colons), or `None`.
fn emoji_for(name: &str) -> Option<&'static str> {
    let e = match name {
        "smile" | "smiley" => "😄",
        "grin" => "😁",
        "laughing" | "satisfied" => "😆",
        "joy" => "😂",
        "rofl" => "🤣",
        "wink" => "😉",
        "blush" => "😊",
        "heart_eyes" => "😍",
        "thinking" => "🤔",
        "neutral_face" => "😐",
        "unamused" => "😒",
        "cry" => "😢",
        "sob" => "😭",
        "rage" | "angry" => "😠",
        "sunglasses" => "😎",
        "sweat_smile" => "😅",
        "scream" => "😱",
        "tada" => "🎉",
        "rocket" => "🚀",
        "fire" => "🔥",
        "star" => "⭐",
        "sparkles" => "✨",
        "zap" => "⚡",
        "boom" => "💥",
        "100" => "💯",
        "heart" => "❤️",
        "broken_heart" => "💔",
        "thumbsup" | "+1" => "👍",
        "thumbsdown" | "-1" => "👎",
        "ok_hand" => "👌",
        "clap" => "👏",
        "wave" => "👋",
        "pray" => "🙏",
        "muscle" => "💪",
        "eyes" => "👀",
        "point_right" => "👉",
        "point_left" => "👈",
        "warning" => "⚠️",
        "white_check_mark" => "✅",
        "heavy_check_mark" => "✔️",
        "x" => "❌",
        "question" => "❓",
        "exclamation" => "❗",
        "bulb" => "💡",
        "book" => "📖",
        "books" => "📚",
        "memo" | "pencil" => "📝",
        "bug" => "🐛",
        "wrench" => "🔧",
        "hammer" => "🔨",
        "gear" => "⚙️",
        "lock" => "🔒",
        "key" => "🔑",
        "mag" => "🔍",
        "link" => "🔗",
        "package" => "📦",
        "clipboard" => "📋",
        "calendar" => "📅",
        "chart_with_upwards_trend" => "📈",
        "hourglass" => "⌛",
        "clock" => "🕐",
        "bell" => "🔔",
        "email" | "envelope" => "✉️",
        "phone" => "📞",
        "computer" => "💻",
        "coffee" => "☕",
        "beer" => "🍺",
        "pizza" => "🍕",
        "apple" => "🍎",
        "sun" | "sunny" => "☀️",
        "moon" => "🌙",
        "cloud" => "☁️",
        "snowflake" => "❄️",
        "rainbow" => "🌈",
        "earth_americas" | "globe" => "🌎",
        "dog" => "🐶",
        "cat" => "🐱",
        "penguin" => "🐧",
        "ghost" => "👻",
        "robot" => "🤖",
        "alien" => "👽",
        "poop" | "hankey" => "💩",
        "crown" => "👑",
        "gift" => "🎁",
        "balloon" => "🎈",
        "trophy" => "🏆",
        "medal" => "🏅",
        "dart" => "🎯",
        "checkered_flag" => "🏁",
        "green_heart" => "💚",
        "blue_heart" => "💙",
        "yellow_heart" => "💛",
        "purple_heart" => "💜",
        _ => return None,
    };
    Some(e)
}

/// A best-effort TeX→Unicode transform for inline/display math: Greek letters,
/// common operators/relations, digit super/subscripts (`x^2`, `a_1`), and simple
/// `\frac{a}{b}` → `a⁄b`. Anything it doesn't know degrades to readable text
/// (backslash stripped, braces removed) rather than raw TeX. It is NOT a
/// typesetter — fractions, matrices, and integrals are approximated.
pub(crate) fn tex_to_unicode(tex: &str) -> String {
    let mut s = tex.trim().to_string();
    // `\frac{a}{b}` → `a⁄b` (repeatedly, innermost-first via a simple scan).
    while let Some(pos) = s.find("\\frac{") {
        if let Some((a, b, end)) = parse_two_groups(&s[pos + 5..]) {
            let repl = format!("{a}⁄{b}");
            s.replace_range(pos..pos + 5 + end, &repl);
        } else {
            break;
        }
    }
    // `\sqrt{x}` → `√(x)`.
    while let Some(pos) = s.find("\\sqrt{") {
        if let Some((g, end)) = parse_group(&s[pos + 5..]) {
            let repl = format!("√({g})");
            s.replace_range(pos..pos + 5 + end, &repl);
        } else {
            break;
        }
    }
    // Named symbols (longest names first so `\subseteq` beats `\subset`).
    for (name, sym) in SYMBOLS {
        if s.contains(name) {
            s = s.replace(name, sym);
        }
    }
    // Super/subscripts: `^{...}`/`^x` and `_{...}`/`_x`.
    s = scripts(&s, '^', super_char);
    s = scripts(&s, '_', sub_char);
    // Strip any leftover braces and stray backslashes.
    s.retain(|c| c != '{' && c != '}');
    s.replace('\\', "")
}

/// Parse a single `{...}` group at the start of `s`, returning (inner, bytes-consumed).
fn parse_group(s: &str) -> Option<(String, usize)> {
    let s = s.strip_prefix('{')?;
    let mut depth = 1;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((s[..i].to_string(), i + 2));
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse two consecutive `{a}{b}` groups, returning (a, b, total-bytes-consumed).
fn parse_two_groups(s: &str) -> Option<(String, String, usize)> {
    let (a, used_a) = parse_group(s)?;
    let (b, used_b) = parse_group(&s[used_a..])?;
    Some((a, b, used_a + used_b))
}

/// Replace `marker{group}` / `marker<char>` with the mapped super/subscript form,
/// falling back to `marker<text>` when a character has no Unicode form.
fn scripts(s: &str, marker: char, map: fn(char) -> Option<char>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != marker {
            out.push(c);
            continue;
        }
        // Grab the operand: a `{...}` group or a single following char.
        let operand = if let Some((_, '{')) = chars.peek().copied() {
            if let Some((grp, used)) = parse_group(&s[i + c.len_utf8()..]) {
                for _ in 0..grp.chars().count() + 2 {
                    chars.next();
                }
                let _ = used;
                grp
            } else {
                String::new()
            }
        } else if let Some((_, ch)) = chars.next() {
            ch.to_string()
        } else {
            String::new()
        };
        // Map every char; if all map, emit the script glyphs, else keep readable.
        let mapped: Option<String> = operand.chars().map(map).collect();
        match mapped {
            Some(m) if !m.is_empty() => out.push_str(&m),
            _ => {
                out.push(if marker == '^' { '^' } else { '_' });
                out.push_str(&operand);
            }
        }
    }
    out
}

fn super_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        _ => return None,
    })
}

fn sub_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'o' => 'ₒ',
        'x' => 'ₓ',
        'n' => 'ₙ',
        _ => return None,
    })
}

/// TeX command → Unicode symbol. Longer names must precede their prefixes.
const SYMBOLS: &[(&str, &str)] = &[
    ("\\alpha", "α"),
    ("\\beta", "β"),
    ("\\gamma", "γ"),
    ("\\delta", "δ"),
    ("\\epsilon", "ε"),
    ("\\varepsilon", "ε"),
    ("\\zeta", "ζ"),
    ("\\eta", "η"),
    ("\\theta", "θ"),
    ("\\iota", "ι"),
    ("\\kappa", "κ"),
    ("\\lambda", "λ"),
    ("\\mu", "μ"),
    ("\\nu", "ν"),
    ("\\xi", "ξ"),
    ("\\pi", "π"),
    ("\\rho", "ρ"),
    ("\\sigma", "σ"),
    ("\\tau", "τ"),
    ("\\upsilon", "υ"),
    ("\\phi", "φ"),
    ("\\varphi", "φ"),
    ("\\chi", "χ"),
    ("\\psi", "ψ"),
    ("\\omega", "ω"),
    ("\\Gamma", "Γ"),
    ("\\Delta", "Δ"),
    ("\\Theta", "Θ"),
    ("\\Lambda", "Λ"),
    ("\\Xi", "Ξ"),
    ("\\Pi", "Π"),
    ("\\Sigma", "Σ"),
    ("\\Phi", "Φ"),
    ("\\Psi", "Ψ"),
    ("\\Omega", "Ω"),
    ("\\times", "×"),
    ("\\div", "÷"),
    ("\\pm", "±"),
    ("\\mp", "∓"),
    ("\\cdot", "·"),
    ("\\ast", "∗"),
    ("\\star", "⋆"),
    ("\\leq", "≤"),
    ("\\geq", "≥"),
    ("\\neq", "≠"),
    ("\\approx", "≈"),
    ("\\equiv", "≡"),
    ("\\cong", "≅"),
    ("\\sim", "∼"),
    ("\\propto", "∝"),
    ("\\infty", "∞"),
    ("\\partial", "∂"),
    ("\\nabla", "∇"),
    ("\\sum", "∑"),
    ("\\prod", "∏"),
    ("\\int", "∫"),
    ("\\oint", "∮"),
    ("\\forall", "∀"),
    ("\\exists", "∃"),
    ("\\nexists", "∄"),
    ("\\emptyset", "∅"),
    ("\\varnothing", "∅"),
    ("\\notin", "∉"),
    ("\\in", "∈"),
    ("\\ni", "∋"),
    ("\\subseteq", "⊆"),
    ("\\subset", "⊂"),
    ("\\supseteq", "⊇"),
    ("\\supset", "⊃"),
    ("\\cup", "∪"),
    ("\\cap", "∩"),
    ("\\setminus", "∖"),
    ("\\land", "∧"),
    ("\\wedge", "∧"),
    ("\\lor", "∨"),
    ("\\vee", "∨"),
    ("\\neg", "¬"),
    ("\\oplus", "⊕"),
    ("\\otimes", "⊗"),
    ("\\Rightarrow", "⇒"),
    ("\\Leftarrow", "⇐"),
    ("\\Leftrightarrow", "⇔"),
    ("\\rightarrow", "→"),
    ("\\leftarrow", "←"),
    ("\\leftrightarrow", "↔"),
    ("\\mapsto", "↦"),
    ("\\to", "→"),
    ("\\gets", "←"),
    ("\\uparrow", "↑"),
    ("\\downarrow", "↓"),
    ("\\ldots", "…"),
    ("\\cdots", "⋯"),
    ("\\vdots", "⋮"),
    ("\\ddots", "⋱"),
    ("\\sqrt", "√"),
    ("\\angle", "∠"),
    ("\\perp", "⊥"),
    ("\\parallel", "∥"),
    ("\\degree", "°"),
    ("\\prime", "′"),
    ("\\hbar", "ℏ"),
    ("\\ell", "ℓ"),
    ("\\Re", "ℜ"),
    ("\\Im", "ℑ"),
    ("\\aleph", "ℵ"),
    ("\\quad", "  "),
    ("\\qquad", "    "),
    ("\\,", " "),
    ("\\;", " "),
    ("\\!", ""),
    ("\\left", ""),
    ("\\right", ""),
];

/// A GitHub-style heading slug: lowercase, alphanumerics kept, spaces/`-`/`_`
/// become `-`, other punctuation dropped, runs of `-` collapsed and trimmed.
pub(crate) fn slugify(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() {
            s.extend(c.to_lowercase());
        } else if (c == ' ' || c == '-' || c == '_' || c == '\t') && !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_replaces_known_leaves_unknown() {
        assert_eq!(replace_emoji("hi :smile: :rocket:"), "hi 😄 🚀");
        assert_eq!(replace_emoji("ratio 3:4 ok"), "ratio 3:4 ok");
        assert_eq!(replace_emoji(":not_an_emoji:"), ":not_an_emoji:");
        assert_eq!(replace_emoji("plain"), "plain");
    }

    #[test]
    fn tex_transforms_common_math() {
        assert_eq!(tex_to_unicode("\\alpha + \\beta"), "α + β");
        assert_eq!(tex_to_unicode("x^2 + y_1"), "x² + y₁");
        assert_eq!(tex_to_unicode("a \\leq b \\times c"), "a ≤ b × c");
        assert_eq!(tex_to_unicode("\\frac{a}{b}"), "a⁄b");
        assert_eq!(tex_to_unicode("\\sqrt{2}"), "√(2)");
        assert_eq!(tex_to_unicode("E = mc^2"), "E = mc²");
        // Unknown command degrades (backslash stripped), never panics.
        assert_eq!(tex_to_unicode("\\unknowncmd x"), "unknowncmd x");
    }

    #[test]
    fn slugify_matches_github_style() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("A  B, C!"), "a-b-c");
        assert_eq!(slugify("  Trim -- Me  "), "trim-me");
        assert_eq!(slugify("Café déjà"), "café-déjà");
    }
}
