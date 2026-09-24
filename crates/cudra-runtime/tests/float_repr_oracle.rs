// Compara float_repr contra o repr() real do CPython 3.12 para todos os
// casos em float_repr_ref.txt (gerado por gen_refs.py).
use cudra_runtime::float_repr;

const REF: &str = include_str!("float_repr_ref.txt");

#[test]
fn float_repr_matches_cpython_oracle() {
    let mut failures = Vec::new();
    let mut total = 0usize;

    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        total += 1;
        let mut parts = line.splitn(2, '\t');
        let hexbits = parts.next().expect("linha sem hex");
        let expected = parts.next().expect("linha sem repr");
        let bits = u64::from_str_radix(hexbits, 16).expect("hex invalido no oraculo");
        let x = f64::from_bits(bits);
        let got = float_repr(x);
        if got != expected {
            failures.push(format!(
                "linha {}: bits={hexbits} esperado={expected:?} obtido={got:?}",
                lineno + 1
            ));
        }
    }

    assert!(total > 0, "ficheiro de referencia vazio");
    assert!(
        total >= 5000,
        "esperava pelo menos 5000 casos no oraculo, encontrei {total}"
    );

    if !failures.is_empty() {
        let shown: Vec<&str> = failures.iter().take(20).map(String::as_str).collect();
        panic!(
            "{} diferenca(s) em {} casos; primeiras {}:\n{}",
            failures.len(),
            total,
            shown.len(),
            shown.join("\n")
        );
    }
}
