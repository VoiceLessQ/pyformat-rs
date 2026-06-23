//! Conformance vectors lifted verbatim from CPython's own test suite,
//! `Lib/test/test_types.py::TypesTests.test_int__format__` (CPython 3.13), plus a few documented
//! `str` cases. Each `int` row below appears as a `test(value, spec, expected)` call in CPython.
//!
//! Static, Python-free proof (`cargo test`); the exhaustive differential lives in `difftest.py`
//! (46k+ ops vs the live `format()` builtin).

use pyformat_rs::{format_float, format_int, format_str};

fn fi(v: i128, spec: &str) -> String {
    format_int(v, spec).unwrap()
}
fn fs(v: &str, spec: &str) -> String {
    format_str(v, spec).unwrap()
}
fn ff(v: f64, spec: &str) -> String {
    format_float(v, spec).unwrap()
}

// test_types.py: test_int__format__ (decimal, sign, alignment)
#[test]
fn int_decimal_and_sign() {
    assert_eq!(fi(123456789, "d"), "123456789");
    assert_eq!(fi(1, "-"), "1");
    assert_eq!(fi(-1, "-"), "-1");
    assert_eq!(fi(1, "-3"), "  1");
    assert_eq!(fi(-1, "-3"), " -1");
    assert_eq!(fi(1, "+3"), " +1");
    assert_eq!(fi(1, " 3"), "  1");
    assert_eq!(fi(-1, " "), "-1");
    assert_eq!(fi(1, " "), " 1");
}

// test_types.py: test_int__format__ (hex / octal / binary)
#[test]
fn int_bases() {
    assert_eq!(fi(1234, "x"), "4d2");
    assert_eq!(fi(-1234, "x"), "-4d2");
    assert_eq!(fi(1234, "8x"), "     4d2");
    assert_eq!(fi(-1234, "8x"), "    -4d2");
    assert_eq!(fi(0xbe, "X"), "BE");
    assert_eq!(fi(-0xbe, "X"), "-BE");
    assert_eq!(fi(65, "o"), "101");
    assert_eq!(fi(1234, "o"), "2322");
    assert_eq!(fi(1234, "+o"), "+2322");
    assert_eq!(fi(1234, "b"), "10011010010");
    assert_eq!(fi(-1234, "b"), "-10011010010");
    assert_eq!(fi(1234, " b"), " 10011010010");
}

// test_types.py: test_int__format__ (alternate '#' form)
#[test]
fn int_alternate() {
    assert_eq!(fi(0, "#b"), "0b0");
    assert_eq!(fi(-1, "-#5b"), " -0b1");
    assert_eq!(fi(100, "+#b"), "+0b1100100");
    assert_eq!(fi(100, "#012b"), "0b0001100100");
    assert_eq!(fi(-100, "#012b"), "-0b001100100");
    assert_eq!(fi(100, "#012o"), "0o0000000144");
    assert_eq!(fi(123456, "#012x"), "0x000001e240");
    assert_eq!(fi(-123456, "#012x"), "-0x00001e240");
    assert_eq!(fi(123456, "#012X"), "0X000001E240");
    assert_eq!(fi(-1, "-#5X"), " -0X1");
}

// test_types.py: test_int__format__ (thousands grouping, incl. issue 5782 zero-fill)
#[test]
fn int_grouping() {
    assert_eq!(fi(1234, ","), "1,234");
    assert_eq!(fi(-1234567, ","), "-1,234,567");
    assert_eq!(fi(123456, ","), "123,456");
    assert_eq!(fi(1234, "010,"), "00,001,234");
}

// test_types.py: test_int__format__ ('c' presentation type)
#[test]
fn int_char() {
    assert_eq!(fi(1, "c"), "\u{01}");
    assert_eq!(fi(65, "c"), "A");
}

