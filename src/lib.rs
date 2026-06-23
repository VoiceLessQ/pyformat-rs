//! A Rust port of Python's string formatting (CPython 3.13): the `format(value, spec)` mini-language
//! for `str`/`int`/`float`, plus the `str.format` replacement-field grammar.
//!
//! [`format_str`], [`format_int`], and [`format_float`] mirror `format(value, spec)` byte-for-byte:
//! the `[[fill]align][sign][#][0][width][grouping][.precision][type]` grammar, the sign/prefix
//! rules, the thousands-grouping-with-zero-fill behaviour (`format(1234, "08,") == "0,001,234"`),
//! and the float presentation types (`e`/`f`/`g`/`%` and `repr`) using CPython's exact rounding.
//! [`str_format`] mirrors `"...".format(*args, **kwargs)`: positional / keyword / auto field
//! numbering, `!r`/`!s`/`!a` conversions, nested replacement fields in the spec, and brace escapes.
//!
//! ```
//! use pyformat_rs::{format_float, format_int, format_str, str_format, Value};
//!
//! assert_eq!(format_int(255, "#06x").unwrap(), "0x00ff");
//! assert_eq!(format_int(-42, "=8").unwrap(), "-     42");
//! assert_eq!(format_str("hello", ".3").unwrap(), "hel");
//! assert_eq!(format_float(3.14159, ".2f").unwrap(), "3.14");
//! assert_eq!(format_float(0.5, "%").unwrap(), "50.000000%");
//!
//! let args = [Value::Int(7), Value::Str("hi".into())];
//! assert_eq!(str_format("{0} {1!r:>6}", &args, &[]).unwrap(), "7   'hi'");
//! ```
//!
//! Out of scope: the locale type `n`, arbitrary-precision ints (the input is an `i128`),
//! precision above 9999, and `[index]` / `.attr` access in replacement fields.

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

/// Left-pad `s` with `'0'` to `width` chars (manual, so width is unbounded - Rust's `format!` width
/// panics above ~16384, but Python places no such limit).
fn zfill_left(s: &str, width: usize) -> String {
    let cur = s.chars().count();
    if cur >= width {
        return s.to_string();
    }
    let mut o = String::with_capacity(width);
    o.extend(std::iter::repeat_n('0', width - cur));
    o.push_str(s);
    o
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
    group(&zfill_left(digits, d), sep, g)
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
            None => zfill_left(&digits, min_field),
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
            None => zfill_left(intdigits, min_field),
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
    // Rust's `format!` precision panics above ~16384; cap well below that. Python supports larger
    // precisions (padding with zeros); that range is a documented bound here.
    if s.precision.is_some_and(|p| p > 9999) {
        return err("precision too large (bounded to 9999 in this port)");
    }
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

// --------------------------------------------------------------------------- str.format (Layer 3)

/// A scalar value usable as a `str.format` argument. Mirrors the common Python argument types.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i128),
    Float(f64),
    Str(String),
    Bool(bool),
    None,
}

fn str_of(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(x) => format_float(*x, "").unwrap(),
        Value::Str(s) => s.clone(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::None => "None".to_string(),
    }
}

/// Python `str` repr (`ascii_mode` additionally escapes non-ASCII, like `ascii()`).
fn py_str_repr(s: &str, ascii_mode: bool) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            c if c == quote => {
                out.push('\\');
                out.push(quote);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if c.is_ascii() => out.push(c),
            c if !ascii_mode && !c.is_control() => out.push(c),
            c => {
                let cp = c as u32;
                if cp <= 0xff {
                    out.push_str(&format!("\\x{cp:02x}"));
                } else if cp <= 0xffff {
                    out.push_str(&format!("\\u{cp:04x}"));
                } else {
                    out.push_str(&format!("\\U{cp:08x}"));
                }
            }
        }
    }
    out.push(quote);
    out
}

fn repr_of(v: &Value, ascii_mode: bool) -> String {
    match v {
        Value::Str(s) => py_str_repr(s, ascii_mode),
        other => str_of(other),
    }
}

