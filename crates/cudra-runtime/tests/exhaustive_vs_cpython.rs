// Teste exaustivo (ignorado por omissao, corre so com --ignored): compara
// python_repr/parse_py_int/parse_py_float com repr()/int()/float() do
// CPython real, para todos os codepoints 0..=0x10FFFF. Uma unica invocacao
// de python3 gera uma linha TSV por codepoint (repr() nunca contem um tab
// ou newline literal, por isso o formato e seguro); as comparacoes ficam
// todas em memoria do lado Rust, para caber bem dentro de 60s em release.
use cudra_runtime::{float_repr, parse_py_float, parse_py_int, python_repr};
use std::process::Command;

const PY_SCRIPT: &str = r#"
import sys
out = []
for cp in range(0, 0x110000):
    ch = chr(cp)
    r = repr(ch)
    if 0xD800 <= cp <= 0xDFFF:
        out.append(f"{cp:x}\t{r}\t-\t-\t-")
        continue
    try:
        int_single = str(int(ch))
    except ValueError:
        int_single = "E"
    s5 = ch + "5"
    try:
        int_combined = str(int(s5))
    except ValueError:
        int_combined = "E"
    try:
        float_combined = repr(float(s5))
    except ValueError:
        float_combined = "E"
    out.append(f"{cp:x}\t{r}\t{int_single}\t{int_combined}\t{float_combined}")
sys.stdout.write("\n".join(out))
"#;

fn take20(v: &[String]) -> String {
    v.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
}

#[test]
#[ignore]
fn exhaustive_vs_cpython() {
    let output = Command::new("python3")
        .arg("-c")
        .arg(PY_SCRIPT)
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .expect("falha ao invocar python3 -- precisa de python3 no PATH");
    assert!(
        output.status.success(),
        "python3 falhou: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("saida do python3 nao era UTF-8 valida");

    let mut repr_failures = Vec::new();
    let mut int_single_failures = Vec::new();
    let mut int_combined_failures = Vec::new();
    let mut float_failures = Vec::new();
    let mut total = 0usize;

    for line in text.lines() {
        total += 1;
        let mut parts = line.splitn(5, '\t');
        let cp_hex = parts.next().expect("linha sem cp");
        let repr_field = parts.next().expect("linha sem repr");
        let int_single_field = parts.next().expect("linha sem int_single");
        let int_combined_field = parts.next().expect("linha sem int_combined");
        let float_combined_field = parts.next().expect("linha sem float_combined");

        let cp = u32::from_str_radix(cp_hex, 16).expect("cp hex invalido");

        let our_repr = python_repr(&[cp]);
        if our_repr != repr_field {
            repr_failures.push(format!(
                "cp=U+{cp_hex}: esperado={repr_field:?} obtido={our_repr:?}"
            ));
        }

        if int_single_field == "-" {
            continue; // surrogate: so o repr e comparado.
        }

        let our_int_single = parse_py_int(&[cp]);
        let int_single_ok = if int_single_field == "E" {
            our_int_single.is_err()
        } else {
            matches!(our_int_single, Ok(v) if v.to_string() == int_single_field)
        };
        if !int_single_ok {
            int_single_failures.push(format!(
                "cp=U+{cp_hex}: esperado={int_single_field:?} obtido={:?}",
                our_int_single.ok()
            ));
        }

        let combined = [cp, '5' as u32];
        let our_int_combined = parse_py_int(&combined);
        let int_combined_ok = if int_combined_field == "E" {
            our_int_combined.is_err()
        } else {
            matches!(our_int_combined, Ok(v) if v.to_string() == int_combined_field)
        };
        if !int_combined_ok {
            int_combined_failures.push(format!(
                "cp=U+{cp_hex}: esperado={int_combined_field:?} obtido={:?}",
                our_int_combined.ok()
            ));
        }

        let our_float_combined = parse_py_float(&combined);
        let float_ok = if float_combined_field == "E" {
            our_float_combined.is_none()
        } else {
            matches!(our_float_combined, Some(v) if float_repr(v) == float_combined_field)
        };
        if !float_ok {
            float_failures.push(format!(
                "cp=U+{cp_hex}: esperado={float_combined_field:?} obtido={:?}",
                our_float_combined.map(float_repr)
            ));
        }
    }

    assert!(total > 1_000_000, "esperava ~1.1M linhas, recebi {total}");

    if !repr_failures.is_empty()
        || !int_single_failures.is_empty()
        || !int_combined_failures.is_empty()
        || !float_failures.is_empty()
    {
        panic!(
            "repr: {} diferenca(s), int(1 char): {} diferenca(s), int(+5): {} diferenca(s), \
             float(+5): {} diferenca(s), em {total} codepoints.\n\
             -- repr --\n{}\n-- int(1 char) --\n{}\n-- int(+5) --\n{}\n-- float(+5) --\n{}",
            repr_failures.len(),
            int_single_failures.len(),
            int_combined_failures.len(),
            float_failures.len(),
            take20(&repr_failures),
            take20(&int_single_failures),
            take20(&int_combined_failures),
            take20(&float_failures),
        );
    }
}
