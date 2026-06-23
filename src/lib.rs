//! A Rust port of Python's format-spec mini-language (CPython 3.13) - `str`, `int`, and `float`
//! formatting.
//!
//! [`format_str`], [`format_int`], and [`format_float`] mirror `format(value, spec)` byte-for-byte:
//! the `[[fill]align][sign][#][0][width][grouping][.precision][type]` grammar, the sign/prefix
//! rules, the thousands-grouping-with-zero-fill behaviour (`format(1234, "08,") == "0,001,234"`),
//! and the float presentation types (`e`/`f`/`g`/`%` and `repr`) using CPython's exact rounding.
//!
//! ```
//! use pyformat_rs::{format_float, format_int, format_str};
//!
//! assert_eq!(format_int(255, "#06x").unwrap(), "0x00ff");
//! assert_eq!(format_int(1234567, ",").unwrap(), "1,234,567");
//! assert_eq!(format_int(-42, "=8").unwrap(), "-     42");
//! assert_eq!(format_str("hello", ".3").unwrap(), "hel");
//! assert_eq!(format_float(3.14159, ".2f").unwrap(), "3.14");
//! assert_eq!(format_float(1234.5, ",.1f").unwrap(), "1,234.5");
//! assert_eq!(format_float(0.5, "%").unwrap(), "50.000000%");
//! ```
//!
//! Out of scope: the locale type `n`, arbitrary-precision ints (the input is an `i128`), and the
//! `str.format` replacement-field grammar.

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
    // Float presentation types promote the int to float (CPython does the same); checked before the
    // integer-only validation because those types legitimately accept a precision.
    if let Some(t) = s.ty {
        if matches!(t, 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%') {
            return format_float(value as f64, spec);
        }
        if t == 'n' {
            return err("locale type 'n' not supported");
        }
    }
    if s.precision.is_some() {
        return err("Precision not allowed in integer format specifier");
    }
    if s.z {
        return err("Negative zero coercion (z) not allowed in integer format specifier");
    }

    let ty = s.ty.unwrap_or('d');

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

// --------------------------------------------------------------------------- float (Layer 2)

/// Split a `{:e}`-style string into (mantissa, exponent).
fn split_e(s: &str) -> (&str, i32) {
    let (m, e) = s.split_once('e').expect("exponential form");
    (m, e.parse::<i32>().expect("exponent"))
}

/// CPython exponent rendering: a sign and at least two digits (`e+01`, `e-05`, `e+100`).
fn fmt_exp(exp: i32) -> String {
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{sign}{:02}", exp.unsigned_abs())
}

/// Strip trailing fractional zeros (and a bare trailing `.`).
fn strip_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Split a rendered unsigned number into (integer digits, the rest: `.`/`e`/`%` onward).
fn split_int_rest(s: &str) -> (String, String) {
    match s.find(['.', 'e', 'E', '%']) {
        Some(i) => (s[..i].to_string(), s[i..].to_string()),
        None => (s.to_string(), String::new()),
    }
}

/// The `g`/`G` algorithm (also the engine for none-with-precision). `p` is the significant-digit
/// count; `sci_at` is the exponent at/above which scientific notation is used (`p` for `g`,
/// `p - 1` for none-with-precision).
fn g_format(xabs: f64, p: usize, sci_at: i32, alt: bool, echar: char, add_dot0: bool) -> (String, String) {
    let es = format!("{:.*e}", p.saturating_sub(1), xabs);
    let (mant_full, exp) = split_e(&es);
    let result = if exp < -4 || exp >= sci_at {
        let mut mant = if alt { mant_full.to_string() } else { strip_zeros(mant_full) };
        if alt && !mant.contains('.') {
            mant.push('.');
        }
        format!("{mant}{echar}{}", fmt_exp(exp))
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        let s = format!("{xabs:.decimals$}");
        let mut s = if alt { s } else { strip_zeros(&s) };
        if alt && !s.contains('.') {
            s.push('.');
        }
        s
    };
    let result = if add_dot0 && !result.contains('.') && !result.contains(echar) {
        format!("{result}.0")
    } else {
        result
    };
    split_int_rest(&result)
}

/// Python `repr`/`str` of a finite float: shortest round-tripping form, fixed when `-4 <= exp < 16`
/// else scientific, always carrying a decimal point in fixed form.
/// Shortest round-tripping significant digits + exponent (Rust's `{:e}` is shortest-correct).
///
/// Note: at an exact decimal-midpoint double, CPython's dtoa and Rust's shortest-repr may pick opposite
/// equally-short, equally-round-tripping decimals (e.g. `90593674776370.12` vs `.13`). Both are
/// valid shortest reprs; this is the one byte-level repr divergence the differential tolerates.
fn shortest_digits(xabs: f64) -> (String, i32) {
    let es = format!("{xabs:e}");
    let (mant, exp) = split_e(&es);
    (mant.chars().filter(|c| *c != '.').collect(), exp)
}

