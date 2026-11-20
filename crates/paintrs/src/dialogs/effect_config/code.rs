//! A plug-in window's code editor (`Control::PluginCode`: BoltBait's CodeLab script box), live: the window stays open
//! in the host while this dialog is (`dotnet::Canvas`, as for a canvas), paint.rs edits its text with its own C#
//! highlighting, and each change reaches the window as it happens: the whole text with the selection, each typed
//! character (CodeLab builds and suggests on it), or a moved selection alone. The window's own changes to its text
//! (indent, snippets), its errors and its read-only state come back with its settings after each.
use super::EffectConfigDialog;
use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{Color32, Event, FontId, Pos2, Rect, Stroke, TextBuffer, Ui, UiBuilder, Vec2};
use pdn_core::effect::dotnet::TOKEN;
use serde_json::{Value, json};
use std::ops::Range;

/// The editor's font size (points).
const CODE_FONT_SIZE: f32 = 13.0;

/// What one code editor holds between frames.
#[derive(Default)]
pub(super) struct CodeEdit {
    text: String,
    /// The window's text as last read, so a change the window makes itself reaches the editor.
    seen: Option<String>,
    /// The selection last sent, in UTF-16 code units.
    sel: Option<(usize, usize)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Plain,
    Keyword,
    Type,
    Str,
    Number,
    Comment,
    Preproc,
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "add",
    "as",
    "async",
    "await",
    "base",
    "break",
    "case",
    "catch",
    "checked",
    "class",
    "const",
    "continue",
    "default",
    "delegate",
    "do",
    "dynamic",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "for",
    "foreach",
    "get",
    "global",
    "goto",
    "if",
    "implicit",
    "in",
    "init",
    "interface",
    "internal",
    "is",
    "lock",
    "nameof",
    "namespace",
    "new",
    "null",
    "operator",
    "out",
    "override",
    "params",
    "partial",
    "private",
    "protected",
    "public",
    "readonly",
    "record",
    "ref",
    "remove",
    "return",
    "sealed",
    "set",
    "sizeof",
    "stackalloc",
    "static",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "unchecked",
    "unsafe",
    "using",
    "value",
    "var",
    "virtual",
    "volatile",
    "when",
    "where",
    "while",
    "with",
    "yield",
];
const TYPES: &[&str] = &[
    "bool", "byte", "char", "decimal", "double", "float", "int", "long", "nint", "nuint", "object",
    "sbyte", "short", "string", "uint", "ulong", "ushort", "void",
];

/// C#'s tokens in `text`, as byte ranges with their kind (whitespace and punctuation are `Plain`).
fn tokens(text: &str) -> Vec<(Range<usize>, Kind)> {
    let b = text.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    // The end of a quoted literal from `i` (its opening quote at `q`): `verbatim` doubles quotes, else `\` escapes and
    // a line end stops it.
    let quoted = |q: usize, verbatim: bool| -> usize {
        let mut j = q + 1;
        while j < n {
            match b[j] {
                b'"' if verbatim && j + 1 < n && b[j + 1] == b'"' => j += 2,
                c if c == b[q] => return j + 1,
                b'\\' if !verbatim => j += 2,
                b'\n' if !verbatim => return j,
                _ => j += 1,
            }
        }
        n
    };
    while i < n {
        let c = b[i];
        let start = i;
        let kind = if c == b'/' && b.get(i + 1) == Some(&b'/') {
            i = text[i..].find('\n').map_or(n, |e| i + e);
            Kind::Comment
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i = text[i + 2..].find("*/").map_or(n, |e| i + 2 + e + 2);
            Kind::Comment
        } else if c == b'#' && line_start {
            i = text[i..].find('\n').map_or(n, |e| i + e);
            Kind::Preproc
        } else if c == b'"' || c == b'\'' {
            i = quoted(i, false);
            Kind::Str
        } else if (c == b'@' || c == b'$') && matches!(b.get(i + 1), Some(b'"' | b'@' | b'$')) {
            // @"…", $"…", $@"…", @$"…": a prefix, then the literal.
            let mut q = i + 1;
            while q < n && matches!(b[q], b'@' | b'$') {
                q += 1;
            }
            let verbatim = b[i..q].contains(&b'@');
            i = if q < n && b[q] == b'"' {
                quoted(q, verbatim)
            } else {
                q
            };
            Kind::Str
        } else if c.is_ascii_digit() {
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                // A member access after a number ("1.ToString") ends it.
                if b[i] == b'.' && !b.get(i + 1).is_some_and(u8::is_ascii_digit) {
                    break;
                }
                i += 1;
            }
            Kind::Number
        } else if c.is_ascii_alphabetic() || c == b'_' || c == b'@' || c >= 0x80 {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] >= 0x80) {
                i += text[i..].chars().next().map_or(1, char::len_utf8);
            }
            let word = text[start..i].trim_start_matches('@');
            if KEYWORDS.contains(&word) {
                Kind::Keyword
            } else if TYPES.contains(&word) {
                Kind::Type
            } else {
                Kind::Plain
            }
        } else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            Kind::Plain
        };
        if c == b'\n' {
            line_start = true;
        } else if !c.is_ascii_whitespace() {
            line_start = false;
        }
        out.push((start..i, kind));
    }
    out
}

