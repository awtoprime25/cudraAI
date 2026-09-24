// Helpers numericos ao estilo CPython 3.12: divisao verdadeira de inteiros
// com arredondamento correto, divmod de float (float_divmod), potencia de
// float (float_pow), comparacao exata int/float e conversao float->int.
use crate::panics::{
    PANIC_FLOAT_FLOORDIV_ZERO, PANIC_FLOAT_MOD_ZERO, PANIC_FLOAT_TOO_LARGE, PANIC_INF_TO_INT,
    PANIC_NAN_TO_INT, PANIC_NEG_BASE_FRACTIONAL_POW, PANIC_POW_OVERFLOW, PANIC_ZERO_DIV,
    PANIC_ZERO_NEG_POWER,
};

const TWO_POW_53: u64 = 1u64 << 53;
const TWO_POW_63_F64: f64 = 9223372036854775808.0; // 2^63

/// a/b em base 10, arredondado corretamente (ties-to-even), como o
/// long_true_divide do CPython. Caminho rapido em f64 nativo quando ambas as
/// magnitudes cabem em 53 bits (exato por construcao do IEEE754); senao,
/// aritmetica exata em u128 sobre as magnitudes.
pub fn int_truediv(a: i64, b: i64) -> Result<f64, u32> {
    if b == 0 {
        return Err(PANIC_ZERO_DIV);
    }
    let neg = (a < 0) != (b < 0);
    let abs_a = a.unsigned_abs();
    let abs_b = b.unsigned_abs();

    let v = if abs_a <= TWO_POW_53 && abs_b <= TWO_POW_53 {
        abs_a as f64 / abs_b as f64
    } else {
        magnitude_truediv(abs_a as u128, abs_b as u128)
    };
    Ok(if neg { -v } else { v })
}

/// Divide (na<<shift)/nb ou na/(nb<<shift) consoante o sinal de `52 - e`,
/// devolvendo o quociente, o resto exato e o divisor usado — sempre sem
/// perda de bits, porque so se desloca para a esquerda.
fn shifted_divmod(na: u128, nb: u128, e: i32) -> (u128, u128, u128) {
    let shift = 52 - e;
    let (numer, denom) = if shift >= 0 {
        (na << shift, nb)
    } else {
        (na, nb << (-shift))
    };
    (numer / denom, numer % denom, denom)
}

/// Quociente exato de duas magnitudes (na pode ser 0; nb > 0), arredondado
/// a f64 com ties-to-even. O expoente e obtido por diferenca de bit-length,
/// corrigido em ate uma unidade quando a estimativa erra por um bit.
fn magnitude_truediv(na: u128, nb: u128) -> f64 {
    if na == 0 {
        return 0.0;
    }
    let la = 128 - na.leading_zeros() as i32;
    let lb = 128 - nb.leading_zeros() as i32;
    let mut e = la - lb;

    let (mut q, mut r, mut denom) = shifted_divmod(na, nb, e);
    if q < (1u128 << 52) {
        e -= 1;
        (q, r, denom) = shifted_divmod(na, nb, e);
    } else if q >= (1u128 << 53) {
        e += 1;
        (q, r, denom) = shifted_divmod(na, nb, e);
    }

    let two_r = r << 1;
    if two_r > denom || (two_r == denom && (q & 1) == 1) {
        q += 1;
        if q == (1u128 << 53) {
            q >>= 1;
            e += 1;
        }
    }

    (q as f64) * 2f64.powi(e - 52)
}

/// Nucleo do float_divmod do CPython: devolve (floordiv, mod), ambos ja
/// corretamente arredondados/sinalizados. `b` nunca e 0.0 (o chamador ja
/// validou).
fn float_divmod_core(a: f64, b: f64) -> (f64, f64) {
    let mut m = a % b; // Rust '%' em f64 e fmod, tal como o C.
    let mut div = (a - m) / b;
    if m != 0.0 {
        if (b < 0.0) != (m < 0.0) {
            m += b;
            div -= 1.0;
        }
    } else {
        m = f64::copysign(0.0, b);
    }

    let fd = if div != 0.0 {
        if div.is_nan() || div.is_infinite() {
            div
        } else {
            let d = div.floor();
            if div - d > 0.5 {
                d + 1.0
            } else {
                d
            }
        }
    } else {
        f64::copysign(0.0, a / b)
    };

    (fd, m)
}

pub fn float_floordiv(a: f64, b: f64) -> Result<f64, u32> {
    if b == 0.0 {
        return Err(PANIC_FLOAT_FLOORDIV_ZERO);
    }
    Ok(float_divmod_core(a, b).0)
}

pub fn float_mod(a: f64, b: f64) -> Result<f64, u32> {
    if b == 0.0 {
        return Err(PANIC_FLOAT_MOD_ZERO);
    }
    Ok(float_divmod_core(a, b).1)
}

fn is_odd_integer(y: f64) -> bool {
    y.fract() == 0.0 && (y.abs() % 2.0) == 1.0
}

