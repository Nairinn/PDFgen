//! One shared O(n) word wrap: each word measured once, packed greedily.
//! Handles explicit newlines, collapses runs of spaces, and hard-breaks
//! words wider than the line.

/// Wrap `text` to `max_w` points. `width` measures a word once (points).
/// Returns the wrapped lines; an empty input yields one empty line.
pub fn wrap(text: &str, max_w: f64, mut width: impl FnMut(&str) -> f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0f64;
    let mut pending_space = false;

    for hard_line in text.split('\n') {
        // Collapse whitespace runs and measure each word once.
        let mut word_start: Option<usize> = None;
        let bytes = hard_line.as_bytes();
        let mut flush_word = |lines: &mut Vec<String>,
                              cur: &mut String,
                              cur_w: &mut f64,
                              pending_space: &mut bool,
                              ws: usize,
                              we: usize,
                              bytes: &[u8]| {
            let word = &hard_line[ws..we];
            let _ = bytes;
            let w = width(word);
            let space_w = width(" ");
            let need = if *pending_space && !cur.is_empty() {
                *cur_w + space_w + w
            } else {
                w
            };
            if !cur.is_empty() && need > max_w {
                lines.push(std::mem::take(cur));
                *cur_w = 0.0;
                *pending_space = false;
            }
            if *pending_space && !cur.is_empty() {
                cur.push(' ');
                *cur_w += space_w;
            }
            // Hard-break a word wider than the line.
            if w > max_w && cur.is_empty() {
                // Split on the first boundary that fits.
                let mut piece = String::new();
                let mut piece_w = 0.0f64;
                for ch in word.chars() {
                    let cw = width(&ch.to_string());
                    if piece_w + cw > max_w && !piece.is_empty() {
                        lines.push(std::mem::take(&mut piece));
                        piece_w = 0.0;
                    }
                    piece.push(ch);
                    piece_w += cw;
                }
                cur.push_str(&piece);
                *cur_w = piece_w;
                *pending_space = true;
                return;
            }
            cur.push_str(word);
            *cur_w += w;
            *pending_space = true;
        };

        for (i, &b) in bytes.iter().enumerate() {
            if b == b' ' {
                if let Some(ws) = word_start.take() {
                    flush_word(
                        &mut lines,
                        &mut cur,
                        &mut cur_w,
                        &mut pending_space,
                        ws,
                        i,
                        bytes,
                    );
                }
                pending_space = !cur.is_empty() || pending_space;
            } else if word_start.is_none() {
                word_start = Some(i);
            }
        }
        if let Some(ws) = word_start.take() {
            flush_word(
                &mut lines,
                &mut cur,
                &mut cur_w,
                &mut pending_space,
                ws,
                hard_line.len(),
                bytes,
            );
        }
        // End of the hard line: flush the current line.
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        cur_w = 0.0;
        pending_space = false;
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monospace-ish measure for tests: 10 pt per char.
    fn w10(s: &str) -> f64 {
        10.0 * s.chars().count() as f64
    }

    #[test]
    fn wraps_by_width() {
        // 10 pt per char, 55 pt line: "aa bb" (50) fits, "ccc dd" (60)
        // does not, so dd wraps.
        let lines = wrap("aa bb ccc dd", 55.0, w10);
        assert_eq!(lines, vec!["aa bb", "ccc", "dd"]);
    }

    #[test]
    fn breaks_on_newline() {
        let lines = wrap("one\ntwo", 500.0, w10);
        assert_eq!(lines, vec!["one", "two"]);
    }

    #[test]
    fn collapses_spaces() {
        let lines = wrap("a   b", 500.0, w10);
        assert_eq!(lines, vec!["a b"]);
    }

    #[test]
    fn hard_breaks_oversized_words() {
        let lines = wrap("abcdefghij", 30.0, w10); // 3 chars per line
        assert_eq!(lines.len(), 4); // "abc" "def" "ghi" "j"
        assert_eq!(lines[0], "abc");
        assert_eq!(lines[3], "j");
    }

    #[test]
    fn empty_input_one_line() {
        let lines = wrap("", 50.0, w10);
        assert_eq!(lines, vec![""]);
    }
}
