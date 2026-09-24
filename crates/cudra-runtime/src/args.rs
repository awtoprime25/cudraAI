// Estado global do argv, guardado em cudra_rt_init, e cudra_arg_int/float.
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::OnceLock;

use crate::panics::exit_with_error;
use crate::parse::{parse_py_float, parse_py_int, IntParseError};
use crate::pyrepr::{python_repr, python_repr_truncated};
use crate::unicode::utf8_surrogateescape_decode;

/// Truncagem das mensagens de erro do int() do CPython (formato interno %.200R).
const REPR_TRUNCATE_CHARS: usize = 200;

static ARGV: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

fn build_args(argc: i32, argv: *const *const c_char) -> Vec<Vec<u8>> {
    let mut args = Vec::new();
    if argv.is_null() || argc <= 0 {
        return args;
    }
    for i in 0..argc as isize {
        // SAFETY: o chamador (o main gerado) garante que argv aponta para
        // argc ponteiros C validos, cada um ou nulo ou uma C string valida —
        // e o mesmo contrato do argv de um main() em C.
        unsafe {
            let ptr = *argv.offset(i);
            if ptr.is_null() {
                args.push(Vec::new());
            } else {
                args.push(CStr::from_ptr(ptr).to_bytes().to_vec());
            }
        }
    }
    args
}

fn argv() -> &'static [Vec<u8>] {
    match ARGV.get() {
        Some(v) => v.as_slice(),
        None => &[],
    }
}

fn arg_codepoints(i: i64) -> Vec<u32> {
    let args = argv();
    let len = args.len() as i64;
    // Indice negativo conta a partir do fim, tal como sys.argv[i] (lista
    // normal do Python): i < 0 -> i += len.
    let idx = if i < 0 {
        i.checked_add(len).unwrap_or(-1)
    } else {
        i
    };
    if idx < 0 || idx >= len {
        exit_with_error("IndexError: list index out of range");
    }
    // Bytes nao-UTF-8 tornam-se surrogates altos (0xDC00 + byte), tal como o
    // codec surrogateescape do Python usa para descodificar argv.
    utf8_surrogateescape_decode(&args[idx as usize])
}

#[no_mangle]
pub extern "C" fn cudra_rt_init(argc: i32, argv: *const *const c_char) {
    ARGV.get_or_init(|| build_args(argc, argv));
}

#[no_mangle]
pub extern "C" fn cudra_arg_int(i: i64) -> i64 {
    let cps = arg_codepoints(i);
    match parse_py_int(&cps) {
        Ok(v) => v,
        Err(IntParseError::Invalid) => exit_with_error(&format!(
            "ValueError: invalid literal for int() with base 10: {}",
            python_repr_truncated(&cps, REPR_TRUNCATE_CHARS)
        )),
        Err(IntParseError::TooManyDigits(n)) => exit_with_error(&format!(
            "ValueError: Exceeds the limit (4300 digits) for integer string \
             conversion: value has {n} digits; use sys.set_int_max_str_digits() \
             to increase the limit"
        )),
        Err(IntParseError::Overflow) => exit_with_error("OverflowError: int too large to convert"),
    }
}

#[no_mangle]
pub extern "C" fn cudra_arg_float(i: i64) -> f64 {
    let cps = arg_codepoints(i);
    match parse_py_float(&cps) {
        Some(v) => v,
        None => exit_with_error(&format!(
            "ValueError: could not convert string to float: {}",
            python_repr(&cps)
        )),
    }
}
