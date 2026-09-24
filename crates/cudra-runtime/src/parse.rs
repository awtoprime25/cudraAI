// Parsing de strings para int/float ao estilo do int(s)/float(s) do Python
// (base 10). Nunca confia no parser do Rust para validar a gramatica — so o
// usa (via str::parse::<f64>) depois de a propria gramatica ja ter sido
// validada e normalizada a mao.

pub enum IntParseError {
    Invalid,
    Overflow,
}

fn is_py_whitespace(c: char) -> bool {
    c.is_whitespace()
}

/// int(s) em base 10: strip de espacos, sinal opcional, digitos ASCII com
/// '_' so entre digitos, zeros a esquerda permitidos. Fora do intervalo de
/// i64 -> Overflow (o Python teria sucesso com bigint: divergencia
/// documentada).
pub fn parse_py_int(s: &str) -> Result<i64, IntParseError> {
    let trimmed = s.trim_matches(is_py_whitespace);

    let mut negative = false;
    let mut rest = trimmed;
    if let Some(c) = trimmed.chars().next() {
        if c == '+' || c == '-' {
            negative = c == '-';
            rest = &trimmed[c.len_utf8()..];
        }
    }

    let chars: Vec<char> = rest.chars().collect();
    let (digits, consumed, valid) = read_digit_run(&chars, 0);
    if !valid || digits.is_empty() || consumed != chars.len() {
        return Err(IntParseError::Invalid);
    }

    let mut magnitude: u64 = 0;
    for b in digits.bytes() {
        let d = (b - b'0') as u64;
        magnitude = magnitude
            .checked_mul(10)
            .and_then(|v| v.checked_add(d))
            .ok_or(IntParseError::Overflow)?;
    }

    if negative {
        let min_magnitude = (i64::MAX as u64) + 1;
        if magnitude > min_magnitude {
            Err(IntParseError::Overflow)
        } else if magnitude == min_magnitude {
            Ok(i64::MIN)
        } else {
            Ok(-(magnitude as i64))
        }
    } else if magnitude > i64::MAX as u64 {
        Err(IntParseError::Overflow)
    } else {
        Ok(magnitude as i64)
    }
}

/// Le um digitpart: digit ('_'? digit)* a partir de `chars[start..]`.
/// Devolve (digitos sem '_', numero de chars consumidos a partir de start,
/// gramatica valida). Uma corrida vazia (0 digitos) e valida — cabe ao
/// chamador decidir se isso chega para o contexto (ex.: fracao vazia em "5."
/// e ok; nenhum digito em int() nao e).
fn read_digit_run(chars: &[char], start: usize) -> (String, usize, bool) {
    let mut out = String::new();
    let mut idx = start;
    let mut prev_was_digit = false;

    while idx < chars.len() {
        let c = chars[idx];
        if c.is_ascii_digit() {
            out.push(c);
            prev_was_digit = true;
            idx += 1;
        } else if c == '_' && prev_was_digit {
            if idx + 1 < chars.len() && chars[idx + 1].is_ascii_digit() {
                idx += 1;
                prev_was_digit = false;
            } else {
                return (out, idx - start, false);
            }
        } else {
            break;
        }
    }

    (out, idx - start, true)
}

/// float(s): strip de espacos, sinal opcional, "inf"/"infinity"/"nan" sem
/// distinguir maiusculas, ou um numero decimal com '_' so entre digitos.
/// Overflow vira +-inf e underflow vira 0.0 (comportamento normal do parse
/// de f64 do Rust, que e correto a arredondar).
pub fn parse_py_float(s: &str) -> Option<f64> {
    let trimmed = s.trim_matches(is_py_whitespace);

    let mut negative = false;
    let mut rest = trimmed;
    if let Some(c) = trimmed.chars().next() {
        if c == '+' || c == '-' {
            negative = c == '-';
            rest = &trimmed[c.len_utf8()..];
        }
    }
    if rest.is_empty() {
        return None;
    }

    let lower = rest.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" {
        return Some(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if lower == "nan" {
        return Some(f64::NAN);
    }

    let clean = parse_float_numeric(rest)?;
    let full = if negative { format!("-{clean}") } else { clean };
    full.parse::<f64>().ok()
}

/// Valida e normaliza a mantissa+expoente de um floatnumber (sem sinal e sem
/// inf/nan, ja tratados antes). Devolve uma string sempre aceitavel por
/// str::parse::<f64> (nunca "5." nem ".5" cruas — normalizadas para "5" e
/// "0.5").
fn parse_float_numeric(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0usize;

    let (int_digits, int_consumed, int_valid) = read_digit_run(&chars, i);
    if !int_valid {
        return None;
    }
    i += int_consumed;

    let mut frac_digits = String::new();
    if i < n && chars[i] == '.' {
        i += 1;
        let (fd, fc, frac_valid) = read_digit_run(&chars, i);
        if !frac_valid {
            return None;
        }
        frac_digits = fd;
        i += fc;
    }

    if int_digits.is_empty() && frac_digits.is_empty() {
        return None;
    }

    let mut has_exp = false;
    let mut exp_sign = '+';
    let mut exp_digits = String::new();
    if i < n && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        let mut sign = '+';
        if j < n && (chars[j] == '+' || chars[j] == '-') {
            sign = chars[j];
            j += 1;
        }
        let (ed, ec, exp_valid) = read_digit_run(&chars, j);
        if !exp_valid || ed.is_empty() {
            return None;
        }
        has_exp = true;
        exp_sign = sign;
        exp_digits = ed;
        i = j + ec;
    }

    if i != n {
        return None;
    }

    let mantissa = match (int_digits.is_empty(), frac_digits.is_empty()) {
        (false, false) => format!("{int_digits}.{frac_digits}"),
        (false, true) => int_digits,
        (true, false) => format!("0.{frac_digits}"),
        (true, true) => std::process::abort(), // ja excluido acima
    };

    if has_exp {
        Some(format!("{mantissa}e{exp_sign}{exp_digits}"))
    } else {
        Some(mantissa)
    }
}
