// Compara os helpers numericos (int_truediv, float_floordiv, float_mod,
// fpow, cmp_int_float, float_to_int) contra o CPython 3.12, lendo
// num_ref.txt (JSON por linha, gerado por gen_refs.py). Mesmo estilo de
// mini-parser do arg_parse_oracle.rs: formato fixo, sem dependencias
// externas.
use cudra_runtime::{
    cmp_int_float, float_floordiv, float_mod, float_to_int, fpow, int_truediv, panic_message,
    PANIC_NEG_BASE_FRACTIONAL_POW,
};

const REF: &str = include_str!("num_ref.txt");

enum ResultField {
    HexBits(u64),
    Nan,
    Diverge,
    Error(String),
    Int(i64),
}

fn expect_literal(bytes: &[u8], pos: &mut usize, lit: &str) {
    let litb = lit.as_bytes();
    assert_eq!(
        &bytes[*pos..*pos + litb.len()],
        litb,
        "formato inesperado no oraculo em pos {pos}"
    );
    *pos += litb.len();
}

/// Le uma string JSON simples (sem escapes) ate ao proximo '"'. Suficiente
/// porque nenhum campo gerado por gen_refs.py para num_ref.txt contem aspas
/// ou barras invertidas.
fn parse_simple_string(bytes: &[u8], pos: &mut usize) -> String {
    assert_eq!(bytes[*pos], b'"', "esperava '\"' no oraculo");
    *pos += 1;
    let start = *pos;
    while bytes[*pos] != b'"' {
        *pos += 1;
    }
    let s = std::str::from_utf8(&bytes[start..*pos])
        .expect("utf8 invalido no oraculo")
        .to_string();
    *pos += 1;
    s
}

fn parse_i64_field(bytes: &[u8], pos: &mut usize) -> i64 {
    let start = *pos;
    if bytes[*pos] == b'-' {
        *pos += 1;
    }
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }
    let text = std::str::from_utf8(&bytes[start..*pos]).expect("numero invalido no oraculo");
    text.parse::<i64>().expect("numero fora de i64 no oraculo")
}

fn parse_hex_bits_field(bytes: &[u8], pos: &mut usize) -> u64 {
    let s = parse_simple_string(bytes, pos);
    u64::from_str_radix(&s, 16).expect("hex bits invalidos no oraculo")
}

fn parse_result_field(bytes: &[u8], pos: &mut usize) -> ResultField {
    match bytes[*pos] {
        b'"' => {
            let s = parse_simple_string(bytes, pos);
            if s == "nan" {
                ResultField::Nan
            } else if s == "DIVERGE" {
                ResultField::Diverge
            } else {
                ResultField::HexBits(
                    u64::from_str_radix(&s, 16).expect("hex bits invalidos no oraculo"),
                )
            }
        }
        b'{' => {
            expect_literal(bytes, pos, "{\"error\": ");
            let msg = parse_simple_string(bytes, pos);
            expect_literal(bytes, pos, "}");
            ResultField::Error(msg)
        }
        _ => ResultField::Int(parse_i64_field(bytes, pos)),
    }
}

fn expect_fn(bytes: &[u8], pos: &mut usize, name: &str) {
    expect_literal(bytes, pos, "{\"fn\": \"");
    expect_literal(bytes, pos, name);
    expect_literal(bytes, pos, "\"");
}

/// Compara um Result<f64, u32> nosso contra o campo "result" do oraculo.
/// nan so e comparado por is_nan(); -0.0 e 0.0 sao distinguidos via bits.
fn check_float_result(
    lineno: usize,
    desc: &str,
    ours: Result<f64, u32>,
    expected: &ResultField,
    failures: &mut Vec<String>,
) {
    let ok = match (&ours, expected) {
        (Ok(v), ResultField::HexBits(bits)) => v.to_bits() == *bits,
        (Ok(v), ResultField::Nan) => v.is_nan(),
        (Err(code), ResultField::Error(msg)) => panic_message(*code) == msg,
        _ => false,
    };
    if !ok {
        let got_desc = match ours {
            Ok(v) => format!("Ok({v:?} bits={:016x})", v.to_bits()),
            Err(code) => format!("Err({})", panic_message(code)),
        };
        failures.push(format!("linha {lineno}: {desc} -> obtido {got_desc}"));
    }
}

#[test]
fn int_truediv_matches_cpython_oracle() {
    let mut failures = Vec::new();
    let mut total = 0usize;
    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() || !line.contains("\"fn\": \"int_truediv\"") {
            continue;
        }
        total += 1;
        let bytes = line.as_bytes();
        let mut pos = 0usize;
        expect_fn(bytes, &mut pos, "int_truediv");
        expect_literal(bytes, &mut pos, ", \"a\": ");
        let a = parse_i64_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, ", \"b\": ");
        let b = parse_i64_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, ", \"result\": ");
        let expected = parse_result_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, "}");

        let ours = int_truediv(a, b);
        check_float_result(
            lineno + 1,
            &format!("int_truediv({a}, {b})"),
            ours,
            &expected,
            &mut failures,
        );
    }
    assert!(total > 0, "ficheiro de referencia sem casos de int_truediv");
    assert_oracle_ok("int_truediv", total, &failures);
}

#[test]
fn float_floordiv_matches_cpython_oracle() {
    let mut failures = Vec::new();
    let mut total = 0usize;
    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() || !line.contains("\"fn\": \"float_floordiv\"") {
            continue;
        }
        total += 1;
        let bytes = line.as_bytes();
        let mut pos = 0usize;
        expect_fn(bytes, &mut pos, "float_floordiv");
        expect_literal(bytes, &mut pos, ", \"a\": ");
        let a = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"b\": ");
        let b = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"result\": ");
        let expected = parse_result_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, "}");

        let ours = float_floordiv(a, b);
        check_float_result(
            lineno + 1,
            &format!("float_floordiv({a:?}, {b:?})"),
            ours,
            &expected,
            &mut failures,
        );
    }
    assert!(
        total > 0,
        "ficheiro de referencia sem casos de float_floordiv"
    );
    assert_oracle_ok("float_floordiv", total, &failures);
}

