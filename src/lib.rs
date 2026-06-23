//! A faithful Rust port of Python's format-spec mini-language (CPython 3.13) - Layer 1: `str` and
//! `int` formatting.
//!
//! [`format_str`] and [`format_int`] mirror `format(value, spec)` byte-for-byte: the
//! `[[fill]align][sign][#][0][width][grouping][.precision][type]` grammar, the sign/prefix rules,
//! and the thousands-grouping-with-zero-fill behaviour (`format(1234, "08,") == "0,001,234"`).
//!
//! ```
//! use pyformat_rs::{format_int, format_str};
//!
//! assert_eq!(format_int(255, "#06x").unwrap(), "0x00ff");
//! assert_eq!(format_int(1234567, ",").unwrap(), "1,234,567");
//! assert_eq!(format_int(-42, "=8").unwrap(), "-     42");
//! assert_eq!(format_str("hello", ".3").unwrap(), "hel");
//! ```
//!
//! Out of scope in this layer (Layer 2): the float presentation types (`e`/`f`/`g`/`%`), which on
//! an `int` promote it to `float`; the locale type `n`; and arbitrary-precision ints (the input is
//! an `i128`).

use std::fmt;

/// Error raised for an invalid spec or value (mirrors `ValueError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FormatError {}

type R<T> = Result<T, FormatError>;

fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(FormatError(msg.into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,     // <
    Right,    // >
    Center,   // ^
    AfterSign, // =
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    Minus, // - (default)
    Plus,  // +
    Space, // ' '
}

/// A parsed format spec.
#[derive(Debug, Clone)]
struct Spec {
    fill: char,
    align: Option<Align>,
    align_explicit: bool,
    sign: Sign,
    sign_explicit: bool,
    z: bool,
    alt: bool,
    width: usize,
    grouping: Option<char>,
    precision: Option<usize>,
    ty: Option<char>,
}

/// Parse the spec following CPython's `parse_internal_render_format_spec` field order.
fn parse_spec(spec: &str) -> R<Spec> {
    let chars: Vec<char> = spec.chars().collect();
    let n = chars.len();
    let mut i = 0;

    let is_align = |c: char| matches!(c, '<' | '>' | '^' | '=');
    let to_align = |c: char| match c {
        '<' => Align::Left,
        '>' => Align::Right,
        '^' => Align::Center,
        '=' => Align::AfterSign,
        _ => unreachable!(),
    };

    let mut fill = ' ';
    let mut fill_explicit = false;
    let mut align = None;
    let mut align_explicit = false;
    // [[fill]align]: a fill char is only recognized when followed by an align char.
    if n >= 2 && is_align(chars[1]) {
        fill = chars[0];
        fill_explicit = true;
        align = Some(to_align(chars[1]));
        align_explicit = true;
        i = 2;
    } else if n >= 1 && is_align(chars[0]) {
        align = Some(to_align(chars[0]));
        align_explicit = true;
        i = 1;
    }

    let mut sign = Sign::Minus;
    let mut sign_explicit = false;
    if i < n && matches!(chars[i], '+' | '-' | ' ') {
        sign = match chars[i] {
            '+' => Sign::Plus,
            '-' => Sign::Minus,
            ' ' => Sign::Space,
            _ => unreachable!(),
        };
        sign_explicit = true;
        i += 1;
    }

    let mut z = false;
    if i < n && chars[i] == 'z' {
        z = true;
        i += 1;
    }

    let mut alt = false;
    if i < n && chars[i] == '#' {
        alt = true;
        i += 1;
    }

    if i < n && chars[i] == '0' {
        // CPython: fill='0' only if not explicitly given; align='=' only if none was given.
        if !fill_explicit {
            fill = '0';
        }
        if align.is_none() {
            align = Some(Align::AfterSign);
        }
        i += 1;
    }

    let mut width = 0usize;
    let mut saw_width = false;
    while i < n && chars[i].is_ascii_digit() {
        saw_width = true;
        width = width * 10 + (chars[i] as usize - '0' as usize);
        i += 1;
    }
    let _ = saw_width;

    let mut grouping = None;
    if i < n && matches!(chars[i], ',' | '_') {
        grouping = Some(chars[i]);
        i += 1;
    }

    let mut precision = None;
    if i < n && chars[i] == '.' {
        i += 1;
        let mut p = 0usize;
        let mut saw = false;
        while i < n && chars[i].is_ascii_digit() {
            saw = true;
            p = p * 10 + (chars[i] as usize - '0' as usize);
            i += 1;
        }
        if !saw {
            return err("Format specifier missing precision");
        }
        precision = Some(p);
    }

    let mut ty = None;
    if i < n {
        ty = Some(chars[i]);
        i += 1;
    }

    if i != n {
        return err(format!("Invalid format specifier '{spec}'"));
    }

    Ok(Spec {
        fill,
        align,
        align_explicit,
        sign,
        sign_explicit,
        z,
        alt,
        width,
        grouping,
        precision,
        ty,
    })
}

/// Insert `sep` every `g` characters of `s`, counting from the right.
fn group(s: &str, sep: char, g: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(n + n / g);
    for (idx, c) in chars.iter().enumerate() {
        if idx > 0 && (n - idx).is_multiple_of(g) {
            out.push(sep);
        }
        out.push(*c);
    }
    out
}

/// Zero-fill `digits` (already the natural digits) and group, so the grouped field is at least
/// `min_field` wide without a leading separator. Mirrors CPython's grouped zero-padding.
fn grouped_zero_fill(digits: &str, sep: char, g: usize, min_field: usize) -> String {
    let real = digits.chars().count().max(1);
    let mut d = real;
    loop {
        let gw = d + (d - 1) / g;
        if gw >= min_field {
            break;
        }
        d += 1;
    }
    let padded = format!("{digits:0>d$}");
    group(&padded, sep, g)
}

fn pad(core: &str, fill: char, align: Align, width: usize, sign_prefix_len: usize, body: &str) -> String {
    let cur = body.chars().count();
    if cur >= width {
        return body.to_string();
    }
    let total_pad = width - cur;
    match align {
        Align::Left => {
            let mut s = body.to_string();
            s.extend(std::iter::repeat_n(fill, total_pad));
            s
        }
        Align::Right => {
            let mut s: String = std::iter::repeat_n(fill, total_pad).collect();
            s.push_str(body);
            s
        }
        Align::Center => {
            let left = total_pad / 2;
            let right = total_pad - left;
            let mut s: String = std::iter::repeat_n(fill, left).collect();
            s.push_str(body);
            s.extend(std::iter::repeat_n(fill, right));
            s
        }
        Align::AfterSign => {
            // fill goes between the sign+prefix and the number body
            let (head, tail) = core.split_at(sign_prefix_byte_len(core, sign_prefix_len));
            let mut s = head.to_string();
            s.extend(std::iter::repeat_n(fill, total_pad));
            s.push_str(tail);
            s
        }
    }
}

/// Byte offset of the first `n` chars of `s` (sign+prefix length in chars).
fn sign_prefix_byte_len(s: &str, n_chars: usize) -> usize {
    s.char_indices().nth(n_chars).map(|(b, _)| b).unwrap_or(s.len())
}

/// `format(value, spec)` for a `str` value.
pub fn format_str(value: &str, spec: &str) -> R<String> {
    let s = parse_spec(spec)?;
    if s.sign_explicit {
        return err("Sign not allowed in string format specifier");
    }
    if s.z {
        return err("z option not allowed in string format specifier");
    }
    if s.alt {
        return err("Alternate form (#) not allowed in string format specifier");
    }
    if s.grouping.is_some() {
        return err("Cannot specify ',' or '_' with 's'");
    }
    if s.align == Some(Align::AfterSign) && s.align_explicit {
        return err("'=' alignment not allowed in string format specifier");
    }
    match s.ty {
        None | Some('s') => {}
        Some(t) => return err(format!("Unknown format code '{t}' for object of type 'str'")),
    }

    let mut body: String = value.chars().collect();
    if let Some(p) = s.precision {
        body = body.chars().take(p).collect();
    }

    // The '0' flag for str means fill '0' but keeps the default left alignment.
    let align = match s.align {
        Some(Align::AfterSign) => Align::Left, // the '0'-implied '='
        Some(a) => a,
        None => Align::Left,
    };
    Ok(pad(&body, s.fill, align, s.width, 0, &body))
}

/// `format(value, spec)` for an `int` value (i128 range in this layer).
pub fn format_int(value: i128, spec: &str) -> R<String> {
    let s = parse_spec(spec)?;
    if s.precision.is_some() {
        return err("Precision not allowed in integer format specifier");
    }
    if s.z {
        return err("Negative zero coercion (z) not allowed in integer format specifier");
    }

    let ty = s.ty.unwrap_or('d');
    // Float presentation types promote int->float: Layer 2.
    if matches!(ty, 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' | 'n') {
        return err("float presentation type on int is Layer 2");
    }

    // grouping validity: ',' only with 'd'; '_' with 'd'/'b'/'o'/'x'/'X'.
    if let Some(gch) = s.grouping {
        match gch {
            ',' if ty != 'd' => return err(format!("Cannot specify ',' with '{ty}'.")),
            '_' if !matches!(ty, 'd' | 'b' | 'o' | 'x' | 'X') => {
                return err(format!("Cannot specify '_' with '{ty}'."))
            }
            _ => {}
        }
    }

    if ty == 'c' {
        if s.sign_explicit {
            return err("Sign not allowed with integer format specifier 'c'");
        }
        if s.alt {
            return err("Alternate form (#) not allowed with integer format specifier 'c'");
        }
        if s.grouping.is_some() {
            return err("Cannot specify ',' with 'c'.");
        }
        if !(0..=0x10FFFF).contains(&value) {
            return err("%c arg not in range");
        }
        let cp = value as u32;
        let ch = char::from_u32(cp).ok_or_else(|| FormatError("invalid codepoint".into()))?;
        // 'c' has no sign/prefix, so '=' padding (head_len 0) coincides with right alignment.
        let body: String = ch.to_string();
        let align = s.align.unwrap_or(Align::Right);
        return Ok(pad(&body, s.fill, align, s.width, 0, &body));
    }

    let mag = value.unsigned_abs();
    let (digits, prefix): (String, &str) = match ty {
        'd' => (format!("{mag}"), ""),
        'b' => (format!("{mag:b}"), if s.alt { "0b" } else { "" }),
        'o' => (format!("{mag:o}"), if s.alt { "0o" } else { "" }),
        'x' => (format!("{mag:x}"), if s.alt { "0x" } else { "" }),
        'X' => (format!("{mag:X}"), if s.alt { "0X" } else { "" }),
        other => return err(format!("Unknown format code '{other}' for object of type 'int'")),
    };

    let sign_str = if value < 0 {
        "-"
    } else {
        match s.sign {
            Sign::Plus => "+",
            Sign::Space => " ",
            Sign::Minus => "",
        }
    };

    let gsize = if matches!(ty, 'b' | 'o' | 'x' | 'X') { 4 } else { 3 };
    let head_len = sign_str.chars().count() + prefix.chars().count();

    // Zero-pad mode: align '=' with '0' fill produces a grouped/zero-filled digit field.
    if s.align == Some(Align::AfterSign) && s.fill == '0' {
        let min_field = s.width.saturating_sub(head_len);
        let number = match s.grouping {
            Some(sep) => grouped_zero_fill(&digits, sep, gsize, min_field),
            None => format!("{digits:0>min_field$}"),
        };
        return Ok(format!("{sign_str}{prefix}{number}"));
    }

    let number = match s.grouping {
        Some(sep) => group(&digits, sep, gsize),
        None => digits,
    };
    let body = format!("{sign_str}{prefix}{number}");
    let align = s.align.unwrap_or(Align::Right);
    Ok(pad(&body, s.fill, align, s.width, head_len, &body))
}
