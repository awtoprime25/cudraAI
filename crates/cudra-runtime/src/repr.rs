// repr() de float igual ao do CPython 3.12.
//
// Estrategia: format!("{:e}", x) do Rust ja da os digitos mais curtos com
// round-trip (mantissa "d" ou "d.ddd" + "e" + expoente decimal sem zeros a
// mais, sem sinal '+'). Extraimos os digitos e o expoente decimal E (posicao
// do primeiro digito) e reformatamos seguindo as regras do repr() do Python.

pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }

    let formatted = format!("{x:e}");
    let (sign, rest) = match formatted.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", formatted.as_str()),
    };
    let Some(e_idx) = rest.find('e') else {
        // format!("{:e}", ..) produz sempre um 'e'; isto seria um bug interno.
        std::process::abort();
    };
    let mantissa = &rest[..e_idx];
    let exp_str = &rest[e_idx + 1..];
    let Ok(exp) = exp_str.parse::<i32>() else {
        std::process::abort();
    };
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    if digits.is_empty() {
        std::process::abort();
    }

    if (-4..16).contains(&exp) {
        fixed_notation(sign, &digits, exp)
    } else {
        scientific_notation(sign, &digits, exp)
    }
}

fn fixed_notation(sign: &str, digits: &str, exp: i32) -> String {
    let mut s = String::with_capacity(digits.len() + 8);
    s.push_str(sign);
    if exp >= 0 {
        let point_pos = exp as usize + 1;
        if digits.len() <= point_pos {
            s.push_str(digits);
            for _ in 0..(point_pos - digits.len()) {
                s.push('0');
            }
            s.push_str(".0");
        } else {
            s.push_str(&digits[..point_pos]);
            s.push('.');
            s.push_str(&digits[point_pos..]);
        }
    } else {
        s.push_str("0.");
        let zeros = (-exp - 1) as usize;
        for _ in 0..zeros {
            s.push('0');
        }
        s.push_str(digits);
    }
    s
}

fn scientific_notation(sign: &str, digits: &str, exp: i32) -> String {
    let mut s = String::with_capacity(digits.len() + 10);
    s.push_str(sign);
    let mut chars = digits.chars();
    let Some(first) = chars.next() else {
        std::process::abort();
    };
    s.push(first);
    let remainder = chars.as_str();
    if !remainder.is_empty() {
        s.push('.');
        s.push_str(remainder);
    }
    s.push('e');
    s.push(if exp >= 0 { '+' } else { '-' });
    let abs_exp = exp.unsigned_abs();
    if abs_exp < 10 {
        s.push('0');
    }
    s.push_str(&abs_exp.to_string());
    s
}

/// Escreve o repr de `v` em `buf` (ate `cap` bytes) e devolve o comprimento
/// total, como o snprintf. Com `buf` nulo so devolve o comprimento.
///
/// # Safety
/// `buf` tem de ser nulo ou apontar para pelo menos `cap` bytes escreviveis.
#[no_mangle]
pub unsafe extern "C" fn cudra_float_repr(v: f64, buf: *mut u8, cap: usize) -> usize {
    let s = float_repr(v);
    let bytes = s.as_bytes();
    if !buf.is_null() && cap > 0 {
        let n = bytes.len().min(cap);
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::float_repr;

    macro_rules! case {
        ($name:ident, $x:expr, $expected:expr) => {
            #[test]
            fn $name() {
                assert_eq!(float_repr($x), $expected);
            }
        };
    }

    case!(t_0_1, 0.1, "0.1");
    case!(t_1_0, 1.0, "1.0");
    case!(t_neg_0_0, -0.0, "-0.0");
    case!(t_1e16, 1e16, "1e+16");
    case!(t_1e15, 1e15, "1000000000000000.0");
    case!(
        t_123456789012345678,
        123456789012345678.0,
        "1.2345678901234568e+17"
    );
    case!(t_0_0001, 0.0001, "0.0001");
    case!(t_0_00001, 0.00001, "1e-05");
    case!(t_one_third, 1.0 / 3.0, "0.3333333333333333");
    case!(t_2_5e_5, 2.5e-5, "2.5e-05");
    case!(t_5e_324, 5e-324, "5e-324");
    case!(t_f64_max, 1.7976931348623157e308, "1.7976931348623157e+308");
    case!(t_9999999999999998, 9999999999999998.0, "9999999999999998.0");
    case!(t_1e22, 1e22, "1e+22");
    case!(t_1e_7, 1e-7, "1e-07");
    case!(t_123_456, 123.456, "123.456");
    case!(t_2_pow_53, 9007199254740992.0_f64, "9007199254740992.0");
    case!(
        t_2_pow_63,
        9223372036854775808.0_f64,
        "9.223372036854776e+18"
    );
    case!(t_0_5, 0.5, "0.5");
    case!(t_100_0, 100.0, "100.0");
    case!(t_1e100, 1e100, "1e+100");

    #[test]
    fn t_inf() {
        assert_eq!(float_repr(f64::INFINITY), "inf");
    }

    #[test]
    fn t_neg_inf() {
        assert_eq!(float_repr(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn t_nan() {
        assert_eq!(float_repr(f64::NAN), "nan");
    }

    #[test]
    fn t_inf_minus_inf_is_nan() {
        assert_eq!(float_repr(f64::INFINITY - f64::INFINITY), "nan");
    }

    #[test]
    fn t_neg_nan() {
        assert_eq!(float_repr(-f64::NAN), "nan");
    }

    #[test]
    fn t_max_repr_len_is_at_most_32_bytes() {
        let values: Vec<f64> = vec![
            0.1,
            1.0,
            -0.0,
            1e16,
            1e15,
            123456789012345678.0,
            0.0001,
            0.00001,
            1.0 / 3.0,
            2.5e-5,
            5e-324,
            -5e-324,
            1.7976931348623157e308,
            -1.7976931348623157e308,
            9999999999999998.0,
            1e22,
            1e-7,
            123.456,
            9007199254740992.0,
            9223372036854775808.0,
            0.5,
            100.0,
            1e100,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::MIN,
            f64::EPSILON,
        ];
        let max_len = values.iter().map(|&x| float_repr(x).len()).max().unwrap();
        assert!(max_len <= 32, "repr mais longo tem {max_len} bytes");
    }
}
