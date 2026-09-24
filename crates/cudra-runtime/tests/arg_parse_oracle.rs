// Compara parse_py_int/parse_py_float/python_repr contra int()/float()/repr()
// do CPython 3.12, lendo arg_parse_ref.txt (JSON por linha, gerado por
// gen_refs.py). Sem dependencias externas: mini-parser JSON escrito a mao,
// suficiente para o formato fixo que o gen_refs.py produz. Os campos ficam
// em Vec<u32> (codepoints), nao String, para aceitar surrogates soltos sem
// entrar em panico em char::from_u32.
use cudra_runtime::{float_repr, parse_py_float, parse_py_int, python_repr, IntParseError};

const REF: &str = include_str!("arg_parse_ref.txt");

enum IntField {
    Number(i64),
    TooManyDigits(usize),
    Sentinel(String),
}

struct RefCase {
    s: Vec<u32>,
    int_field: IntField,
    float_field: String,
    repr_field: String,
}

/// Le uma string JSON como sequencia de codepoints. Um par de surrogates
/// \uD8xx\uDCxx e combinado num so codepoint astral (como o json.dumps
/// produz); um surrogate solto (sem par) fica tal e qual, sem exigir
/// validade como char do Rust.
fn parse_json_string(bytes: &[u8], pos: &mut usize) -> Vec<u32> {
    assert_eq!(bytes[*pos], b'"', "esperava '\"' no oraculo");
    *pos += 1;
    let mut out = Vec::new();
    loop {
        let b = bytes[*pos];
        if b == b'"' {
            *pos += 1;
            break;
        } else if b == b'\\' {
            *pos += 1;
            let esc = bytes[*pos];
            match esc {
                b'"' => {
                    out.push('"' as u32);
                    *pos += 1;
                }
                b'\\' => {
                    out.push('\\' as u32);
                    *pos += 1;
                }
                b'/' => {
                    out.push('/' as u32);
                    *pos += 1;
                }
                b'n' => {
                    out.push('\n' as u32);
                    *pos += 1;
                }
                b't' => {
                    out.push('\t' as u32);
                    *pos += 1;
                }
                b'r' => {
                    out.push('\r' as u32);
                    *pos += 1;
                }
                b'b' => {
                    out.push(0x0008);
                    *pos += 1;
                }
                b'f' => {
                    out.push(0x000c);
                    *pos += 1;
                }
                b'u' => {
                    *pos += 1;
                    let cu = read_hex4(bytes, pos);
                    if (0xD800..0xDC00).contains(&cu) && bytes[*pos..].starts_with(b"\\u") {
                        let save = *pos;
                        *pos += 2;
                        let cu2 = read_hex4(bytes, pos);
                        if (0xDC00..0xE000).contains(&cu2) {
                            let cp = 0x10000 + ((cu - 0xD800) << 10) + (cu2 - 0xDC00);
                            out.push(cp);
                        } else {
                            // nao era par valido: repor e tratar como soltos.
                            *pos = save;
                            out.push(cu);
                        }
                    } else {
                        out.push(cu);
                    }
                }
                other => panic!("escape json desconhecido: {}", other as char),
            }
        } else {
            out.push(b as u32);
            *pos += 1;
        }
    }
    out
}

fn read_hex4(bytes: &[u8], pos: &mut usize) -> u32 {
    let hex = std::str::from_utf8(&bytes[*pos..*pos + 4]).expect("hex \\u invalido");
    let v = u32::from_str_radix(hex, 16).expect("hex \\u invalido");
    *pos += 4;
    v
}

