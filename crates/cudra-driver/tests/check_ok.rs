use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn check_ok_cases_exit_zero() {
    let root = workspace_root();
    let dir = root.join("tests").join("check-ok");

    let mut cases: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("não consegui ler {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "cdr").unwrap_or(false))
        .collect();
    cases.sort();

    assert!(
        cases.len() >= 20,
        "esperavam-se pelo menos 20 casos em tests/check-ok, há {}",
        cases.len()
    );

    let mut failures = Vec::new();
    for cdr_path in &cases {
        let rel = cdr_path.strip_prefix(&root).unwrap();
        let out = Command::new(bin())
            .arg("check")
            .arg(rel)
            .current_dir(&root)
            .output()
            .unwrap();
        if out.status.code() != Some(0) {
            failures.push(format!(
                "{}: esperava-se exit=0, veio {:?}\nstderr:\n{}",
                rel.display(),
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
