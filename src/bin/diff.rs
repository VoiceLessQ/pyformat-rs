//! Differential CLI. Each stdin line is `op<TAB>value<TAB>spec`. `difftest.py` feeds the same input
//! here and to Python's `format()` and compares line by line. A success result is prefixed with a
//! US byte (\x1f) so a formatted string that happens to be "ERR" never collides with an error line.

use std::io::{self, Read, Write};

use pyformat_rs::{format_int, format_str};

const US: char = '\u{1f}';

fn wrap(r: Result<String, impl std::fmt::Debug>) -> String {
    match r {
        Ok(s) => format!("{US}{s}"),
        Err(_) => "ERR".to_string(),
    }
}

fn dispatch(line: &str) -> String {
    let f: Vec<&str> = line.split('\t').collect();
    match f[0] {
        "fi" => match f[1].parse::<i128>() {
            Ok(v) => wrap(format_int(v, f[2])),
            Err(_) => "ERR".to_string(), // out of i128 range: Layer 1 bound
        },
        "fs" => wrap(format_str(f[1], f[2])),
        other => panic!("unknown op {other:?}"),
    }
}

fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).expect("read stdin");
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for line in input.lines() {
        writeln!(out, "{}", dispatch(line)).expect("write");
    }
}