/// `format(value, spec)` dispatched by the value's type (no conversion applied).
fn format_typed(v: &Value, spec: &str) -> R<String> {
    match v {
        Value::Int(n) => format_int(*n, spec),
        Value::Float(x) => format_float(*x, spec),
        Value::Str(s) => format_str(s, spec),
        Value::Bool(b) => {
            if spec.is_empty() {
                Ok(if *b { "True" } else { "False" }.to_string())
            } else {
                format_int(if *b { 1 } else { 0 }, spec)
            }
        }
        Value::None => {
            if spec.is_empty() {
                Ok("None".to_string())
            } else {
                err("unsupported format string passed to NoneType.__format__")
            }
        }
    }
}

fn render_field(v: &Value, conversion: Option<char>, spec: &str) -> R<String> {
    match conversion {
        None => format_typed(v, spec),
        Some('s') => format_str(&str_of(v), spec),
        Some('r') => format_str(&repr_of(v, false), spec),
        Some('a') => format_str(&repr_of(v, true), spec),
        Some(c) => err(format!("Unknown conversion specifier {c}")),
    }
}

/// Field-numbering state shared across a whole `format` call (including nested specs).
struct Numbering {
    auto: usize,
    mode: Mode,
}

#[derive(PartialEq)]
enum Mode {
    Unset,
    Auto,
    Manual,
}

impl Numbering {
    fn resolve<'a>(
        &mut self,
        name: &str,
        args: &'a [Value],
        kwargs: &'a [(String, Value)],
    ) -> R<&'a Value> {
        if name.is_empty() {
            if self.mode == Mode::Manual {
                return err("cannot switch from manual field specification to automatic field numbering");
            }
            self.mode = Mode::Auto;
            let idx = self.auto;
            self.auto += 1;
            args.get(idx).ok_or_else(|| FormatError(format!("Replacement index {idx} out of range")))
        } else if let Ok(idx) = name.parse::<usize>() {
            if self.mode == Mode::Auto {
                return err("cannot switch from automatic field numbering to manual field specification");
            }
            self.mode = Mode::Manual;
            args.get(idx).ok_or_else(|| FormatError(format!("Replacement index {idx} out of range")))
        } else {
            // `[...]` / `.attr` access is not supported in this layer.
            if name.contains(['[', '.']) {
                return err("attribute/index access not supported");
            }
            kwargs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v)
                .ok_or_else(|| FormatError(format!("'{name}'")))
        }
    }
}

/// Read the content of a replacement field (between the outer braces), brace-depth aware.
fn read_field(chars: &[char], start: usize) -> R<(String, usize)> {
    let mut depth = 1;
    let mut i = start;
    let mut content = String::new();
    while i < chars.len() {
        match chars[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((content, i + 1));
                }
            }
            _ => {}
        }
        content.push(chars[i]);
        i += 1;
    }
    err("Single '{' encountered in format string")
}

fn vformat(
    template: &str,
    args: &[Value],
    kwargs: &[(String, Value)],
    state: &mut Numbering,
    depth: usize,
) -> R<String> {
    if depth > 2 {
        return err("Max string recursion exceeded");
    }
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                out.push('{');
                i += 2;
            }
            '}' if i + 1 < chars.len() && chars[i + 1] == '}' => {
                out.push('}');
                i += 2;
            }
            '}' => return err("Single '}' encountered in format string"),
            '{' => {
                let (content, next) = read_field(&chars, i + 1)?;
                i = next;
                // Split name[!conv] from :spec at the first ':'.
                let (name_conv, spec) = match content.split_once(':') {
                    Some((nc, sp)) => (nc, sp.to_string()),
                    None => (content.as_str(), String::new()),
                };
                let (name, conversion) = match name_conv.split_once('!') {
                    Some((n, c)) => {
                        let cc: Vec<char> = c.chars().collect();
                        if cc.len() != 1 {
                            return err("conversion must be one character");
                        }
                        (n, Some(cc[0]))
                    }
                    None => (name_conv, None),
                };
                let value = state.resolve(name, args, kwargs)?.clone();
                let resolved_spec = if spec.contains('{') {
                    vformat(&spec, args, kwargs, state, depth + 1)?
                } else {
                    spec
                };
                out.push_str(&render_field(&value, conversion, &resolved_spec)?);
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// `template.format(*args, **kwargs)` for scalar [`Value`] arguments.
pub fn str_format(template: &str, args: &[Value], kwargs: &[(String, Value)]) -> R<String> {
    let mut state = Numbering { auto: 0, mode: Mode::Unset };
    vformat(template, args, kwargs, &mut state, 0)
}
