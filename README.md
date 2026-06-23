# pyformat-rs

A **faithful** Rust port of Python's
[format-spec mini-language](https://docs.python.org/3/library/string.html#format-specification-mini-language)
(CPython 3.13) - currently `str` and `int` formatting. [`format_str`] and [`format_int`] mirror
`format(value, spec)` byte-for-byte: the
`[[fill]align][sign][#][0][width][grouping][.precision][type]` grammar, the sign/prefix rules, and
the thousands-grouping-with-zero-fill behaviour. Verified against the live `format()` builtin across
46k+ operations and against CPython's own test suite.

## Usage

```rust
use pyformat_rs::{format_int, format_str};

assert_eq!(format_int(255, "#06x").unwrap(), "0x00ff");
assert_eq!(format_int(1234567, ",").unwrap(), "1,234,567");
assert_eq!(format_int(-42, "=8").unwrap(), "-     42");
assert_eq!(format_int(1234, "010,").unwrap(), "00,001,234"); // grouping + zero fill
assert_eq!(format_int(65, "c").unwrap(), "A");
assert_eq!(format_str("hello", "^11").unwrap(), "   hello   ");
assert_eq!(format_str("hello", ".3").unwrap(), "hel");

// invalid specs raise, exactly where CPython's ValueError does
assert!(format_str("hi", "d").is_err());
assert!(format_int(42, ".2").is_err());
```

## Installation

```sh
cargo add pyformat-rs
```

Requires a Rust toolchain with 2024-edition support (Rust 1.85 or newer). No dependencies.

## What it covers (Layer 1)

- **`format_int`** (i128) - the `d`/`b`/`o`/`x`/`X`/`c` presentation types and "no type"; fill +
  align (`<` `>` `^` `=`), `+`/`-`/space sign, `#` alternate prefix, `0` zero-fill, width, `,` and
  `_` grouping, and the exact CPython zero-fill-with-grouping algorithm
  (`format(1234, "08,") == "0,001,234"`).
- **`format_str`** - fill + align, width, `.precision` truncation, type `s`, and the `0`-flag fill.
- The full spec parser, including the `[[fill]align]` rule (a fill char is only recognised when
  followed by an align char) and every `ValueError` CPython raises for an invalid spec / value
  (sign on a string, precision on an int, `,` with `x`, both `,` and `_`, sign with `c`, ...).

## How it matches CPython

- **The `0` flag** sets fill to `'0'` only when a fill was not given explicitly, and align to `'='`
  only when an align was not given - so `*<09` keeps the `*` fill while `=+09` zero-fills.
- **Zero-fill with grouping** grows the digit count until the grouped field reaches the requested
  width without a leading separator, which is why the width can overshoot
  (`format(1, "08,") == "0,000,001"`, 9 chars wide).
- **`c`** has no sign or prefix, so `=` padding coincides with right alignment, and an explicit sign
  is a `ValueError`.

## Out of scope (this layer)

- Float presentation types (`e`/`f`/`g`/`G`/`%`), which on an `int` promote it to `float` - Layer 2
  (they ride CPython's dtoa rounding).
- The locale type `n` (locale-dependent, not portable).
- Arbitrary-precision ints (the input is an `i128`) and lone-surrogate codepoints for `c`.
- The `str.format` replacement-field grammar (`{0.attr!r:>10}`) - a later layer.

## Verification

1. **CPython's own test vectors** - [`tests/cpython_vectors.rs`](tests/cpython_vectors.rs) lifts
   rows straight from `Lib/test/test_types.py::test_int__format__`. Run with `cargo test`, no
   Python needed.
2. **A live differential** - [`difftest.py`](difftest.py) throws a curated corpus plus a seeded
   fuzzer (random specs and values) at both this crate and Python's `format()` (CPython 3.13.1) and
   fails on any divergence. The current suite runs 46k+ operations.

```sh
cargo test
cargo build
python difftest.py     # prints "ALL MATCH - N operations agree ..." on success
```

## License

Licensed under the [MIT License](LICENSE-MIT).
