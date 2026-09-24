// Estado global do argv, guardado em cudra_rt_init, e cudra_arg_int/float.
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::OnceLock;

use crate::panics::exit_with_error;
use crate::parse::{parse_py_float, parse_py_int, IntParseError};
use crate::pyrepr::python_repr;

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

fn arg_string(i: i64) -> String {
    let args = argv();
    if i < 0 || i as usize >= args.len() {
        exit_with_error("IndexError: argument index out of range");
    }
    // Bytes nao-UTF-8 tornam-se U+FFFD, o que garantidamente falha a
    // gramatica de int()/float() mais abaixo -> ValueError, como pedido.
    String::from_utf8_lossy(&args[i as usize]).into_owned()
}

#[no_mangle]
pub extern "C" fn cudra_rt_init(argc: i32, argv: *const *const c_char) {
    ARGV.get_or_init(|| build_args(argc, argv));
}

#[no_mangle]
pub extern "C" fn cudra_arg_int(i: i64) -> i64 {
    let s = arg_string(i);
    match parse_py_int(&s) {
        Ok(v) => v,
        Err(IntParseError::Invalid) => exit_with_error(&format!(
            "ValueError: invalid literal for int() with base 10: {}",
            python_repr(&s)
        )),
        Err(IntParseError::Overflow) => exit_with_error("OverflowError: int too large to convert"),
    }
}

#[no_mangle]
pub extern "C" fn cudra_arg_float(i: i64) -> f64 {
    let s = arg_string(i);
    match parse_py_float(&s) {
        Some(v) => v,
        None => exit_with_error(&format!(
            "ValueError: could not convert string to float: {}",
            python_repr(&s)
        )),
    }
}
