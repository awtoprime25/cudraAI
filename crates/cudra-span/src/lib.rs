use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub lo: u32,
    pub hi: u32,
}

impl Span {
    pub fn new(file: FileId, lo: u32, hi: u32) -> Self {
        debug_assert!(lo <= hi);
        Span { file, lo, hi }
    }

    pub fn to(self, other: Span) -> Span {
        if self.file != other.file {
            return self;
        }
        Span {
            file: self.file,
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }

    pub fn len(self) -> u32 {
        self.hi - self.lo
    }

    pub fn is_empty(self) -> bool {
        self.hi == self.lo
    }
}

struct FileData {
    name: String,
    src: String,
    line_starts: Vec<u32>,
}

#[derive(Default)]
pub struct SourceMap {
    files: Vec<FileData>,
}

impl SourceMap {
    pub fn new() -> Self {
        SourceMap::default()
    }

    pub fn add(&mut self, name: String, src: String) -> FileId {
        let line_starts = line_starts_of(&src);
        let id = FileId(self.files.len() as u32);
        self.files.push(FileData {
            name,
            src,
            line_starts,
        });
        id
    }

    pub fn name(&self, file: FileId) -> &str {
        &self.files[file.0 as usize].name
    }

    pub fn source(&self, file: FileId) -> &str {
        &self.files[file.0 as usize].src
    }

    pub fn line_col(&self, file: FileId, offset: u32) -> (u32, u32) {
        line_col_of(&self.files[file.0 as usize], offset)
    }

    pub fn get_name(&self, file: FileId) -> Option<&str> {
        self.files.get(file.0 as usize).map(|f| f.name.as_str())
    }

    pub fn get_source(&self, file: FileId) -> Option<&str> {
        self.files.get(file.0 as usize).map(|f| f.src.as_str())
    }

    pub fn get_line_col(&self, file: FileId, offset: u32) -> Option<(u32, u32)> {
        Some(line_col_of(self.files.get(file.0 as usize)?, offset))
    }
}

// `\r\n` conta como uma só quebra de linha; um `\r` sozinho (não seguido de
// `\n`) também é uma quebra, tal como `\n`.
fn line_starts_of(src: &str) -> Vec<u32> {
    let bytes = src.as_bytes();
    let mut line_starts = vec![0u32];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line_starts.push((i + 1) as u32);
                i += 1;
            }
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => i += 1,
            b'\r' => {
                line_starts.push((i + 1) as u32);
                i += 1;
            }
            _ => i += 1,
        }
    }
    line_starts
}

fn line_col_of(data: &FileData, offset: u32) -> (u32, u32) {
    let len = data.src.len() as u32;
    let mut offset = offset.min(len);
    while offset > 0 && !data.src.is_char_boundary(offset as usize) {
        offset -= 1;
    }
    let line_idx = match data.line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i - 1,
    };
    let line_start = data.line_starts[line_idx] as usize;
    let col = data.src[line_start..offset as usize].chars().count() as u32 + 1;
    (line_idx as u32 + 1, col)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Symbol(pub u32);

#[derive(Default)]
pub struct Interner {
    strings: Vec<String>,
    map: HashMap<String, Symbol>,
}