#[test]
fn float_mod_matches_cpython_oracle() {
    let mut failures = Vec::new();
    let mut total = 0usize;
    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() || !line.contains("\"fn\": \"float_mod\"") {
            continue;
        }
        total += 1;
        let bytes = line.as_bytes();
        let mut pos = 0usize;
        expect_fn(bytes, &mut pos, "float_mod");
        expect_literal(bytes, &mut pos, ", \"a\": ");
        let a = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"b\": ");
        let b = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"result\": ");
        let expected = parse_result_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, "}");

        let ours = float_mod(a, b);
        check_float_result(
            lineno + 1,
            &format!("float_mod({a:?}, {b:?})"),
            ours,
            &expected,
            &mut failures,
        );
    }
    assert!(total > 0, "ficheiro de referencia sem casos de float_mod");
    assert_oracle_ok("float_mod", total, &failures);
}

#[test]
fn fpow_matches_cpython_oracle() {
    let mut failures = Vec::new();
    let mut diverge = 0usize;
    let mut total = 0usize;
    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() || !line.contains("\"fn\": \"fpow\"") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut pos = 0usize;
        expect_fn(bytes, &mut pos, "fpow");
        expect_literal(bytes, &mut pos, ", \"x\": ");
        let x = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"y\": ");
        let y = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"result\": ");
        let expected = parse_result_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, "}");

        let ours = fpow(x, y);
        if matches!(expected, ResultField::Diverge) {
            // Divergencia documentada: x<0 com expoente fracionario. O
            // Python real devolve complex; nos so garantimos o nosso erro.
            diverge += 1;
            if !matches!(ours, Err(PANIC_NEG_BASE_FRACTIONAL_POW)) {
                failures.push(format!(
                    "linha {}: fpow({x:?}, {y:?}) devia divergir com ValueError, obtido {:?}",
                    lineno + 1,
                    ours
                ));
            }
            continue;
        }
        total += 1;
        check_float_result(
            lineno + 1,
            &format!("fpow({x:?}, {y:?})"),
            ours,
            &expected,
            &mut failures,
        );
    }
    assert!(total > 0, "ficheiro de referencia sem casos de fpow");
    assert!(
        diverge > 0,
        "ficheiro de referencia sem casos DIVERGE de fpow"
    );
    assert_oracle_ok("fpow", total, &failures);
}

#[test]
fn cmp_int_float_matches_cpython_oracle() {
    let mut failures = Vec::new();
    let mut total = 0usize;
    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() || !line.contains("\"fn\": \"cmp_int_float\"") {
            continue;
        }
        total += 1;
        let bytes = line.as_bytes();
        let mut pos = 0usize;
        expect_fn(bytes, &mut pos, "cmp_int_float");
        expect_literal(bytes, &mut pos, ", \"i\": ");
        let i = parse_i64_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, ", \"f\": ");
        let f = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"result\": ");
        let expected = match parse_result_field(bytes, &mut pos) {
            ResultField::Int(v) => v as i32,
            _ => panic!(
                "linha {}: resultado inesperado para cmp_int_float",
                lineno + 1
            ),
        };
        expect_literal(bytes, &mut pos, "}");

        let ours = cmp_int_float(i, f);
        if ours != expected {
            failures.push(format!(
                "linha {}: cmp_int_float({i}, {f:?}) esperado={expected} obtido={ours}",
                lineno + 1
            ));
        }
    }
    assert!(
        total > 0,
        "ficheiro de referencia sem casos de cmp_int_float"
    );
    assert_oracle_ok("cmp_int_float", total, &failures);
}

#[test]
fn float_to_int_matches_cpython_oracle() {
    let mut failures = Vec::new();
    let mut total = 0usize;
    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() || !line.contains("\"fn\": \"float_to_int\"") {
            continue;
        }
        total += 1;
        let bytes = line.as_bytes();
        let mut pos = 0usize;
        expect_fn(bytes, &mut pos, "float_to_int");
        expect_literal(bytes, &mut pos, ", \"f\": ");
        let f = f64::from_bits(parse_hex_bits_field(bytes, &mut pos));
        expect_literal(bytes, &mut pos, ", \"result\": ");
        let expected = parse_result_field(bytes, &mut pos);
        expect_literal(bytes, &mut pos, "}");

        let ours = float_to_int(f);
        let ok = match (&ours, &expected) {
            (Ok(v), ResultField::Int(want)) => v == want,
            (Err(code), ResultField::Error(msg)) => panic_message(*code) == msg,
            _ => false,
        };
        if !ok {
            let got_desc = match ours {
                Ok(v) => format!("Ok({v})"),
                Err(code) => format!("Err({})", panic_message(code)),
            };
            failures.push(format!(
                "linha {}: float_to_int({f:?}) -> obtido {got_desc}",
                lineno + 1
            ));
        }
    }
    assert!(
        total > 0,
        "ficheiro de referencia sem casos de float_to_int"
    );
    assert_oracle_ok("float_to_int", total, &failures);
}

fn assert_oracle_ok(name: &str, total: usize, failures: &[String]) {
    if !failures.is_empty() {
        let shown: Vec<&str> = failures.iter().take(20).map(String::as_str).collect();
        panic!(
            "{name}: {} diferenca(s) em {total} casos.\n{}",
            failures.len(),
            shown.join("\n"),
        );
    }
}
