// Codigos de panico do runtime e o caminho unico de saida por erro:
// flush do stdout, mensagem no stderr, exit(1). Nunca faz unwind/panic! do
// Rust — so process::exit.
use std::io::Write;

pub const PANIC_ZERO_DIV: u32 = 0;
pub const PANIC_INT_OVERFLOW: u32 = 1;
pub const PANIC_NEG_EXPONENT: u32 = 2;
pub const PANIC_NEG_SHIFT: u32 = 3;
pub const PANIC_RANGE_STEP_ZERO: u32 = 4;
pub const PANIC_NAN_TO_INT: u32 = 5;
pub const PANIC_INF_TO_INT: u32 = 6;
pub const PANIC_FLOAT_TOO_LARGE: u32 = 7;
pub const PANIC_FLOAT_DIV_ZERO: u32 = 8;
pub const PANIC_INT_FLOORDIV_ZERO: u32 = 9;
pub const PANIC_INT_MOD_ZERO: u32 = 10;
pub const PANIC_FLOAT_FLOORDIV_ZERO: u32 = 11;
pub const PANIC_FLOAT_MOD_ZERO: u32 = 12;
pub const PANIC_ZERO_NEG_POWER: u32 = 13;
pub const PANIC_POW_OVERFLOW: u32 = 14;
pub const PANIC_NEG_BASE_FRACTIONAL_POW: u32 = 15;
pub const PANIC_COMPLEX_POW_OVERFLOW: u32 = 16;

fn known_message(code: u32) -> Option<&'static str> {
    match code {
        PANIC_ZERO_DIV => Some("ZeroDivisionError: division by zero"),
        PANIC_INT_OVERFLOW => Some("OverflowError: integer overflow"),
        PANIC_NEG_EXPONENT => Some("ValueError: negative int exponent not supported"),
        PANIC_NEG_SHIFT => Some("ValueError: negative shift count"),
        PANIC_RANGE_STEP_ZERO => Some("ValueError: range() arg 3 must not be zero"),
        PANIC_NAN_TO_INT => Some("ValueError: cannot convert float NaN to integer"),
        PANIC_INF_TO_INT => Some("OverflowError: cannot convert float infinity to integer"),
        PANIC_FLOAT_TOO_LARGE => Some("OverflowError: float too large to convert to int"),
        PANIC_FLOAT_DIV_ZERO => Some("ZeroDivisionError: float division by zero"),
        PANIC_INT_FLOORDIV_ZERO => Some("ZeroDivisionError: integer division or modulo by zero"),
        PANIC_INT_MOD_ZERO => Some("ZeroDivisionError: integer modulo by zero"),
        PANIC_FLOAT_FLOORDIV_ZERO => Some("ZeroDivisionError: float floor division by zero"),
        PANIC_FLOAT_MOD_ZERO => Some("ZeroDivisionError: float modulo"),
        PANIC_ZERO_NEG_POWER => Some("ZeroDivisionError: 0.0 cannot be raised to a negative power"),
        PANIC_POW_OVERFLOW => Some("OverflowError: (34, 'Numerical result out of range')"),
        PANIC_NEG_BASE_FRACTIONAL_POW => Some(
            "ValueError: potência de base negativa com expoente fracionário ainda não é suportada",
        ),
        PANIC_COMPLEX_POW_OVERFLOW => Some("OverflowError: complex exponentiation"),
        _ => None,
    }
}

/// Mensagem estatica de um codigo de panico conhecido (para o codegen embutir
/// diretamente). Codigos desconhecidos devolvem o texto generico sem o
/// numero — quem precisa do numero e o `cudra_panic`, que o formata na hora.
pub fn panic_message(code: u32) -> &'static str {
    known_message(code).unwrap_or("SystemError: erro interno do runtime")
}

/// Caminho unico de saida por erro: flush do stdout (erro ignorado — ja nao
/// ha nada a fazer), mensagem + '\n' no stderr, exit(1).
pub(crate) fn exit_with_error(msg: &str) -> ! {
    let _ = crate::out::flush();
    let mut stderr = std::io::stderr();
    let _ = stderr.write_all(msg.as_bytes());
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
    std::process::exit(1);
}

#[no_mangle]
pub extern "C" fn cudra_panic(code: u32) -> ! {
    match known_message(code) {
        Some(msg) => exit_with_error(msg),
        None => exit_with_error(&format!(
            "SystemError: erro interno do runtime (código {code})"
        )),
    }
}