/// x**y ao estilo do float_pow do CPython (Objects/floatobject.c), casos
/// especiais pela mesma ordem exata do original.
pub fn fpow(x: f64, y: f64) -> Result<f64, u32> {
    if y == 0.0 {
        return Ok(1.0);
    }
    if x.is_nan() {
        return Ok(f64::NAN);
    }
    if y.is_nan() {
        return Ok(if x == 1.0 { 1.0 } else { f64::NAN });
    }
    if y.is_infinite() {
        let ax = x.abs();
        if ax == 1.0 {
            return Ok(1.0);
        }
        let result_is_inf = (ax > 1.0) == (y > 0.0);
        return Ok(if result_is_inf { f64::INFINITY } else { 0.0 });
    }
    if x.is_infinite() {
        let y_odd = is_odd_integer(y);
        return Ok(if y > 0.0 {
            if y_odd {
                x
            } else {
                f64::INFINITY
            }
        } else if y_odd {
            f64::copysign(0.0, x)
        } else {
            0.0
        });
    }
    if x == 0.0 {
        if y < 0.0 {
            return Err(PANIC_ZERO_NEG_POWER);
        }
        let y_odd = is_odd_integer(y);
        return Ok(if x.is_sign_negative() && y_odd {
            -0.0
        } else {
            0.0
        });
    }
    if x < 0.0 && y.fract() != 0.0 {
        return Err(PANIC_NEG_BASE_FRACTIONAL_POW);
    }
    if x == 1.0 {
        return Ok(1.0);
    }

    let negate_result = x < 0.0 && is_odd_integer(y);
    let base = x.abs();
    let mut result = base.powf(y);
    if negate_result {
        result = -result;
    }
    if result.is_infinite() {
        return Err(PANIC_POW_OVERFLOW);
    }
    Ok(result)
}

/// Compara i (i64) com f (f64) exatamente: -1 (i<f), 0 (iguais), 1 (i>f),
/// 2 (f e nan). Ao estilo do float_richcompare do CPython.
pub fn cmp_int_float(i: i64, f: f64) -> i32 {
    if f.is_nan() {
        return 2;
    }
    if i.unsigned_abs() <= TWO_POW_53 {
        let fi = i as f64;
        return cmp_f64(fi, f);
    }
    if f.is_infinite() {
        return if f > 0.0 { -1 } else { 1 };
    }
    if f.abs() > TWO_POW_63_F64 {
        return if f > 0.0 { -1 } else { 1 };
    }

    let it = f.trunc();
    let i128_val = i as i128;
    let f128_val = it as i128;
    match i128_val.cmp(&f128_val) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => {
            let frac = f - it;
            if frac > 0.0 {
                -1
            } else if frac < 0.0 {
                1
            } else {
                0
            }
        }
    }
}

fn cmp_f64(a: f64, b: f64) -> i32 {
    if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    }
}

/// int(f): trunca para zero. nan/inf/fora do intervalo de i64 sao erro
/// (regra 9 — divergencia documentada para o caso "fora do intervalo",
/// onde o Python teria sucesso com bigint).
pub fn float_to_int(f: f64) -> Result<i64, u32> {
    if f.is_nan() {
        return Err(PANIC_NAN_TO_INT);
    }
    if f.is_infinite() {
        return Err(PANIC_INF_TO_INT);
    }
    let t = f.trunc();
    if t < -TWO_POW_63_F64 || t >= TWO_POW_63_F64 {
        return Err(PANIC_FLOAT_TOO_LARGE);
    }
    Ok(t as i64)
}

#[no_mangle]
pub extern "C" fn cudra_int_truediv(a: i64, b: i64) -> f64 {
    match int_truediv(a, b) {
        Ok(v) => v,
        Err(code) => crate::panics::cudra_panic(code),
    }
}

#[no_mangle]
pub extern "C" fn cudra_float_floordiv(a: f64, b: f64) -> f64 {
    match float_floordiv(a, b) {
        Ok(v) => v,
        Err(code) => crate::panics::cudra_panic(code),
    }
}

#[no_mangle]
pub extern "C" fn cudra_float_mod(a: f64, b: f64) -> f64 {
    match float_mod(a, b) {
        Ok(v) => v,
        Err(code) => crate::panics::cudra_panic(code),
    }
}

#[no_mangle]
pub extern "C" fn cudra_fpow(x: f64, y: f64) -> f64 {
    match fpow(x, y) {
        Ok(v) => v,
        Err(code) => crate::panics::cudra_panic(code),
    }
}

#[no_mangle]
pub extern "C" fn cudra_cmp_int_float(i: i64, f: f64) -> i32 {
    cmp_int_float(i, f)
}

#[no_mangle]
pub extern "C" fn cudra_float_to_int(f: f64) -> i64 {
    match float_to_int(f) {
        Ok(v) => v,
        Err(code) => crate::panics::cudra_panic(code),
    }
}
