# pyformat-rs

Rust port of Python's string formatting (CPython 3.13): the
[format-spec mini-language](https://docs.python.org/3/library/string.html#format-specification-mini-language)
for `str`/`int`/`float`, plus the `str.format` replacement-field grammar. `format_str`,
`format_int`, and `format_float` mirror `format(value, spec)` byte-for-byte (the full
`[[fill]align][sign][#][0][width][grouping][.precision][type]` grammar and the float presentation
types using CPython's exact rounding); `str_format` mirrors `"...".format(*args, **kwargs)`.
Verified against the live `format()`/`str.format` across 112k+ operations and against CPython's own
test suite.

## Usage

```rust
use pyformat_rs::{format_float, format_int, format_str, str_format, Value};

// "...".format(*args): positional/keyword/auto numbering, !r/!s/!a, nested specs
let args = [Value::Int(7), Value::Str("hi".into())];
assert_eq!(str_format("{0} {1!r:>6}", &args, &[]).unwrap(), "7   'hi'");

assert_eq!(format_int(255, "#06x").unwrap(), "0x00ff");
assert_eq!(format_int(1234567, ",").unwrap(), "1,234,567");
assert_eq!(format_int(-42, "=8").unwrap(), "-     42"); // pad after the sign
assert_eq!(format_int(1234, "010,").unwrap(), "00,001,234"); // grouping + zero fill
assert_eq!(format_int(65, "c").unwrap(), "A");
assert_eq!(format_str("hello", "^11").unwrap(), "   hello   ");
assert_eq!(format_float(3.14159, ".2f").unwrap(), "3.14");
assert_eq!(format_float(1234.5, "012,.2f").unwrap(), "0,001,234.50");
assert_eq!(format_float(0.5, "%").unwrap(), "50.000000%");
assert_eq!(format_float(1e16, "").unwrap(), "1e+16"); // repr shortest

// invalid specs raise, exactly where CPython's ValueError does
assert!(format_str("hi", "d").is_err());
assert!(format_int(42, ".2").is_err());
```

## Installation

```sh
cargo add pyformat-rs
```

Requires a Rust toolchain with 2024-edition support (Rust 1.85 or newer). No dependencies.

## What it covers

- **`format_int`** (i128) - the `d`/`b`/`o`/`x`/`X`/`c` types and "no type"; fill + align
  (`<` `>` `^` `=`), `+`/`-`/space sign, `#` alternate prefix, `0` zero-fill, width, `,` and `_`
  grouping, and the exact CPython zero-fill-with-grouping algorithm
  (`format(1234, "08,") == "0,001,234"`). Float types (`e`/`f`/`g`/`%`) promote the int to float.
- **`format_float`** - the `e`/`E`/`f`/`F`/`g`/`G`/`%` types and `repr` (no type), with `inf`/`nan`
  (and their upper-case forms), `z` negative-zero coercion, `#` alternate form, grouping and
  zero-fill on the integer part, and CPython's exact rounding (round-half-even, shortest repr).
- **`format_str`** - fill + align, width, `.precision` truncation, type `s`, and the `0`-flag fill.
- **`str_format`** - `"...".format(*args, **kwargs)` over a scalar [`Value`] model
  (int/float/str/bool/None): positional / keyword / auto field numbering (and the
  cannot-switch error), `!r`/`!s`/`!a` conversions, nested replacement fields in the spec
  (`{:{}.{}}`), brace escapes, and the `bool`/`None` `__format__` rules.
- The full spec parser and every `ValueError` CPython raises for an invalid spec / value (sign on a
  string, precision on an int, `,` with `x`, both `,` and `_`, sign with `c`, ...).

## How it matches CPython

- **Rounding** uses Rust's correctly-rounded float formatting, which agrees with CPython's dtoa
  digit-for-digit (round-half-even for fixed/scientific, shortest for `repr`).
- **The `0` flag** sets fill to `'0'` only when a fill was not given explicitly, and align to `'='`
  only when an align was not given - so `*<09` keeps the `*` fill while `=+09` zero-fills.
- **Zero-fill with grouping** grows the digit count until the grouped field reaches the requested
  width without a leading separator, which is why the width can overshoot
  (`format(1, "08,") == "0,000,001"`, 9 chars wide).
- **`c`** has no sign or prefix, so `=` padding coincides with right alignment, and an explicit sign
  is a `ValueError`.

The one tolerated byte-level difference: at a double that is an exact decimal midpoint, CPython's
dtoa and Rust's shortest-repr can pick opposite (equally short, equally round-tripping) decimals for
`repr` - e.g. `90593674776370.12` vs `.13`. Both round-trip to the same `f64`.

## Out of scope

- The locale type `n` (locale-dependent, not portable).
- Arbitrary-precision ints (the input is an `i128`), precision above 9999 (Rust's `format!` caps
  near 16384), and lone-surrogate codepoints for `c`.
- `[index]` / `.attr` access inside replacement fields (`{0[0]}`, `{0.real}`) - the `str.format`
  field name is a positional index or keyword name only.

## Verification

1. **CPython's own test vectors** - [`tests/cpython_vectors.rs`](tests/cpython_vectors.rs) lifts
   rows straight from CPython's test suite: `Lib/test/test_types.py::test_int__format__` and
   `test_float__format__`, and `Lib/test/test_str.py::test_format` (positional fields, brace
   escapes, string-spec edges, null-byte fill, a 10000-wide field). Run with `cargo test`, no Python
   needed.
2. **A live differential** - [`difftest.py`](difftest.py) throws a curated corpus plus a seeded
   fuzzer at both this crate and Python's `format()` / `str.format` (CPython 3.13.1) and fails on
   any divergence. Each run is **112,000+ operations** across all four entry points:

   | entry point | ops/run |
   |---|---|
   | `format_float` (`ff`) | ~41,000 |
   | `format_int` (`fi`) | ~31,000 |
   | `str_format` (`sf`) | ~25,000 |
   | `format_str` (`fs`) | ~15,000 |

   About 77k produce real output (checked byte-for-byte) and 26k exercise error paths (the crate
   raises exactly where CPython does). Floats are passed as raw IEEE-754 bits, and the fuzzer draws
   arbitrary finite doubles (denormals, every exponent, power-of-10 and half-way boundaries) to
   hammer the rounding and `repr` paths. The suite is **seedable** and has been run clean across 20+
   seeds (~2.4M operations); that adversarial pass is what caught the `%`-overflow-to-infinity bug.

```sh
cargo test                 # CPython-derived vectors, no Python needed
cargo build
python difftest.py         # default seed; prints "ALL MATCH - N operations agree ..."
python difftest.py 42      # any seed, for a multi-seed CI sweep
```

## License

Licensed under the [MIT License](LICENSE-MIT).
