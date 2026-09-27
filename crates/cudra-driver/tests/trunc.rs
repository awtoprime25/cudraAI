//! Fuzz de truncagem (criterio do §12): o lexer, o parser e a sema correm
//! sobre cada ficheiro de teste cortado em TODAS as fronteiras de caracter
//! UTF-8, em processo e sem processo novo por corte. Nenhum corte pode
//! provocar um panic.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

fn cdr_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let mut files = Vec::new();
    for dir in [
        "tests/ui",
        "tests/check-ok",
        "tests/run",
        "tests/run-expect",
    ] {
        let path = root.join(dir);
        let mut here: Vec<PathBuf> = std::fs::read_dir(&path)
            .unwrap_or_else(|e| panic!("não consegui ler {}: {e}", path.display()))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|e| e == "cdr").unwrap_or(false))
            .collect();
        here.sort();
        assert!(!here.is_empty(), "{dir} está vazio");
        files.extend(here);
    }
    files
}

#[test]
fn truncating_any_test_program_never_panics() {
    let files = cdr_files();
    let mut cuts = 0usize;
    for file in &files {
        let src = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("{} não é UTF-8: {e}", file.display()));
        let mut offsets: Vec<usize> = src.char_indices().map(|(i, _)| i).collect();
        offsets.push(src.len());
        for cut in offsets {
            let piece = &src[..cut];
            let name = file.display().to_string();
            let result = catch_unwind(AssertUnwindSafe(|| {
                let _ = cudra_driver::check_source(&name, piece, false);
            }));
            assert!(result.is_ok(), "panic ao truncar {name} em {cut} bytes");
            cuts += 1;
        }
    }
    assert!(
        cuts > 5_000,
        "o fuzz devia cobrir muitos cortes, cobriu {cuts}"
    );
}
