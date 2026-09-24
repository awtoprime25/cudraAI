// Parsing de strings para int/float ao estilo do int(s)/float(s) do Python
// (base 10). Nunca confia no parser do Rust para validar a gramatica — so o
// usa (via str::parse::<f64>) depois de a propria gramatica ja ter sido
// validada e normalizada a mao. Trabalha com codepoints (u32) para aceitar
// digitos Unicode (categoria Nd) e argv com surrogates soltos.
use crate::unicode::digit_value;

pub enum IntParseError {
    Invalid,
    TooManyDigits(usize),
    Overflow,
}

/// Limite de digitos do int(str) do CPython 3.12 (sys.int_info.default_max_str_digits).
const MAX_INT_DIGITS: usize = 4300;

fn is_py_whitespace(cp: u32) -> bool {
    match char::from_u32(cp) {
        Some(c) => c.is_whitespace(),
        None => false,
    }
}

fn trim_ws(cps: &[u32]) -> &[u32] {
    let start = cps.iter().position(|&cp| !is_py_whitespace(cp));
    let Some(start) = start else {
        return &[];
    };
    let end = cps.iter().rposition(|&cp| !is_py_whitespace(cp)).unwrap();
    &cps[start..=end]
}

/// int(s) em base 10: strip de espacos, sinal opcional, digitos Nd com '_'
/// so entre digitos, zeros a esquerda permitidos. Mais de MAX_INT_DIGITS
/// digitos (depois de gramatica valida) -> TooManyDigits. Fora do intervalo
/// de i64 -> Overflow (o Python teria sucesso com bigint: divergencia
/// documentada).
pub fn parse_py_int(cps: &[u32]) -> Result<i64, IntParseError> {
    let trimmed = trim_ws(cps);

    let mut negative = false;
    let mut rest = trimmed;
    if let Some(&cp) = trimmed.first() {
        if cp == '+' as u32 || cp == '-' as u32 {
            negative = cp == '-' as u32;
            rest = &trimmed[1..];
        }
    }

    let (digits, consumed, valid) = read_digit_run(rest, 0);
    if !valid || digits.is_empty() || consumed != rest.len() {
        return Err(IntParseError::Invalid);
    }

    if digits.len() > MAX_INT_DIGITS {
        return Err(IntParseError::TooManyDigits(digits.len()));
    }

    let mut magnitude: u64 = 0;
    for d in &digits {
        magnitude = magnitude
            .checked_mul(10)
            .and_then(|v| v.checked_add(*d as u64))
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

/// Le um digitpart: digit ('_'? digit)* a partir de `cps[start..]`, aceitando
/// qualquer digito Nd (nao so ASCII). Devolve (valores 0-9 sem '_', numero de
/// codepoints consumidos a partir de start, gramatica valida). Uma corrida
/// vazia (0 digitos) e valida — cabe ao chamador decidir se isso chega para o
/// contexto (ex.: fracao vazia em "5." e ok; nenhum digito em int() nao e).
fn read_digit_run(cps: &[u32], start: usize) -> (Vec<u8>, usize, bool) {
    let mut out = Vec::new();
    let mut idx = start;
    let mut prev_was_digit = false;

    while idx < cps.len() {
        if let Some(d) = digit_value(cps[idx]) {
            out.push(d);
            prev_was_digit = true;
            idx += 1;
        } else if cps[idx] == '_' as u32 && prev_was_digit {
            if idx + 1 < cps.len() && digit_value(cps[idx + 1]).is_some() {
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
/// distinguir maiusculas (so ASCII), ou um numero decimal com '_' so entre
/// digitos. Sem limite de digitos (o Python tambem nao tem para float).
/// Overflow vira +-inf e underflow vira 0.0 (comportamento normal do parse
/// de f64 do Rust, que e correto a arredondar).
pub fn parse_py_float(cps: &[u32]) -> Option<f64> {
    let trimmed = trim_ws(cps);

    let mut negative = false;
    let mut rest = trimmed;
    if let Some(&cp) = trimmed.first() {
        if cp == '+' as u32 || cp == '-' as u32 {
            negative = cp == '-' as u32;
            rest = &trimmed[1..];
        }
    }
    if rest.is_empty() {
        return None;
    }

    if let Some(word) = ascii_lower_word(rest) {
        if word == "inf" || word == "infinity" {
            return Some(if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            });
        }
        if word == "nan" {
            return Some(f64::NAN);
        }
    }

    let clean = parse_float_numeric(rest)?;
    let full = if negative { format!("-{clean}") } else { clean };
    full.parse::<f64>().ok()
}

/// Devolve `rest` em minusculas ASCII sse todos os codepoints forem ASCII;
/// caso contrario None (nunca e "inf"/"infinity"/"nan").
fn ascii_lower_word(cps: &[u32]) -> Option<String> {
    let mut s = String::with_capacity(cps.len());
    for &cp in cps {
        if cp > 0x7f {
            return None;
        }
        s.push((cp as u8 as char).to_ascii_lowercase());
    }
    Some(s)
}

/// Valida e normaliza a mantissa+expoente de um floatnumber (sem sinal e sem
/// inf/nan, ja tratados antes). Devolve uma string ASCII sempre aceitavel
/// por str::parse::<f64> (nunca "5." nem ".5" cruas — normalizadas para "5"
/// e "0.5").
fn parse_float_numeric(cps: &[u32]) -> Option<String> {
    let n = cps.len();
    let mut i = 0usize;

    let (int_digits, int_consumed, int_valid) = read_digit_run(cps, i);
    if !int_valid {
        return None;
    }
    i += int_consumed;

    let mut frac_digits: Vec<u8> = Vec::new();
    if i < n && cps[i] == '.' as u32 {
        i += 1;
        let (fd, fc, frac_valid) = read_digit_run(cps, i);
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
    let mut exp_digits: Vec<u8> = Vec::new();
    if i < n && (cps[i] == 'e' as u32 || cps[i] == 'E' as u32) {
        let mut j = i + 1;
        let mut sign = '+';
        if j < n && (cps[j] == '+' as u32 || cps[j] == '-' as u32) {
            sign = if cps[j] == '+' as u32 { '+' } else { '-' };
            j += 1;
        }
        let (ed, ec, exp_valid) = read_digit_run(cps, j);
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

    let int_str: String = int_digits.iter().map(|d| (b'0' + d) as char).collect();
    let frac_str: String = frac_digits.iter().map(|d| (b'0' + d) as char).collect();
    let exp_str: String = exp_digits.iter().map(|d| (b'0' + d) as char).collect();

    let mantissa = match (int_str.is_empty(), frac_str.is_empty()) {
        (false, false) => format!("{int_str}.{frac_str}"),
        (false, true) => int_str,
        (true, false) => format!("0.{frac_str}"),
        (true, true) => std::process::abort(), // ja excluido acima
    };

    if has_exp {
        Some(format!("{mantissa}e{exp_sign}{exp_str}"))
    } else {
        Some(mantissa)
    }
}
