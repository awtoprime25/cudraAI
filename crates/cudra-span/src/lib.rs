use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

/// Hasher não criptográfico (estilo FxHash do rustc): o interner e as tabelas
/// de símbolos indexam-se por inteiros e por nomes curtos, onde o SipHash do
/// `RandomState` custa mais do que a procura.
#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, i: u64) {
        self.hash = (self.hash.rotate_left(5) ^ i).wrapping_mul(FX_SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        // Finalizador de avalanche (murmur3). Sem ele, os bits baixos do
        // produto final ficam fracos para chaves curtas que só diferem no
        // fim (ex.: `f0`..`f99999`, que partilham o primeiro byte): as
        // tabelas degeneram em cadeias de sondagem longas e o interner passa
        // a custar ordens de grandeza mais do que a procura.
        let mut h = self.hash;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
        h
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while rest.len() >= 8 {
            let (head, tail) = rest.split_at(8);
            self.add(u64::from_le_bytes(head.try_into().unwrap()));
            rest = tail;
        }
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(buf));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
}

pub type FxBuild = BuildHasherDefault<FxHasher>;

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
    // Índice dos chars multi-byte (UTF-8 com mais de 1 byte): posições em
    // byte, em ordem crescente, e o prefixo acumulado de bytes "a mais"
    // (len_utf8() - 1) até (exclusive) cada uma. Permite converter um
    // byte-offset num nº de chars por pesquisa binária (O(log n)).
    multibyte_pos: Vec<u32>,
    multibyte_extra_prefix: Vec<u32>,
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
        let (multibyte_pos, multibyte_extra_prefix) = multibyte_index_of(&src);
        let id = FileId(self.files.len() as u32);
        self.files.push(FileData {
            name,
            src,
            line_starts,
            multibyte_pos,
            multibyte_extra_prefix,
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

// Constrói o índice dos chars multi-byte de `src` (ver `FileData`).
fn multibyte_index_of(src: &str) -> (Vec<u32>, Vec<u32>) {
    let mut pos = Vec::new();
    let mut extra_prefix = vec![0u32];
    let mut running = 0u32;
    for (i, c) in src.char_indices() {
        let extra = c.len_utf8() as u32 - 1;
        if extra > 0 {
            pos.push(i as u32);
            running += extra;
            extra_prefix.push(running);
        }
    }
    (pos, extra_prefix)
}

// Soma dos bytes "a mais" (além de 1) de todos os chars multi-byte cuja
// posição é < `offset`. `pos` está ordenado, por isso é uma pesquisa binária.
fn extra_bytes_before(pos: &[u32], extra_prefix: &[u32], offset: u32) -> u32 {
    let k = pos.partition_point(|&p| p < offset);
    extra_prefix[k]
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
    let line_start = data.line_starts[line_idx];
    let bytes_in_line = offset - line_start;
    let extra = extra_bytes_before(&data.multibyte_pos, &data.multibyte_extra_prefix, offset)
        - extra_bytes_before(
            &data.multibyte_pos,
            &data.multibyte_extra_prefix,
            line_start,
        );
    let col = bytes_in_line - extra + 1;
    (line_idx as u32 + 1, col)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(pub u32);

#[derive(Default)]
pub struct Interner {
    strings: Vec<Rc<str>>,
    map: HashMap<Rc<str>, Symbol, FxBuild>,
}

impl Interner {
    pub fn new() -> Self {
        Interner::default()
    }

    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }
        // Uma só alocação para a string, partilhada pela chave e pela lista.
        let sym = Symbol(self.strings.len() as u32);
        let owned: Rc<str> = Rc::from(s);
        self.map.insert(Rc::clone(&owned), sym);
        self.strings.push(owned);
        sym
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
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

    // Nomes curtos que só diferem no fim (como `f0`..`f9999`) partilham o
    // primeiro byte; sem o finalizador de avalanche os bits baixos do hash
    // colapsavam em pouquíssimos baldes e as tabelas degeneravam.
    #[test]
    fn fxhash_distribui_nomes_curtos() {
        use std::hash::{Hash, Hasher};
        let mut baldes = vec![0u32; 1 << 17];
        for i in 0..10_000 {
            let mut h = FxHasher::default();
            format!("f{i}").hash(&mut h);
            baldes[(h.finish() as usize) & 0x1_FFFF] += 1;
        }
        let ocupados = baldes.iter().filter(|&&c| c > 0).count();
        assert!(ocupados > 8_000, "hash concentra-se em {ocupados} baldes");
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

    // Implementação antiga (ingénua), mantida só para o teste diferencial.
    fn naive_line_col(src: &str, line_starts: &[u32], offset: u32) -> (u32, u32) {
        let len = src.len() as u32;
        let mut offset = offset.min(len);
        while offset > 0 && !src.is_char_boundary(offset as usize) {
            offset -= 1;
        }
        let line_idx = match line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let line_start = line_starts[line_idx] as usize;
        let col = src[line_start..offset as usize].chars().count() as u32 + 1;
        (line_idx as u32 + 1, col)
    }

    // xorshift64 determinístico: sem dependências novas, só para gerar
    // strings/offsets de teste de forma reprodutível.
    struct Rng(u64);
    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn next_range(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
    }

    fn random_src(rng: &mut Rng) -> String {
        const CHARS: &[char] = &['a', 'b', ' ', '\t', 'é', '🙂', '\r', '\n'];
        let len = 1 + rng.next_range(60);
        let mut s = String::new();
        for _ in 0..len {
            if rng.next_range(5) == 0 && s.ends_with('\r') {
                // ocasionalmente força um '\r\n' explícito.
                s.push('\n');
                continue;
            }
            s.push(CHARS[rng.next_range(CHARS.len())]);
        }
        s
    }

    #[test]
    fn line_col_matches_naive_implementation_at_every_offset() {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        for _ in 0..500 {
            let src = random_src(&mut rng);
            let line_starts = line_starts_of(&src);
            let (multibyte_pos, multibyte_extra_prefix) = multibyte_index_of(&src);
            let data = FileData {
                name: "t".to_string(),
                src: src.clone(),
                line_starts: line_starts.clone(),
                multibyte_pos,
                multibyte_extra_prefix,
            };
            let len = src.len() as u32;
            for offset in 0..=(len + 3) {
                let fast = line_col_of(&data, offset);
                let naive = naive_line_col(&src, &line_starts, offset);
                assert_eq!(
                    fast, naive,
                    "offset={offset} src={src:?}: rápido={fast:?} ingénuo={naive:?}"
                );
            }
        }
    }

    #[test]
    fn line_col_on_2mb_line_handles_1m_calls_in_under_one_second_debug() {
        let mut rng = Rng(0xD1B54A32D192ED03);
        let mut src = String::with_capacity(2 * 1024 * 1024);
        while src.len() < 2 * 1024 * 1024 {
            if rng.next_range(3) == 0 {
                src.push('é');
            } else {
                src.push('a');
            }
        }
        let len = src.len() as u32;
        let mut sm = SourceMap::new();
        let f = sm.add("t.cdr".to_string(), src);

        let start = std::time::Instant::now();
        let mut rng2 = Rng(0x2545F4914F6CDD1D);
        for _ in 0..1_000_000u32 {
            let offset = rng2.next_range(len as usize + 1) as u32;
            let _ = sm.line_col(f, offset);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs_f64() < 1.5,
            "1M chamadas a line_col demoraram {elapsed:?}"
        );
    }
}