/// `text` in the editor's font, coloured by its C# tokens.
fn highlight(text: &str, dark: bool, font: &FontId) -> LayoutJob {
    let colour = |k: Kind| {
        let (d, l) = match k {
            Kind::Plain => ((220, 220, 220), (0, 0, 0)),
            Kind::Keyword => ((86, 156, 214), (0, 0, 255)),
            Kind::Type => ((78, 201, 176), (43, 145, 175)),
            Kind::Str => ((214, 157, 133), (163, 21, 21)),
            Kind::Number => ((181, 206, 168), (9, 134, 88)),
            Kind::Comment => ((87, 166, 74), (0, 128, 0)),
            Kind::Preproc => ((155, 155, 155), (128, 128, 128)),
        };
        let (r, g, b) = if dark { d } else { l };
        Color32::from_rgb(r, g, b)
    };
    let mut job = LayoutJob::default();
    for (range, kind) in tokens(text) {
        job.append(
            &text[range],
            0.0,
            TextFormat::simple(font.clone(), colour(kind)),
        );
    }
    job
}

/// UTF-16 code units in the first `chars` characters of `text` (.NET's string positions).
fn utf16_at(text: &str, chars: usize) -> usize {
    text.chars().take(chars).map(char::len_utf16).sum()
}

/// The character index of UTF-16 position `units` in `text`.
fn char_at_utf16(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.chars().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    text.chars().count()
}

/// The window's errors in its code value: (line, column, length, message), lines and columns from 0.
fn errors(v: &Value) -> Vec<(usize, usize, usize, String)> {
    let at = |e: &Value, i: usize| e.get(i).and_then(Value::as_u64).unwrap_or(0) as usize;
    v.get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|e| {
            let msg = e.get(3).and_then(Value::as_str).unwrap_or("").to_owned();
            (at(e, 0), at(e, 1), at(e, 2), msg)
        })
        .collect()
}

/// The events an edit sends: the whole text with the selection after a change, then each typed character; or the
/// selection alone when only it moved.
fn edit_events(
    changed: bool,
    text: &str,
    sel: Option<(usize, usize)>,
    last_sel: Option<(usize, usize)>,
    typed: &str,
) -> Vec<Value> {
    let sel_json = |s: (usize, usize)| json!([s.0, s.1]);
    let mut out = Vec::new();
    if changed {
        let s = sel.unwrap_or((0, 0));
        out.push(json!({"e": "text", "text": text, "sel": sel_json(s)}));
        out.extend(
            typed
                .chars()
                .map(|c| json!({"e": "char", "c": c.to_string()})),
        );
    } else if let Some(s) = sel.filter(|s| Some(*s) != last_sel) {
        out.push(json!({"e": "sel", "sel": sel_json(s)}));
    }
    out
}

