// Buffer de output com flush manual para o fd 1 (stdout).
use std::io::Write;
use std::sync::Mutex;

const BUF_CAP: usize = 1 << 20;

struct OutState {
    buf: Vec<u8>,
    error: bool,
}

static OUTPUT: Mutex<OutState> = Mutex::new(OutState {
    buf: Vec::new(),
    error: false,
});

fn flush_locked(state: &mut OutState) {
    if state.buf.is_empty() {
        return;
    }
    if state.error {
        state.buf.clear();
        return;
    }
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    if lock.write_all(&state.buf).is_err() || lock.flush().is_err() {
        state.error = true;
    }
    state.buf.clear();
}

/// Acrescenta bytes ao buffer; faz flush sozinho quando fica cheio.
/// Depois de um erro de escrita, descarta tudo o que vier a seguir.
pub(crate) fn push(data: &[u8]) {
    let mut state = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    if state.error {
        return;
    }
    if state.buf.capacity() == 0 {
        state.buf.reserve(BUF_CAP);
    }
    state.buf.extend_from_slice(data);
    if state.buf.len() >= BUF_CAP {
        flush_locked(&mut state);
    }
}

/// Faz flush do buffer para o stdout. Devolve false se alguma vez houve erro de escrita.
pub(crate) fn flush() -> bool {
    let mut state = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    flush_locked(&mut state);
    !state.error
}

#[no_mangle]
pub extern "C" fn cudra_print_int(v: i64) {
    push(v.to_string().as_bytes());
}

#[no_mangle]
pub extern "C" fn cudra_print_float(v: f64) {
    push(crate::repr::float_repr(v).as_bytes());
}

#[no_mangle]
pub extern "C" fn cudra_print_bool(v: bool) {
    push(if v { b"True" } else { b"False" });
}

#[no_mangle]
pub extern "C" fn cudra_print_none() {
    push(b"None");
}

#[no_mangle]
pub extern "C" fn cudra_print_sep() {
    push(b" ");
}

#[no_mangle]
pub extern "C" fn cudra_print_end() {
    push(b"\n");
}

#[no_mangle]
pub extern "C" fn cudra_rt_exit() -> i32 {
    if flush() {
        0
    } else {
        1
    }
}
