pub(crate) fn pad(v: &mut Vec<bool>, len: usize) {
    if v.len() < len {
        v.resize(len, false);
    }
}

pub(crate) fn and_into(a: &mut Vec<bool>, b: &[bool]) {
    pad(a, b.len());
    for (i, slot) in a.iter_mut().enumerate() {
        let bv = b.get(i).copied().unwrap_or(false);
        *slot = *slot && bv;
    }
}
