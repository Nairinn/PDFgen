//! Decoding PDF strings from content streams: literal strings with
//! escapes (\\, \(, \), \n, \r, \t, \b, \f, octal \ddd) and hex strings.
//! Shared by the renderer and text extraction so they agree byte-for-byte.

/// Decode one literal string starting at `i` (content[i] == b'(').
/// Returns the decoded bytes and the index just past the closing paren.
#[must_use] 
pub fn read_literal(content: &[u8], i: usize) -> Option<(Vec<u8>, usize)> {
    if i >= content.len() || content[i] != b'(' {
        return None;
    }
    let mut j = i + 1;
    let mut depth = 1usize;
    let mut out: Vec<u8> = Vec::with_capacity(16);
    while j < content.len() {
        match content[j] {
            b'\\' => {
                j += 1;
                if j >= content.len() {
                    break;
                }
                match content[j] {
                    b'n' => {
                        out.push(b'\n');
                        j += 1;
                    }
                    b'r' => {
                        out.push(b'\r');
                        j += 1;
                    }
                    b't' => {
                        out.push(b'\t');
                        j += 1;
                    }
                    b'b' => {
                        out.push(0x08);
                        j += 1;
                    }
                    b'f' => {
                        out.push(0x0C);
                        j += 1;
                    }
                    b'(' | b')' | b'\\' => {
                        out.push(content[j]);
                        j += 1;
                    }
                    b'\n' => {
                        // Line continuation: skip.
                        j += 1;
                    }
                    c if c.is_ascii_digit() => {
                        // Octal: up to 3 digits.
                        let mut digits = 0u32;
                        let mut take = 0;
                        while take < 3
                            && j < content.len()
                            && content[j].is_ascii_digit()
                            && content[j] != b'8'
                            && content[j] != b'9'
                        {
                            digits = digits * 8 + u32::from(content[j] - b'0');
                            j += 1;
                            take += 1;
                        }
                        out.push(digits as u8);
                    }
                    c => {
                        out.push(c);
                        j += 1;
                    }
                }
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
                j += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((out, j + 1));
                }
                out.push(b')');
                j += 1;
            }
            c => {
                out.push(c);
                j += 1;
            }
        }
    }
    Some((out, j))
}

/// Decode one hex string starting at `i` (content[i] == b'<', with the
/// next byte not b'<'). Returns the decoded bytes and the index just
/// past the closing b'>'.
#[must_use] 
pub fn read_hex(content: &[u8], i: usize) -> Option<(Vec<u8>, usize)> {
    if i >= content.len() || content[i] != b'<' {
        return None;
    }
    let mut j = i + 1;
    let mut out: Vec<u8> = Vec::new();
    let mut hi: Option<u8> = None;
    while j < content.len() && content[j] != b'>' {
        let c = content[j].to_ascii_uppercase();
        if let Some(v) = (c as char).to_digit(16) {
            match hi {
                None => hi = Some(v as u8),
                Some(h) => {
                    out.push((h << 4) | v as u8);
                    hi = None;
                }
            }
        }
        j += 1;
    }
    // Trailing nibble with an odd count: pad low.
    if let Some(h) = hi {
        out.push(h << 4);
    }
    if j < content.len() {
        j += 1; // past '>'
    }
    Some((out, j))
}
