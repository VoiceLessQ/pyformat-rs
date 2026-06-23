r"""Differential test: pyformat-rs vs Python's builtin format().

Feeds identical (value, spec) pairs to the `diff` binary and to `format()`, checking every result
agrees. A curated corpus covers the documented edges (sign, prefix, grouping + zero-fill, align,
precision); a seeded fuzzer then throws random specs and values at both sides.

Layer 1 covers `str` and `int` (i128 range). Excluded here (Layer 2): float presentation types on
ints (e/f/g/E/F/G/%), the locale type `n`, ints outside i128, and lone-surrogate codepoints for 'c'.

Run from the pyformat-rs/ folder after `cargo build`:
    python difftest.py
"""

import os
import random
import re
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RUST_BIN = os.path.join(HERE, "target", "debug", "diff.exe" if os.name == "nt" else "diff")

US = "\x1f"
SEED = 20260623
FUZZ_INT = 30000
FUZZ_STR = 15000

I128_MAX = (1 << 127) - 1
I128_MIN = -(1 << 127)

# Int presentation types we cover (float types e/f/g/E/F/G/% and locale n are Layer 2).
INT_TYPES = ["", "d", "b", "o", "x", "X", "c"]
STR_TYPES = ["", "s"]


def expect(line):
    f = line.split("\t")
    op = f[0]
    try:
        if op == "fi":
            v = int(f[1])
            if not (I128_MIN <= v <= I128_MAX):
                return "ERR"
            # Skip the cases Layer 1 defers, mirroring the harness's ERR.
            return US + format(v, f[2])
        if op == "fs":
            return US + format(f[1], f[2])
        if op == "ff":
            v = struct.unpack("<d", struct.pack("<Q", int(f[1])))[0]
            return US + format(v, f[2])
    except (ValueError, OverflowError):
        return "ERR"
    raise AssertionError(op)


def fbits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def curated():
    cmds = []
    int_specs = [
        "", "d", "b", "o", "x", "X", "c", "+", "-", " ", "#x", "#X", "#o", "#b", "#06x",
        "08", "08,", "08_", "010,", "011,", "=8", "=+09", "<8", ">8", "^9", "*^9", "*<8",
        ",", "_", "_x", "_b", ",d", "010x", "#010x", "#012_x", " x", "+,", "010,", "05",
        ".2", "z", ",x", ",_d", "{^5", "<09", "*<09", "=09", ">09",
    ]
    int_vals = [0, 1, -1, 42, -42, 255, -255, 1234, -1234, 1234567, 65, 8, 5,
                I128_MAX, I128_MIN, 0x1F600, 128512, -7]
    for v in int_vals:
        for spec in int_specs:
            cmds.append("\t".join(["fi", str(v), spec]))

    str_specs = ["", "s", "<8", ">8", "^8", ".3", "8.3", "*<8", "*^10", "08", "10",
                 "d", "+", ",", "=8", "#", "_", ".0", "x", "<.4"]
    str_vals = ["", "hi", "hello", "a", "unicode-éñ", "\U0001F600x", "  spaces  "]
    for v in str_vals:
        for spec in str_specs:
            cmds.append("\t".join(["fs", v, spec]))

    cmds += curated_float()
    return cmds


def curated_float():
    cmds = []
    fspecs = [
        "", "f", "e", "E", "g", "G", "%", ".2f", ".0f", ".2e", ".0e", "#.0e", "#.0f", "#g",
        ".3g", ".1%", "+", " ", "+.2f", " .2f", "z.2f", "010.2f", "012,.2f", ",", "_",
        ",.2f", ".3", ".0", ".6", ".17g", "010", ">8", "^10", "*<12.3f", "=+012.2e",
        ".3e", ".10f", "g", "20.10g", "#.4g", "+#012.2f",
    ]
    fvals = [0.0, -0.0, 1.0, -1.0, 0.1, 0.5, 2.5, 3.5, 3.14159, 42.0, -42.0, 1234.5,
             1234567.891, 1e16, 1e17, 1e-4, 1e-5, 1e100, 1e-100, 0.0001234, 123456.0,
             2.675, 9.999e-5, 100000.0, 1000000.0, 9999999999999998.0,
             float("inf"), float("-inf"), float("nan")]
    for v in fvals:
        for spec in fspecs:
            cmds.append("\t".join(["ff", str(fbits(v)), spec]))

    # int -> float promotion
    for spec in ["e", "f", ".2g", "%", ".3e", "012.2f"]:
        for v in [0, 1, -1, 42, 1234567, -1234567]:
            cmds.append("\t".join(["fi", str(v), spec]))
    return cmds


def rand_int_spec(rng):
    parts = []
    if rng.random() < 0.5:
        align = rng.choice("<>^=")
        if rng.random() < 0.5:
            fill = rng.choice("*#@0 xY.+")  # printable, no tab/newline
            parts.append(fill + align)
        else:
            parts.append(align)
    if rng.random() < 0.5:
        parts.append(rng.choice("+- "))
    if rng.random() < 0.3:
        parts.append("#")
    if rng.random() < 0.4:
        parts.append("0")
    if rng.random() < 0.7:
        parts.append(str(rng.randint(0, 14)))
    if rng.random() < 0.4:
        parts.append(rng.choice(",_"))
    if rng.random() < 0.85:
        parts.append(rng.choice(INT_TYPES))
    return "".join(parts)