impl EffectConfigDialog {
    /// The code editor `key` of the open window, in `area`.
    pub(super) fn plugin_code(
        &mut self,
        ui: &mut Ui,
        key: &'static str,
        area: Rect,
    ) -> Option<bool> {
        let value = self.values.get(key).cloned().unwrap_or(Value::Null);
        let settling = self.settling();
        let host_text = value.get("text").and_then(Value::as_str).map(str::to_owned);
        let read_only = value
            .get("readOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let errors = errors(&value);
        let id = ui.make_persistent_id(("code", key));
        let edit = self.code_edits.entry(key).or_default();
        // The window's own change to its text (an indent, a snippet) with the selection it left.
        if host_text.is_some() && host_text != edit.seen {
            edit.text = host_text.clone().unwrap_or_default();
            edit.seen = host_text;
            let at = |i: usize| value.get("sel")?.get(i)?.as_u64().map(|u| u as usize);
            if let (Some(a), Some(b)) = (at(0), at(1)) {
                let cursor = |u| CCursor::new(char_at_utf16(&edit.text, u));
                let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(cursor(a), cursor(b))));
                state.store(ui.ctx(), id);
                edit.sel = Some((a, b));
            }
        }
        let dark = ui.visuals().dark_mode;
        let font = FontId::monospace(CODE_FONT_SIZE);
        let lines = edit.text.split('\n').count().max(1);
        let digit = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
        let gutter = (lines.to_string().len().max(3) as f32) * digit + 12.0;
        let bg = if dark {
            Color32::from_rgb(30, 30, 30)
        } else {
            Color32::WHITE
        };
        ui.painter().rect_filled(area, 0.0, bg);
        let editor = Rect::from_min_max(area.min + Vec2::new(gutter, 0.0), area.max);
        // Characters typed into the editor this frame: CodeLab's CharAdded, which Scintilla also raises with '\n'
        // for Enter (CodeLab indents the new line on it).
        let focused = ui.memory(|m| m.has_focus(id));
        let typed: String = if focused && !read_only {
            ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        Event::Text(t) => Some(t.as_str()),
                        Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            modifiers,
                            ..
                        } if modifiers.is_none() || *modifiers == egui::Modifiers::SHIFT => {
                            Some("\n")
                        }
                        _ => None,
                    })
                    .collect()
            })
        } else {
            String::new()
        };
        let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, _wrap: f32| {
            ui.fonts_mut(|f| f.layout_job(highlight(buf.as_str(), dark, &font)))
        };
        let mut child = ui.new_child(UiBuilder::new().max_rect(editor));
        let shown = egui::ScrollArea::both()
            .id_salt(("code-scroll", key))
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                egui::TextEdit::multiline(&mut edit.text)
                    .id(id)
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .interactive(!read_only)
                    .layouter(&mut layouter)
                    .show(ui)
            })
            .inner;
        // Enter in the focused editor is a new line, not the dialog's OK.
        crate::kit::inputs::take_enter(ui, &shown.response);
        // A settle that may take a while is next (CodeLab compiles its script): say so, over the editor.
        if settling {
            let text = if dark {
                Color32::from_gray(220)
            } else {
                Color32::from_gray(30)
            };
            let painter = ui.painter_at(area);
            let at = area.right_top() + Vec2::new(-8.0, 6.0);
            let galley =
                painter.layout_no_wrap("Working…".into(), FontId::proportional(13.0), text);
            let bg = Rect::from_min_size(
                at - Vec2::new(galley.size().x + 8.0, 2.0),
                galley.size() + Vec2::new(8.0, 4.0),
            );
            painter.rect_filled(
                bg,
                3.0,
                if dark {
                    Color32::from_black_alpha(180)
                } else {
                    Color32::from_white_alpha(200)
                },
            );
            painter.galley(bg.min + Vec2::new(4.0, 2.0), galley, text);
        }
        // The line numbers, beside each line's first row.
        let painter = ui.painter_at(area);
        let number = if dark {
            Color32::from_gray(120)
        } else {
            Color32::from_gray(140)
        };
        let mut line = 1;
        let mut starts = true;
        for row in &shown.galley.rows {
            if starts {
                let y = shown.galley_pos.y + row.pos.y;
                painter.text(
                    Pos2::new(area.min.x + gutter - 6.0, y),
                    egui::Align2::RIGHT_TOP,
                    line.to_string(),
                    FontId::monospace(CODE_FONT_SIZE),
                    number,
                );
                line += 1;
            }
            starts = row.ends_with_newline;
        }
        // The window's errors underlined, each with its message under the pointer.
        let starts_of_lines: Vec<usize> = std::iter::once(0)
            .chain(
                edit.text
                    .char_indices()
                    .filter(|c| c.1 == '\n')
                    .map(|c| edit.text[..c.0].chars().count() + 1),
            )
            .collect();
        let hover = ui.input(|i| i.pointer.hover_pos());
        let mut hovered_error = None;
        for (l, col, len, msg) in &errors {
            let Some(&first) = starts_of_lines.get(*l) else {
                continue;
            };
            let a = first + col;
            let b = a + (*len).max(1);
            let ra = shown
                .galley
                .pos_from_cursor(CCursor::new(a))
                .translate(shown.galley_pos.to_vec2());
            let rb = shown
                .galley
                .pos_from_cursor(CCursor::new(b))
                .translate(shown.galley_pos.to_vec2());
            let y = ra.max.y - 1.0;
            let x1 = if (rb.min.y - ra.min.y).abs() < 1.0 {
                rb.min.x
            } else {
                ra.min.x + digit * (*len).max(1) as f32
            };
            painter.line_segment(
                [Pos2::new(ra.min.x, y), Pos2::new(x1, y)],
                Stroke::new(1.5, Color32::from_rgb(232, 17, 35)),
            );
            let span = Rect::from_min_max(Pos2::new(ra.min.x, ra.min.y), Pos2::new(x1, ra.max.y));
            if hover.is_some_and(|p| span.contains(p)) {
                hovered_error = Some(msg.clone());
            }
        }
        if let Some(msg) = hovered_error {
            shown
                .response
                .response
                .clone()
                .on_hover_text_at_pointer(msg);
        }
        let changed = shown.response.changed();
        let sel = shown.cursor_range.map(|r| {
            let r = r.as_sorted_char_range();
            (
                utf16_at(&edit.text, r.start.0),
                utf16_at(&edit.text, r.end.0),
            )
        });
        let events = edit_events(changed, &edit.text, sel, edit.sel, &typed);
        if events.is_empty() {
            return None;
        }
        let env = self.session.as_ref()?.env();
        let live = self.live_canvas.as_mut()?;
        let sent = live
            .canvas
            .send(key, &Value::Array(events), &self.values, &env)
            .and_then(|()| live.canvas.token());
        let edit = self.code_edits.entry(key).or_default();
        match sent {
            Ok(token) => {
                edit.seen = Some(edit.text.clone());
                edit.sel = sel;
                self.values.insert(TOKEN.into(), Value::from(token));
                self.read_live();
                Some(changed)
            }
            Err(e) => {
                if let Some(notice) = &env.notice {
                    *notice.lock().unwrap() = Some(e);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        tokens(text)
            .into_iter()
            .filter(|(_, k)| *k != Kind::Plain)
            .map(|(r, k)| (&text[r], k))
            .collect()
    }

    /// CodeLab's script reads in C#'s colours: keywords, built-in types, the literals and comments, a directive at
    /// a line's start, and words that only contain a keyword stay plain.
    #[test]
    fn csharp_tokens() {
        let src = "#region UICode\nint Amount1 = 10; // [0,100] Slider\n#endregion\nvoid Render(Surface dst)\n{\n    string s = @\"a\"\"b\" + $\"{x}\\n\" + 'c';\n    /* note */ if (x >= 1.5f && classy) return;\n}";
        assert_eq!(
            kinds(src),
            [
                ("#region UICode", Kind::Preproc),
                ("int", Kind::Type),
                ("10", Kind::Number),
                ("// [0,100] Slider", Kind::Comment),
                ("#endregion", Kind::Preproc),
                ("void", Kind::Type),
                ("string", Kind::Type),
                ("@\"a\"\"b\"", Kind::Str),
                ("$\"{x}\\n\"", Kind::Str),
                ("'c'", Kind::Str),
                ("/* note */", Kind::Comment),
                ("if", Kind::Keyword),
                ("1.5f", Kind::Number),
                ("return", Kind::Keyword),
            ]
        );
        // The tokens cover the text, in order, on character boundaries.
        let all = tokens("é = \"ü\"; // ß");
        assert_eq!(all.first().unwrap().0.start, 0);
        assert!(all.windows(2).all(|w| w[0].0.end == w[1].0.start));
        assert_eq!(all.last().unwrap().0.end, "é = \"ü\"; // ß".len());
    }

    /// An edit sends the whole text with the selection then each typed character; a caret move alone sends the
    /// selection, once; positions count UTF-16 units, as .NET does.
    #[test]
    fn edits_reach_the_window_as_scintilla_events() {
        let ev = edit_events(true, "ab", Some((2, 2)), None, "b");
        assert_eq!(
            ev,
            [
                json!({"e": "text", "text": "ab", "sel": [2, 2]}),
                json!({"e": "char", "c": "b"})
            ]
        );
        assert_eq!(
            edit_events(false, "ab", Some((1, 1)), Some((2, 2)), ""),
            [json!({"e": "sel", "sel": [1, 1]})]
        );
        assert!(edit_events(false, "ab", Some((1, 1)), Some((1, 1)), "").is_empty());
        assert_eq!(utf16_at("a😀b", 2), 3);
        assert_eq!(char_at_utf16("a😀b", 3), 2);
    }
}
