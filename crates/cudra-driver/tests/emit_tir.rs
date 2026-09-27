use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

fn write_tmp(name: &str, contents: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cudrac_tir_{}_{}.cdr", std::process::id(), name));
    fs::write(&path, contents).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

#[test]
fn emit_tir_arithmetic_casts() {
    let path = write_tmp(
        "arith",
        b"def f(a: int, b: float) -> float:\n    x = a + 1\n    y = x / 2\n    return y * b\nprint(f(3, 2.5))\n",
    );
    let out = run(&["build", path.to_str().unwrap(), "--emit=tir"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn emit_tir_if_while_break() {
    let path = write_tmp(
        "if_while",
        b"def f(a: int) -> int:\n    if a > 0:\n        y = 1\n    else:\n        y = 2\n    while True:\n        y += 1\n        if y > 5:\n            break\n    return y\nprint(f(1))\n",
    );
    let out = run(&["build", path.to_str().unwrap(), "--emit=tir"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn emit_tir_for_and_mutual_recursion() {
    let path = write_tmp(
        "for_recursion",
        b"def a(n: int) -> int:\n    if n <= 0:\n        return 0\n    return b(n - 1)\ndef b(n: int) -> int:\n    return a(n)\nfor i in range(3):\n    print(i)\nprint(a(3))\n",
    );
    let out = run(&["build", path.to_str().unwrap(), "--emit=tir"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn emit_tir_calls_and_print() {
    let path = write_tmp(
        "calls",
        b"def add(a: int, b: int) -> int:\n    return a + b\nprint(add(1, 2), add(3, 4))\n",
    );
    let out = run(&["build", path.to_str().unwrap(), "--emit=tir"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn emit_tir_intrinsics_abs_and_args() {
    let path = write_tmp(
        "intrinsics",
        b"import cudra\nx = cudra.arg_int(1)\ny = cudra.arg_float(2)\nprint(abs(x), abs(y))\n",
    );
    let out = run(&["build", path.to_str().unwrap(), "--emit=tir"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}
