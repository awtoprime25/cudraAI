// Exponenciacao inteira (int ** int) por exponenciacao binaria.
//
// Cuidado classico: nao quadrar a base depois do ultimo bit do expoente,
// senao valores como 2**62 dao overflow espurio no ultimo passo inutil.

use crate::panics::{PANIC_INT_OVERFLOW, PANIC_NEG_EXPONENT};

/// Devolve Err(codigo de panico) em caso de expoente negativo ou overflow
/// (quando `checked`). Sem `checked`, envolve em wrapping (mod 2^64).
pub fn ipow(base: i64, exp: i64, checked: bool) -> Result<i64, u32> {
    if exp < 0 {
        return Err(PANIC_NEG_EXPONENT);
    }
    if exp == 0 {
        return Ok(1);
    }

    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp as u64;

    loop {
        if e & 1 == 1 {
            result = if checked {
                match result.checked_mul(b) {
                    Some(v) => v,
                    None => return Err(PANIC_INT_OVERFLOW),
                }
            } else {
                result.wrapping_mul(b)
            };
        }
        e >>= 1;
        if e == 0 {
            break;
        }
        b = if checked {
            match b.checked_mul(b) {
                Some(v) => v,
                None => return Err(PANIC_INT_OVERFLOW),
            }
        } else {
            b.wrapping_mul(b)
        };
    }

    Ok(result)
}

#[no_mangle]
pub extern "C" fn cudra_ipow(base: i64, exp: i64, checked: bool) -> i64 {
    match ipow(base, exp, checked) {
        Ok(v) => v,
        Err(code) => crate::panics::cudra_panic(code),
    }
}