// Documented str format-spec behaviour (Python docs / format() builtin).
#[test]
fn str_basic() {
    assert_eq!(fs("hi", "<8"), "hi      ");
    assert_eq!(fs("hi", ">8"), "      hi");
    assert_eq!(fs("hi", "^8"), "   hi   ");
    assert_eq!(fs("hello", ".3"), "hel");
    assert_eq!(fs("hello", "8.3"), "hel     ");
    assert_eq!(fs("hi", "*<8"), "hi******");
    assert_eq!(fs("hi", "08"), "hi000000");
}

// test_types.py: test_float__format__ (presentation types, sign, exponent)
#[test]
fn float_types() {
    assert_eq!(ff(0.0, "f"), "0.000000");
    assert_eq!(ff(0.0, ""), "0.0");
    assert_eq!(ff(0.01, ""), "0.01");
    assert_eq!(ff(0.01, "g"), "0.01");
    assert_eq!(ff(1.0, " g"), " 1");
    assert_eq!(ff(1.0, "+g"), "+1");
    assert_eq!(ff(1.1234e200, "g"), "1.1234e+200");
    assert_eq!(ff(1.1234e200, "G"), "1.1234E+200");
    assert_eq!(ff(1.0, "e"), "1.000000e+00");
    assert_eq!(ff(-1.0, "E"), "-1.000000E+00");
    assert_eq!(ff(1.1234e20, "e"), "1.123400e+20");
    assert_eq!(ff(1e200, "+"), "+1e+200");
    assert_eq!(ff(1.1e200, "+"), "+1.1e+200");
}

// test_types.py: test_float__format__ (zero-fill, grouping, precision)
#[test]
fn float_padding_grouping() {
    assert_eq!(ff(1234., "012f"), "01234.000000");
    assert_eq!(ff(-1234., "013f"), "-01234.000000");
    assert_eq!(ff(-1234.12341234, "013f"), "-01234.123412");
    assert_eq!(ff(-123456.12341234, "011.2f"), "-0123456.12");
    assert_eq!(ff(1.2, "010,.2"), "0,000,001.2");
    assert_eq!(ff(1234., "013,f"), "01,234.000000");
    assert_eq!(ff(-1234., "014,f"), "-01,234.000000");
}

// Documented float edges (Python docs / observed behaviour).
#[test]
fn float_edges() {
    assert_eq!(ff(2.5, ".0f"), "2"); // round half to even
    assert_eq!(ff(3.5, ".0f"), "4");
    assert_eq!(ff(0.5, "%"), "50.000000%");
    assert_eq!(ff(1e16, ""), "1e+16");
    assert_eq!(ff(1e-5, ""), "1e-05");
    assert_eq!(ff(0.0001, ""), "0.0001");
    assert_eq!(ff(-0.0, ".2f"), "-0.00");
    assert_eq!(ff(-0.001, "z.2f"), "0.00"); // z coerces negative zero
    assert_eq!(ff(42.0, "#.0f"), "42.");
    assert_eq!(ff(f64::INFINITY, "f"), "inf");
    assert_eq!(ff(f64::NEG_INFINITY, ""), "-inf");
    assert_eq!(ff(f64::NAN, "F"), "NAN");
    assert_eq!(format_int(1234567, ".2g").unwrap(), "1.2e+06"); // int promotes to float
}

// Spec errors raised by CPython (ValueError).
#[test]
fn errors() {
    assert!(format_str("hi", "d").is_err()); // type 'd' invalid for str
    assert!(format_str("hi", "+").is_err()); // sign invalid for str
    assert!(format_str("hi", "=8").is_err()); // explicit '=' invalid for str
    assert!(format_int(42, ".2").is_err()); // precision invalid for int
    assert!(format_int(42, ",x").is_err()); // ',' invalid with 'x'
    assert!(format_int(42, ",_d").is_err()); // both ',' and '_'
    assert!(format_int(65, "+c").is_err()); // sign invalid with 'c'
}
