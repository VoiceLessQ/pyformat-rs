//! Conformance vectors lifted verbatim from CPython's own test suite,
//! `Lib/test/test_types.py::TypesTests.test_int__format__` (CPython 3.13), plus a few documented
//! `str` cases. Each `int` row below appears as a `test(value, spec, expected)` call in CPython.
//!
//! Static, Python-free proof (`cargo test`); the exhaustive differential lives in `difftest.py`
//! (46k+ ops vs the live `format()` builtin).

use pyformat_rs::{format_int, format_str};

fn fi(v: i128, spec: &str) -> String {
    format_int(v, spec).unwrap()
}
fn fs(v: &str, spec: &str) -> String {
    format_str(v, spec).unwrap()
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
