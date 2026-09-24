// Teste sem referencia: 1_000_000 de padroes de bits pseudoaleatorios
// (xorshift64 com seed fixa, sem dependencias) — float_repr(x) tem de fazer
// round-trip e cumprir a gramatica do repr do Python.
//
// Lento em debug: marcado #[ignore]. Corre com
// `cargo test -p cudra-runtime --release -- --ignored`.
use cudra_runtime::float_repr;

fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Gramatica a mao do repr do Python: "nan"/"inf"/"-inf", ou notacao fixa
/// "-?\d+\.\d+", ou cientifica "-?\d(\.\d+)?e[+-]\d{2,}" sem zeros a mais no
/// expoente.
fn validate_repr_grammar(s: &str) -> bool {
    if s == "nan" || s == "inf" || s == "-inf" {
        return true;
    }
    let rest = s.strip_prefix('-').unwrap_or(s);
    if rest.is_empty() {
        return false;
    }

    if let Some(e_pos) = rest.find('e') {
        let mantissa = &rest[..e_pos];
        let exp = &rest[e_pos + 1..];

        let mantissa_ok = if let Some(dot) = mantissa.find('.') {
            let int_part = &mantissa[..dot];
            let frac_part = &mantissa[dot + 1..];
            int_part.len() == 1
                && all_ascii_digits(int_part)
                && !frac_part.is_empty()
                && all_ascii_digits(frac_part)
        } else {
            mantissa.len() == 1 && all_ascii_digits(mantissa)
        };
        if !mantissa_ok {
            return false;
        }

        let exp_digits = match exp.strip_prefix('+').or_else(|| exp.strip_prefix('-')) {
            Some(d) => d,
            None => return false, // sinal do expoente e obrigatorio
        };
        exp_digits.len() >= 2
            && all_ascii_digits(exp_digits)
            && !(exp_digits.len() > 2 && exp_digits.starts_with('0'))
    } else if let Some(dot) = rest.find('.') {
        let int_part = &rest[..dot];
        let frac_part = &rest[dot + 1..];
        !int_part.is_empty()
            && all_ascii_digits(int_part)
            && !frac_part.is_empty()
            && all_ascii_digits(frac_part)
    } else {
        false // notacao fixa tem sempre pelo menos ".0"
    }
}

fn all_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

#[test]
#[ignore]
fn float_repr_roundtrip_and_grammar() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut failures: Vec<String> = Vec::new();

    for i in 0..1_000_000u64 {
        let bits = xorshift64(&mut state);
        let x = f64::from_bits(bits);
        let s = float_repr(x);

        if !validate_repr_grammar(&s) {
            failures.push(format!(
                "#{i} bits={bits:016x} repr={s:?} gramatica invalida"
            ));
        } else if x.is_nan() {
            let parsed: f64 = s.parse().unwrap_or(f64::NAN);
            if !parsed.is_nan() {
                failures.push(format!(
                    "#{i} bits={bits:016x} repr={s:?} nao deu parse como nan"
                ));
            }
        } else {
            match s.parse::<f64>() {
                Ok(parsed) if parsed.to_bits() == x.to_bits() => {}
                Ok(parsed) => failures.push(format!(
                    "#{i} bits={bits:016x} repr={s:?} round-trip falhou: obtido_bits={:016x}",
                    parsed.to_bits()
                )),
                Err(_) => failures.push(format!("#{i} bits={bits:016x} repr={s:?} nao deu parse")),
            }
        }

        if failures.len() >= 20 {
            break;
        }
    }

    assert!(
        failures.is_empty(),
        "falhas de round-trip/gramatica:\n{}",
        failures.join("\n")
    );
}
