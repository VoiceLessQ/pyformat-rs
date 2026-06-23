//! Differential CLI. Each stdin line is `op<TAB>value<TAB>spec`. `difftest.py` feeds the same input
//! here and to Python's `format()` and compares line by line. A success result is prefixed with a
//! US byte (\x1f) so a formatted string that happens to be "ERR" never collides with an error line.

use std::io::{self, Read, Write};

use pyformat_rs::{Value, format_float, format_int, format_str, str_format};

const US: char = '\u{1f}';

/// Decode a tagged scalar: i<int>, f<u64 bits>, s<str>, T, F, N.
fn parse_val(tok: &str) -> Value {
    let (tag, rest) = tok.split_at(1);
    match tag {
        "i" => Value::Int(rest.parse().expect("int")),
        "f" => Value::Float(f64::from_bits(rest.parse::<u64>().expect("bits"))),
        "s" => Value::Str(rest.to_string()),
        "T" => Value::Bool(true),
        "F" => Value::Bool(false),
        "N" => Value::None,
        _ => panic!("bad value tag {tok:?}"),
    }
}

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
        // float passed as raw IEEE-754 bits (decimal u64) so both sides see identical values.
        "ff" => match f[1].parse::<u64>() {
            Ok(bits) => wrap(format_float(f64::from_bits(bits), f[2])),
            Err(_) => "ERR".to_string(),
        },
        // sf <template> <args> <kwargs>: args US-joined tagged values; kwargs US-joined name=value.
        "sf" => {
            let args: Vec<Value> =
                if f[2].is_empty() { vec![] } else { f[2].split(US).map(parse_val).collect() };
            let kwargs: Vec<(String, Value)> = if f[3].is_empty() {
                vec![]
            } else {
                f[3]
                    .split(US)
                    .map(|kv| {
                        let (k, v) = kv.split_once('=').expect("kwarg");
                        (k.to_string(), parse_val(v))
                    })
                    .collect()
            };
            wrap(str_format(f[1], &args, &kwargs))
        }
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
