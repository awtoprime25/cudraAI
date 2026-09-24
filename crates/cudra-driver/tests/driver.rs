use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

fn write_tmp(name: &str, contents: &[u8]) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cudrac_test_{}_{}.cdr", std::process::id(), name));
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
fn check_ok_exits_0() {
    let path = write_tmp("check_ok", b"x = 1\n");
    let out = run(&["check", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn check_syntax_error_exits_1() {
    let path = write_tmp("check_err", b"x = \n");
    let out = run(&["check", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.is_empty());
}

#[test]
fn check_missing_file_exits_2() {
    let out = run(&["check", "/caminho/que/nao/existe/xyz.cdr"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn check_non_utf8_exits_1() {
    let path = write_tmp("check_binary", &[0xFF, 0xFE, 0x00, 0x01]);
    let out = run(&["check", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn build_without_emit_exits_2() {
    let path = write_tmp("build_no_emit", b"x = 1\n");
    let out = run(&["build", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ainda não implementado"));
}

#[test]
fn run_exits_2() {
    let path = write_tmp("run_stub", b"x = 1\n");
    let out = run(&["run", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ainda não implementado"));
}

#[test]
fn build_emit_tokens_ok_exits_0() {
    let path = write_tmp("emit_tokens_ok", b"x = 1\n");
    let out = run(&["build", path.to_str().unwrap(), "--emit=tokens"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn build_emit_ast_ok_exits_0() {
    let path = write_tmp(
        "emit_ast_ok",
        b"def f(a: int, b: float) -> float:\n    return a + b\n",
    );
    let out = run(&["build", path.to_str().unwrap(), "--emit=ast"]);
    assert_eq!(out.status.code(), Some(0));
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn build_emit_ast_error_exits_1_and_prints_nothing_on_stdout() {
    let path = write_tmp("emit_ast_err", b"x = \n");
    let out = run(&["build", path.to_str().unwrap(), "--emit=ast"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).is_empty());
    assert!(!out.stderr.is_empty());
}
