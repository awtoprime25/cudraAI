use cudra_tir::LocalId;

/// Conjunto dos locais já definidos num `func`/`main`.
///
/// A versão anterior guardava um `Vec<bool>` que era clonado (e intersectado)
/// a cada ramo de `if`/`elif`/`else` e a cada ciclo. Isso custava O(n.º de
/// locais) por ramo e degenerava em O(n²) com dezenas de milhares de
/// variáveis e de `if`s. Aqui há um único estado; um instantâneo é apenas um
/// índice no tronco de anulações, e restaurar só desfaz as definições feitas
/// desde então (nunca percorre locais intocados).
pub(crate) struct Defined {
    /// `cur[i]` é `true` se o local `LocalId(i)` já está definido.
    cur: Vec<bool>,
    /// Índices virados para `true`, por ordem. Um instantâneo é um
    /// comprimento deste tronco. Nenhum índice se repete numa região
    /// contígua: um `true` só volta a ser registado depois de um restauro o
    /// ter reposto a `false` (e esse restauro corta o tronco).
    undo: Vec<u32>,
    /// Marca de época e contagem por local, reutilizadas entre intersecções:
    /// evitam percorrer todos os locais (ou alocar uma tabela) em cada merge.
    stamp: Vec<u32>,
    count: Vec<u32>,
    epoch: u32,
}

/// Posição no tronco de anulações; `Copy` para poder ser restaurado várias
/// vezes (um instantâneo por ramo).
#[derive(Clone, Copy)]
pub(crate) struct Snapshot(usize);

impl Defined {
    pub(crate) fn new() -> Self {
        Defined {
            cur: Vec::new(),
            undo: Vec::new(),
            stamp: Vec::new(),
            count: Vec::new(),
            epoch: 0,
        }
    }

    /// Regista um local novo (não definido), mantendo
    /// `cur.len() == tir_locals.len()`.
    pub(crate) fn push_local(&mut self) {
        self.cur.push(false);
        self.stamp.push(0);
        self.count.push(0);
    }

    #[inline]
    pub(crate) fn get(&self, id: LocalId) -> bool {
        self.cur[id.0 as usize]
    }

    /// Marca o local como definido (só actua na primeira vez: as definições
    /// são irreversíveis dentro de um ramo).
    pub(crate) fn set(&mut self, id: LocalId) {
        let i = id.0 as usize;
        if !self.cur[i] {
            self.cur[i] = true;
            self.undo.push(id.0);
        }
    }

    pub(crate) fn save(&self) -> Snapshot {
        Snapshot(self.undo.len())
    }

    /// Locais definidos desde o instantâneo, sem repetições e por ordem.
    pub(crate) fn since(&self, s: Snapshot) -> &[u32] {
        &self.undo[s.0..]
    }

    /// Repõe o estado do instantâneo (desfazendo as definições posteriores).
    pub(crate) fn restore(&mut self, s: Snapshot) {
        let Self { cur, undo, .. } = self;
        for id in undo.drain(s.0..) {
            cur[id as usize] = false;
        }
    }

    /// Preenche `out` com os locais definidos em **todos** os ramos de
    /// `branches` (cada ramo é uma lista de locais definidos), sem percorrer
    /// locais que nenhum ramo tocou.
    pub(crate) fn intersect(&mut self, branches: &[Vec<u32>], out: &mut Vec<u32>) {
        out.clear();
        if branches.is_empty() {
            return;
        }
        if self.epoch == u32::MAX {
            self.stamp.fill(0);
            self.epoch = 0;
        }
        self.epoch += 1;
        let epoch = self.epoch;
        for b in branches {
            for &id in b {
                let i = id as usize;
                if self.stamp[i] == epoch {
                    self.count[i] += 1;
                } else {
                    self.stamp[i] = epoch;
                    self.count[i] = 1;
                }
            }
        }
        let need = branches.len() as u32;
        let smallest = branches
            .iter()
            .min_by_key(|b| b.len())
            .expect("ramos não vazios");
        for &id in smallest {
            if self.count[id as usize] == need {
                out.push(id);
            }
        }
    }
}
