//! Runtime do cudraAI: ABI C ligada aos binarios gerados pelo compilador.
//! Sem dependencias externas (so std). O objetivo e que stdout e exit code
//! sejam iguais aos do CPython 3.12, salvo as divergencias documentadas nos
//! modulos (overflow de i64 em argv, etc.).

mod args;
mod ipow;
mod out;
mod panics;
mod parse;
mod pyrepr;
mod repr;
mod unicode;
mod unicode_tables;

pub use args::{cudra_arg_float, cudra_arg_int, cudra_rt_init};
pub use ipow::{cudra_ipow, ipow};
pub use out::{
    cudra_print_bool, cudra_print_end, cudra_print_float, cudra_print_int, cudra_print_none,
    cudra_print_sep, cudra_rt_exit,
};
pub use panics::{
    cudra_panic, panic_message, PANIC_FLOAT_TOO_LARGE, PANIC_INF_TO_INT, PANIC_INT_OVERFLOW,
    PANIC_NAN_TO_INT, PANIC_NEG_EXPONENT, PANIC_NEG_SHIFT, PANIC_RANGE_STEP_ZERO, PANIC_ZERO_DIV,
};
pub use parse::{parse_py_float, parse_py_int, IntParseError};
pub use pyrepr::{python_repr, python_repr_truncated};
pub use repr::{cudra_float_repr, float_repr};
pub use unicode::{digit_value, is_printable, str_to_codepoints, utf8_surrogateescape_decode};
