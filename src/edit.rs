//! Source-level transforms — the reusable core of a formatting toolbar /
//! keyboard shortcuts. Pure string operations over the Markdown source:
//! task-checkbox toggling, selection wrapping, and line-prefix toggling.

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
        let after_marker = if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            Some((trimmed.len() - rest.len(), rest))
        } else {
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            if digits > 0
                && let Some(sep) = trimmed[digits..]
                    .strip_prefix('.')
                    .or_else(|| trimmed[digits..].strip_prefix(')'))
                && let Some(rest) = sep.strip_prefix(' ')
            {
                Some((trimmed.len() - rest.len(), rest))
            } else {
                None
            }
        };
        let Some((prefix_len, rest)) = after_marker else {
            continue;
        };
        let checked = if rest.starts_with("[ ] ") || rest == "[ ]" {
            Some(false)
        } else if rest.starts_with("[x] ")
            || rest.starts_with("[X] ")
            || rest == "[x]"
            || rest == "[X]"
        {
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
// Formatting helpers — the reusable core of a toolbar / keyboard shortcuts.
//
// These are pure source transforms (like `toggle_task`): a selection-aware edit
// pane (e.g. `pebbles-code-editor`, which exposes the cursor/selection) supplies
// the byte range, applies the returned source, and restores the returned range.
// The bundled `text_area` edit pane does not expose a selection, so wiring live
// shortcuts to it is a host concern.
// ---------------------------------------------------------------------------

/// Toggle an inline wrapper (`**` bold, `*` italic, `~~` strike, `` ` `` code)
/// around the `[start, end)` byte range: wraps if absent, unwraps if the range is
/// already surrounded by `marker`. Returns `(new_source, new_start, new_end)`.
/// Out-of-range / non-char-boundary inputs are returned unchanged.
pub fn wrap_selection(
    source: &str,
    start: usize,
    end: usize,
    marker: &str,
) -> (String, usize, usize) {
    let start = start.min(source.len());
    let end = end.min(source.len());
    let (start, end) = (start.min(end), start.max(end));
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) || marker.is_empty() {
        return (source.to_string(), start, end);
    }
    let m = marker;
    let before = start >= m.len()
        && source.is_char_boundary(start - m.len())
        && &source[start - m.len()..start] == m;
    let after = end + m.len() <= source.len()
        && source.is_char_boundary(end + m.len())
        && &source[end..end + m.len()] == m;
    if before && after {
        // Unwrap: drop the surrounding markers.
        let mut s = String::with_capacity(source.len() - 2 * m.len());
        s.push_str(&source[..start - m.len()]);
        s.push_str(&source[start..end]);
        s.push_str(&source[end + m.len()..]);
        return (s, start - m.len(), end - m.len());
    }
    // Wrap.
    let mut s = String::with_capacity(source.len() + 2 * m.len());
    s.push_str(&source[..start]);
    s.push_str(m);
    s.push_str(&source[start..end]);
    s.push_str(m);
    s.push_str(&source[end..]);
    (s, start + m.len(), end + m.len())
}

/// Toggle a line-start `prefix` (`"> "` quote, `"- "` bullet, `"# "` heading …)
/// on every line the `[start, end)` byte range touches: removes it if the first
/// touched line already has it, otherwise adds it to each. Returns
/// `(new_source, new_start, new_end)` with the selection shifted to match.
pub fn toggle_line_prefix(
    source: &str,
    start: usize,
    end: usize,
    prefix: &str,
) -> (String, usize, usize) {
    let start = start.min(source.len());
    let end = end.min(source.len()).max(start);
    if prefix.is_empty() {
        return (source.to_string(), start, end);
    }
    // Expand the range to whole lines.
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[end..].find('\n').map_or(source.len(), |i| end + i);
    let region = &source[line_start..line_end];
    let adding = !region.lines().next().is_some_and(|l| l.starts_with(prefix));

    let mut rebuilt = String::new();
    for (i, line) in region.split('\n').enumerate() {
        if i > 0 {
            rebuilt.push('\n');
        }
        if adding {
            rebuilt.push_str(prefix);
            rebuilt.push_str(line);
        } else {
            rebuilt.push_str(line.strip_prefix(prefix).unwrap_or(line));
        }
    }
    let mut s = String::with_capacity(source.len() + rebuilt.len());
    s.push_str(&source[..line_start]);
    s.push_str(&rebuilt);
    s.push_str(&source[line_end..]);

    // Shift the selection by the change applied to the first touched line.
    let delta: isize = if adding {
        prefix.len() as isize
    } else {
        -(prefix.len() as isize)
    };
    let shift = |p: usize| -> usize {
        if p >= line_start {
            (p as isize + delta).max(line_start as isize) as usize
        } else {
            p
        }
    };
    (s, shift(start), shift(end))
}

// ---------------------------------------------------------------------------
