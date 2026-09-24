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
