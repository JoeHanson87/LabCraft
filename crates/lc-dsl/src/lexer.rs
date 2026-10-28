//! Hand-written lexer for the LabCraft VI DSL.
//!
//! The DSL is intentionally small: literals, identifiers, pipelines (`->`),
//! property lists and range operators. Keeping the token set tiny makes the
//! grammar easy to teach to agents and easy to keep stable.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Number {
        value: f64,
        is_int: bool,
        raw: String,
    },
    Str(String),
    Arrow,
    DotDot,
    Punct(char),
    Eof,
}

impl Tok {
    pub fn is_ident(&self, name: &str) -> bool {
        matches!(self, Tok::Ident(s) if s == name)
    }
    pub fn is_punct(&self, c: char) -> bool {
        matches!(self, Tok::Punct(p) if *p == c)
    }
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Ident(s) => write!(f, "`{s}`"),
            Tok::Number { raw, .. } => write!(f, "`{raw}`"),
            Tok::Str(s) => write!(f, "\"{s}\""),
            Tok::Arrow => write!(f, "`->`"),
            Tok::DotDot => write!(f, "`..`"),
            Tok::Punct(c) => write!(f, "`{c}`"),
            Tok::Eof => write!(f, "end of input"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
}

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;

    let peek2 = |i: usize| chars.get(i + 1).copied();

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            i += 1;
            line += 1;
            col = 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            col += 1;
            continue;
        }
        // Comments: // ... and # ...
        if (c == '/' && peek2(i) == Some('/')) || c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && peek2(i) == Some('*') {
            let (l0, c0) = (line, col);
            i += 2;
            col += 2;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '*' && peek2(i) == Some('/') {
                    i += 2;
                    col += 2;
                    closed = true;
                    break;
                }
                if chars[i] == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
            }
            if !closed {
                return Err(LexError {
                    message: "unterminated block comment".into(),
                    line: l0,
                    col: c0,
                });
            }
            continue;
        }

        let start_line = line;
        let start_col = col;

        // Arrow and minus.
        if c == '-' && peek2(i) == Some('>') {
            out.push(Token {
                tok: Tok::Arrow,
                line: start_line,
                col: start_col,
            });
            i += 2;
            col += 2;
            continue;
        }
        if c == '.' && peek2(i) == Some('.') {
            out.push(Token {
                tok: Tok::DotDot,
                line: start_line,
                col: start_col,
            });
            i += 2;
            col += 2;
            continue;
        }

        if c.is_ascii_digit() {
            let mut j = i;
            let mut has_dot = false;
            let mut has_exp = false;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            // A `..` immediately after digits is a range, not a decimal point.
            if j < chars.len() && chars[j] == '.' && chars.get(j + 1).copied() != Some('.') {
                has_dot = true;
                j += 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if j < chars.len() && (chars[j] == 'e' || chars[j] == 'E') {
                let mut k = j + 1;
                if k < chars.len() && (chars[k] == '+' || chars[k] == '-') {
                    k += 1;
                }
                let digits_start = k;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                if k > digits_start {
                    has_exp = true;
                    j = k;
                }
            }
            let raw: String = chars[i..j].iter().collect();
            let value = raw.parse::<f64>().map_err(|_| LexError {
                message: format!("invalid number `{raw}`"),
                line: start_line,
                col: start_col,
            })?;
            out.push(Token {
                tok: Tok::Number {
                    value,
                    is_int: !has_dot && !has_exp,
                    raw,
                },
                line: start_line,
                col: start_col,
            });
            col += j - i;
            i = j;
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let ident: String = chars[i..j].iter().collect();
            out.push(Token {
                tok: Tok::Ident(ident),
                line: start_line,
                col: start_col,
            });
            col += j - i;
            i = j;
            continue;
        }

        if c == '"' {
            let mut j = i + 1;
            let mut s = String::new();
            let mut closed = false;
            while j < chars.len() {
                match chars[j] {
                    '"' => {
                        closed = true;
                        j += 1;
                        break;
                    }
                    '\\' => {
                        j += 1;
                        let Some(esc) = chars.get(j).copied() else {
                            break;
                        };
                        match esc {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'r' => s.push('\r'),
                            '\\' => s.push('\\'),
                            '"' => s.push('"'),
                            '0' => s.push('\0'),
                            'u' => {
                                // \uXXXX
                                let mut hex = String::new();
                                for k in 0..4 {
                                    if let Some(h) = chars.get(j + 1 + k) {
                                        hex.push(*h);
                                    }
                                }
                                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                                    if let Some(ch) = char::from_u32(code) {
                                        s.push(ch);
                                    }
                                    j += 4;
                                }
                            }
                            other => s.push(other),
                        }
                        j += 1;
                    }
                    '\n' => break,
                    other => {
                        s.push(other);
                        j += 1;
                    }
                }
            }
            if !closed {
                return Err(LexError {
                    message: "unterminated string literal".into(),
                    line: start_line,
                    col: start_col,
                });
            }
            // Column: count consumed characters (no newlines allowed).
            col += j - i;
            i = j;
            out.push(Token {
                tok: Tok::Str(s),
                line: start_line,
                col: start_col,
            });
            continue;
        }

        if "{}()[],:=@.;<>!".contains(c) {
            out.push(Token {
                tok: Tok::Punct(c),
                line: start_line,
                col: start_col,
            });
            i += 1;
            col += 1;
            continue;
        }

        // A lone '-' is a valid token so negative literals can be parsed.
        if c == '-' {
            out.push(Token {
                tok: Tok::Punct('-'),
                line: start_line,
                col: start_col,
            });
            i += 1;
            col += 1;
            continue;
        }

        return Err(LexError {
            message: format!("unexpected character `{c}`"),
            line: start_line,
            col: start_col,
        });
    }

    out.push(Token {
        tok: Tok::Eof,
        line,
        col,
    });
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}