impl Interner {
    pub fn new() -> Self {
        Interner::default()
    }

    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }
        let sym = Symbol(self.strings.len() as u32);
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), sym);
        sym
    }

    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_to_merges_range() {
        let f = FileId(0);
        let a = Span::new(f, 5, 10);
        let b = Span::new(f, 2, 8);
        let merged = a.to(b);
        assert_eq!(merged, Span::new(f, 2, 10));
    }

    #[test]
    fn span_to_keeps_self_unchanged_when_different_file() {
        let a = Span::new(FileId(0), 5, 10);
        let b = Span::new(FileId(1), 2, 8);
        let merged = a.to(b);
        assert_eq!(merged, a);
    }

    #[test]
    #[should_panic]
    fn span_new_rejects_lo_greater_than_hi() {
        let _ = Span::new(FileId(0), 5, 3);
    }

    #[test]
    fn span_len_and_is_empty() {
        let s = Span::new(FileId(0), 3, 3);
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
        let s2 = Span::new(FileId(0), 3, 7);
        assert_eq!(s2.len(), 4);
        assert!(!s2.is_empty());
    }

    #[test]
    fn line_col_basic() {
        let mut sm = SourceMap::new();
        let f = sm.add("a.cdr".to_string(), "abc\ndef\nghi".to_string());
        assert_eq!(sm.line_col(f, 0), (1, 1));
        assert_eq!(sm.line_col(f, 3), (1, 4));
        assert_eq!(sm.line_col(f, 4), (2, 1));
        assert_eq!(sm.line_col(f, 7), (2, 4));
        assert_eq!(sm.line_col(f, 8), (3, 1));
    }

    #[test]
    fn line_col_clamps_past_end() {
        let mut sm = SourceMap::new();
        let f = sm.add("a.cdr".to_string(), "abc".to_string());
        assert_eq!(sm.line_col(f, 3), (1, 4));
        assert_eq!(sm.line_col(f, 1000), (1, 4));
    }

    #[test]
    fn line_col_crlf_is_one_break() {
        let mut sm = SourceMap::new();
        let f = sm.add("a.cdr".to_string(), "abc\r\ndef".to_string());
        // '\r' fica na linha 1 como o último char antes do '\n'.
        assert_eq!(sm.line_col(f, 3), (1, 4));
        assert_eq!(sm.line_col(f, 5), (2, 1));
        assert_eq!(sm.line_col(f, 8), (2, 4));
    }

    #[test]
    fn line_col_counts_chars_not_bytes_accents() {
        let mut sm = SourceMap::new();
        // "é" ocupa 2 bytes em UTF-8.
        let f = sm.add("a.cdr".to_string(), "é\ndef".to_string());
        assert_eq!(sm.line_col(f, 2), (1, 2));
        assert_eq!(sm.line_col(f, 3), (2, 1));
    }

    #[test]
    fn line_col_counts_chars_not_bytes_emoji() {
        let mut sm = SourceMap::new();
        // "🙂" ocupa 4 bytes em UTF-8.
        let f = sm.add("a.cdr".to_string(), "🙂x\ndef".to_string());
        assert_eq!(sm.line_col(f, 4), (1, 2));
        assert_eq!(sm.line_col(f, 5), (1, 3));
        assert_eq!(sm.line_col(f, 6), (2, 1));
    }

    #[test]
    fn line_col_lone_cr_is_a_line_break() {
        let mut sm = SourceMap::new();
        let f = sm.add("a.cdr".to_string(), "a\rb".to_string());
        assert_eq!(sm.line_col(f, 2), (2, 1));
    }

    #[test]
    fn line_col_does_not_panic_mid_char_accent() {
        let mut sm = SourceMap::new();
        // "é" ocupa os bytes 0-1; o offset 1 cai a meio do char.
        let f = sm.add("a.cdr".to_string(), "é".to_string());
        assert_eq!(sm.line_col(f, 1), (1, 1));
    }

    #[test]
    fn line_col_does_not_panic_mid_char_emoji() {
        let mut sm = SourceMap::new();
        // "🙂" ocupa os bytes 0-3; os offsets 1..3 caem a meio do char.
        let f = sm.add("a.cdr".to_string(), "🙂".to_string());
        assert_eq!(sm.line_col(f, 1), (1, 1));
        assert_eq!(sm.line_col(f, 2), (1, 1));
        assert_eq!(sm.line_col(f, 3), (1, 1));
        assert_eq!(sm.line_col(f, 4), (1, 2));
    }

    #[test]
    fn get_name_and_get_source_return_none_for_unknown_file() {
        let mut sm = SourceMap::new();
        sm.add("a.cdr".to_string(), "x".to_string());
        assert_eq!(sm.get_name(FileId(99)), None);
        assert_eq!(sm.get_source(FileId(99)), None);
        assert_eq!(sm.get_line_col(FileId(99), 0), None);
    }

    #[test]
    fn get_name_and_get_source_return_none_on_empty_source_map() {
        let sm = SourceMap::new();
        assert_eq!(sm.get_name(FileId(0)), None);
        assert_eq!(sm.get_source(FileId(0)), None);
        assert_eq!(sm.get_line_col(FileId(0), 0), None);
    }

    #[test]
    fn interner_same_string_same_symbol() {
        let mut it = Interner::new();
        let a = it.intern("total");
        let b = it.intern("total");
        let c = it.intern("outro");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(it.resolve(a), "total");
        assert_eq!(it.resolve(c), "outro");
    }
}