fn repr_format(xabs: f64, alt: bool, echar: char) -> (String, String) {
    if xabs == 0.0 {
        return ("0".to_string(), ".0".to_string());
    }
    let (digits, exp) = shortest_digits(xabs);
    if (-4..16).contains(&exp) {
        if exp >= 0 {
            let intlen = exp as usize + 1;
            if digits.len() <= intlen {
                let intp = format!("{digits:0<intlen$}");
                (intp, ".0".to_string())
            } else {
                (digits[..intlen].to_string(), format!(".{}", &digits[intlen..]))
            }
        } else {
            let zeros = (-exp - 1) as usize;
            ("0".to_string(), format!(".{}{}", "0".repeat(zeros), digits))
        }
    } else {
        let rest = &digits[1..];
        // '#' keeps a decimal point even when the mantissa is a single digit (`1.e+16`).
        let mant_rest = if rest.is_empty() {
            if alt { ".".to_string() } else { String::new() }
        } else {
            format!(".{rest}")
        };
        (digits[..1].to_string(), format!("{mant_rest}{echar}{}", fmt_exp(exp)))
    }
}

/// Produce (integer digits, rest) for a finite non-negative float under the given type/precision.
fn float_number(xabs: f64, ty: Option<char>, prec: Option<usize>, alt: bool, echar: char) -> (String, String) {
    match ty.map(|c| c.to_ascii_lowercase()) {
        Some('f') => {
            let p = prec.unwrap_or(6);
            let mut s = format!("{xabs:.p$}");
            if alt && p == 0 {
                s.push('.');
            }
            split_int_rest(&s)
        }
        Some('%') => {
            let p = prec.unwrap_or(6);
            let mut s = format!("{:.p$}", xabs * 100.0);
            if alt && p == 0 {
                s.push('.');
            }
            s.push('%');
            split_int_rest(&s)
        }
        Some('e') => {
            let p = prec.unwrap_or(6);
            let es = format!("{xabs:.p$e}");
            let (m, exp) = split_e(&es);
            let mut mant = m.to_string();
            if alt && p == 0 && !mant.contains('.') {
                mant.push('.');
            }
            split_int_rest(&format!("{mant}{echar}{}", fmt_exp(exp)))
        }
        Some('g') => {
            let p = prec.unwrap_or(6).max(1);
            g_format(xabs, p, p as i32, alt, echar, false)
        }
        None => match prec {
            None => repr_format(xabs, alt, echar),
            Some(p) => {
                let p = p.max(1);
                g_format(xabs, p, p as i32 - 1, alt, echar, true)
            }
        },
        _ => unreachable!(),
    }
}

/// Test whether all digit characters in the two parts are zero (for `z` negative-zero coercion).
fn all_zero(intdigits: &str, rest: &str) -> bool {
    !intdigits.chars().chain(rest.chars()).any(|c| c.is_ascii_digit() && c != '0')
}

/// Assemble sign + prefix + grouped/zero-filled integer digits + rest, then pad to width.
fn assemble(sign: &str, intdigits: &str, rest: &str, spec: &Spec, gsize: usize) -> String {
    let head_len = sign.chars().count();
    if spec.align == Some(Align::AfterSign) && spec.fill == '0' {
        let min_field = spec.width.saturating_sub(head_len + rest.chars().count());
        let number = match spec.grouping {
            Some(sep) => grouped_zero_fill(intdigits, sep, gsize, min_field),
            None => format!("{intdigits:0>min_field$}"),
        };
        return format!("{sign}{number}{rest}");
    }
    let number = match spec.grouping {
        Some(sep) => group(intdigits, sep, gsize),
        None => intdigits.to_string(),
    };
    let body = format!("{sign}{number}{rest}");
    let align = spec.align.unwrap_or(Align::Right);
    pad(&body, spec.fill, align, spec.width, head_len, &body)
}

/// `format(value, spec)` for a `float` value.
pub fn format_float(value: f64, spec: &str) -> R<String> {
    let s = parse_spec(spec)?;
    let ty = s.ty;
    let upper = matches!(ty, Some('E') | Some('F') | Some('G'));
    match ty {
        None | Some('e') | Some('E') | Some('f') | Some('F') | Some('g') | Some('G') | Some('%') => {}
        Some('n') => return err("locale type 'n' not supported"),
        Some(t) => return err(format!("Unknown format code '{t}' for object of type 'float'")),
    }

    let neg = value.is_sign_negative() && !value.is_nan();
    let sign_str = |neg: bool| -> &'static str {
        if neg {
            "-"
        } else {
            match s.sign {
                Sign::Plus => "+",
                Sign::Space => " ",
                Sign::Minus => "",
            }
        }
    };

    if value.is_nan() || value.is_infinite() {
        let mut word = if value.is_nan() { "nan" } else { "inf" }.to_string();
        if upper {
            word = word.to_uppercase();
        }
        // The '%' type still appends its percent sign to inf/nan.
        let rest = if s.ty == Some('%') { "%" } else { "" };
        // inf/nan never carry grouping; zero-fill still applies.
        let mut sp = s.clone();
        sp.grouping = None;
        return Ok(assemble(sign_str(neg), &word, rest, &sp, 3));
    }

    let xabs = value.abs();
    let echar = if upper { 'E' } else { 'e' };
    let (intdigits, rest) = float_number(xabs, ty, s.precision, s.alt, echar);
    let neg = if s.z && neg && all_zero(&intdigits, &rest) { false } else { neg };
    Ok(assemble(sign_str(neg), &intdigits, &rest, &s, 3))
}