def rand_str_spec(rng):
    parts = []
    if rng.random() < 0.6:
        align = rng.choice("<>^=")
        if rng.random() < 0.5:
            parts.append(rng.choice("*#@0 xY.") + align)
        else:
            parts.append(align)
    # occasionally inject the int-only pieces to exercise ERR parity
    if rng.random() < 0.2:
        parts.append(rng.choice("+- #,_"))
    if rng.random() < 0.6:
        parts.append(str(rng.randint(0, 12)))
    if rng.random() < 0.4:
        parts.append("." + str(rng.randint(0, 8)))
    if rng.random() < 0.7:
        parts.append(rng.choice(STR_TYPES + ["d", "x"]))
    return "".join(parts)


def rand_int_val(rng):
    r = rng.random()
    if r < 0.6:
        return rng.randint(-100000, 100000)
    if r < 0.8:
        return rng.randint(I128_MIN, I128_MAX)
    return rng.choice([0, 1, -1, I128_MAX, I128_MIN])


def fuzz(rng):
    cmds = []
    for _ in range(FUZZ_INT):
        spec = rand_int_spec(rng)
        # For 'c', keep the value to a representable, non-surrogate codepoint.
        if spec.endswith("c"):
            v = rng.choice([rng.randint(0, 0xD7FF), rng.randint(0xE000, 0x10FFFF)])
        else:
            v = rand_int_val(rng)
        cmds.append("\t".join(["fi", str(v), spec]))

    sample_strs = ["", "a", "hi", "hello world", "éñü", "\U0001F600",
                   "tab\tless", "1234567"]
    for _ in range(FUZZ_STR):
        spec = rand_str_spec(rng)
        v = rng.choice(sample_strs)
        if "\t" in v:
            v = v.replace("\t", "_")
        cmds.append("\t".join(["fs", v, spec]))

    cmds += fuzz_float(rng)
    return cmds


def rand_float(rng):
    r = rng.random()
    if r < 0.45:
        return rng.uniform(-1e6, 1e6)
    if r < 0.65:
        return rng.uniform(-1.0, 1.0) * 10.0 ** rng.randint(-15, 15)
    if r < 0.8:
        return float(rng.randint(-10**12, 10**12))
    return rng.choice([0.0, -0.0, float("inf"), float("-inf"), float("nan"),
                       0.1, 2.675, 1e16, 1e-5, 123456.789, 0.5, 2.5])


def rand_float_spec(rng):
    parts = []
    if rng.random() < 0.5:
        align = rng.choice("<>^=")
        if rng.random() < 0.5:
            parts.append(rng.choice("*#@0 xY.+") + align)
        else:
            parts.append(align)
    if rng.random() < 0.4:
        parts.append(rng.choice("+- "))
    if rng.random() < 0.15:
        parts.append("z")
    if rng.random() < 0.3:
        parts.append("#")
    if rng.random() < 0.4:
        parts.append("0")
    if rng.random() < 0.7:
        parts.append(str(rng.randint(0, 16)))
    if rng.random() < 0.3:
        parts.append(rng.choice(",_"))
    if rng.random() < 0.7:
        parts.append("." + str(rng.randint(0, 17)))
    if rng.random() < 0.85:
        parts.append(rng.choice(["e", "E", "f", "F", "g", "G", "%", ""]))
    return "".join(parts)


def fuzz_float(rng):
    cmds = []
    FUZZ_FLOAT = 40000
    for _ in range(FUZZ_FLOAT):
        cmds.append("\t".join(["ff", str(fbits(rand_float(rng))), rand_float_spec(rng)]))
    return cmds


def _grab_float(s):
    m = re.search(r"[-+]?[0-9][0-9.eE+]*", s.replace("_", ""))
    return float(m.group()) if m else None


def benign_repr_tie(cmd, exp, got):
    """A float `repr` shortest-tie: CPython's dtoa and Rust's shortest-repr can pick opposite
    equally-short, equally-round-tripping decimals at an exact decimal midpoint. Accept only when
    the surrounding structure is byte-identical (so padding/sign/exponent bugs are NOT masked) and
    both numeric tokens round-trip to the very same double."""
    if not cmd.startswith("ff\t"):
        return False
    # digit-skeletons must match: same length, fill, sign, exponent layout - only digits differ.
    if re.sub(r"[0-9]", "#", exp) != re.sub(r"[0-9]", "#", got):
        return False
    a, b = _grab_float(exp), _grab_float(got)
    return a is not None and b is not None and struct.pack("<d", a) == struct.pack("<d", b)


def main():
    if not os.path.exists(RUST_BIN):
        sys.exit(f"missing {RUST_BIN} - run `cargo build` first")
    rng = random.Random(SEED)
    cmds = curated() + fuzz(rng)

    proc = subprocess.run([RUST_BIN], input="\n".join(cmds), capture_output=True, text=True,
                          encoding="utf-8")
    if proc.returncode != 0:
        sys.exit(f"rust diff binary failed:\n{proc.stderr}")

    rust = proc.stdout.split("\n")
    if rust and rust[-1] == "":
        rust.pop()
    if len(rust) != len(cmds):
        sys.exit(f"line count mismatch: {len(rust)} rust vs {len(cmds)} commands")

    mismatches = []
    for cmd, got in zip(cmds, rust):
        exp = expect(cmd)
        if exp != got and not benign_repr_tie(cmd, exp, got):
            mismatches.append((cmd, exp, got))

    if mismatches:
        print(f"{len(mismatches)} mismatches (of {len(cmds)}):")
        for cmd, exp, got in mismatches[:30]:
            print(ascii(f"  op={cmd!r}\n    python={exp!r}\n    rust  ={got!r}"))
        sys.exit("\nMISMATCHES FOUND.")

    print(f"ALL MATCH - {len(cmds)} operations agree with format() "
          f"(Python {sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}).")


if __name__ == "__main__":
    main()