/// Converte codepoints garantidamente sem surrogates soltos (texto produzido
/// pelo proprio Python, nunca argv cru) para String.
fn cps_to_string(cps: &[u32]) -> String {
    cps.iter()
        .map(|&cp| char::from_u32(cp).expect("campo do oraculo nao deveria conter surrogates"))
        .collect()
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

fn parse_int_field(bytes: &[u8], pos: &mut usize) -> IntField {
    if bytes[*pos] == b'"' {
        IntField::Sentinel(cps_to_string(&parse_json_string(bytes, pos)))
    } else if bytes[*pos] == b'{' {
        expect_literal(bytes, pos, "{\"too_many_digits\": ");
        let start = *pos;
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        let text = std::str::from_utf8(&bytes[start..*pos]).expect("numero invalido no oraculo");
        let n: usize = text.parse().expect("too_many_digits invalido no oraculo");
        expect_literal(bytes, pos, "}");
        IntField::TooManyDigits(n)
    } else {
        let start = *pos;
        if bytes[*pos] == b'-' {
            *pos += 1;
        }
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        let text = std::str::from_utf8(&bytes[start..*pos]).expect("numero invalido no oraculo");
        IntField::Number(text.parse::<i64>().expect("numero fora de i64 no oraculo"))
    }
}

fn parse_ref_line(line: &str) -> RefCase {
    let bytes = line.as_bytes();
    let mut pos = 0usize;
    expect_literal(bytes, &mut pos, "{\"s\": ");
    let s = parse_json_string(bytes, &mut pos);
    expect_literal(bytes, &mut pos, ", \"int\": ");
    let int_field = parse_int_field(bytes, &mut pos);
    expect_literal(bytes, &mut pos, ", \"float\": ");
    let float_field = cps_to_string(&parse_json_string(bytes, &mut pos));
    expect_literal(bytes, &mut pos, ", \"repr\": ");
    let repr_field = cps_to_string(&parse_json_string(bytes, &mut pos));
    expect_literal(bytes, &mut pos, "}");
    RefCase {
        s,
        int_field,
        float_field,
        repr_field,
    }
}

#[test]
fn arg_parse_matches_cpython_oracle() {
    let mut int_failures = Vec::new();
    let mut float_failures = Vec::new();
    let mut repr_failures = Vec::new();
    let mut total = 0usize;

    for (lineno, line) in REF.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        total += 1;
        let case = parse_ref_line(line);

        let our_int = parse_py_int(&case.s);
        let int_ok = match &case.int_field {
            IntField::Number(v) => matches!(our_int, Ok(got) if got == *v),
            IntField::TooManyDigits(n) => {
                matches!(our_int, Err(IntParseError::TooManyDigits(got)) if got == *n)
            }
            IntField::Sentinel(s) if s == "ValueError" => {
                matches!(our_int, Err(IntParseError::Invalid))
            }
            IntField::Sentinel(s) if s == "OverflowError" => {
                matches!(our_int, Err(IntParseError::Overflow))
            }
            // DIVERGE: o Python aceita (caracteres nao-ASCII fora de Nd), nos
            // rejeitamos sempre com ValueError — essa e a divergencia
            // documentada.
            IntField::Sentinel(s) if s == "DIVERGE" => {
                matches!(our_int, Err(IntParseError::Invalid))
            }
            IntField::Sentinel(other) => panic!("sentinela int desconhecida no oraculo: {other}"),
        };
        if !int_ok {
            let got_desc = match our_int {
                Ok(v) => format!("Ok({v})"),
                Err(IntParseError::Invalid) => "ValueError".to_string(),
                Err(IntParseError::TooManyDigits(n)) => format!("TooManyDigits({n})"),
                Err(IntParseError::Overflow) => "OverflowError".to_string(),
            };
            int_failures.push(format!(
                "linha {}: s={:?} esperado={} obtido={got_desc}",
                lineno + 1,
                case.s,
                describe_int_field(&case.int_field),
            ));
        }

        let our_float = parse_py_float(&case.s);
        let float_ok = if case.float_field == "ValueError" || case.float_field == "DIVERGE" {
            our_float.is_none()
        } else {
            match our_float {
                Some(v) => float_repr(v) == case.float_field,
                None => false,
            }
        };
        if !float_ok {
            let got_desc = match our_float {
                Some(v) => float_repr(v),
                None => "ValueError".to_string(),
            };
            float_failures.push(format!(
                "linha {}: s={:?} esperado={:?} obtido={got_desc:?}",
                lineno + 1,
                case.s,
                case.float_field,
            ));
        }

        let our_repr = python_repr(&case.s);
        if our_repr != case.repr_field {
            repr_failures.push(format!(
                "linha {}: s={:?} esperado={:?} obtido={:?}",
                lineno + 1,
                case.s,
                case.repr_field,
                our_repr,
            ));
        }
    }

    assert!(total > 0, "ficheiro de referencia vazio");

    if !int_failures.is_empty() || !float_failures.is_empty() || !repr_failures.is_empty() {
        let shown_int: Vec<&str> = int_failures.iter().take(20).map(String::as_str).collect();
        let shown_float: Vec<&str> = float_failures.iter().take(20).map(String::as_str).collect();
        let shown_repr: Vec<&str> = repr_failures.iter().take(20).map(String::as_str).collect();
        panic!(
            "int: {} diferenca(s), float: {} diferenca(s), repr: {} diferenca(s), em {total} casos.\n\
             -- int --\n{}\n-- float --\n{}\n-- repr --\n{}",
            int_failures.len(),
            float_failures.len(),
            repr_failures.len(),
            shown_int.join("\n"),
            shown_float.join("\n"),
            shown_repr.join("\n"),
        );
    }
}

fn describe_int_field(f: &IntField) -> String {
    match f {
        IntField::Number(v) => format!("Ok({v})"),
        IntField::TooManyDigits(n) => format!("TooManyDigits({n})"),
        IntField::Sentinel(s) => s.clone(),
    }
}
