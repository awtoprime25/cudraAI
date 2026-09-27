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
fn ui_cases_match_expected_stderr() {
    let root = workspace_root();
    let ui_dir = root.join("tests").join("ui");
    let bless = std::env::var("BLESS").is_ok();

    let mut cases: Vec<PathBuf> = fs::read_dir(&ui_dir)
        .unwrap_or_else(|e| panic!("não consegui ler {}: {e}", ui_dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "cdr").unwrap_or(false))
        .collect();
    cases.sort();

    assert!(
        cases.len() >= 40,
        "esperavam-se pelo menos 40 casos em tests/ui, há {}",
        cases.len()
    );

    let mut failures = Vec::new();
    for cdr_path in &cases {
        let stderr_path = cdr_path.with_extension("stderr");
        let rel = cdr_path.strip_prefix(&root).unwrap();

        let out = Command::new(bin())
            .arg("check")
            .arg(rel)
            .current_dir(&root)
            .output()
            .unwrap();
        let actual = String::from_utf8_lossy(&out.stderr).to_string();

        if out.status.code() != Some(1) {
            failures.push(format!(
                "{}: esperava-se exit=1, veio {:?}\nstderr:\n{}",
                rel.display(),
                out.status.code(),
                actual
            ));
            continue;
        }

        if bless {
            fs::write(&stderr_path, &actual).unwrap();
            continue;
        }

        let expected = fs::read_to_string(&stderr_path).unwrap_or_else(|_| {
            panic!(
                "falta {} (corre `BLESS=1 cargo test -p cudra-driver --test ui` para gerar)",
                stderr_path.display()
            )
        });
        if expected != actual {
            failures.push(format!(
                "{}: stderr não bate com o esperado (corre com BLESS=1 para atualizar)\n--- esperado ---\n{expected}\n--- obtido ---\n{actual}",
                rel.display()
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
