// Pilha de níveis de indentação. Só guarda níveis que vieram de um Indent
// real, para garantir #Indent == #Dedent mesmo com erros de recuperação.
pub(crate) struct IndentStack {
    levels: Vec<u32>,
}

impl IndentStack {
    pub(crate) fn new() -> Self {
        IndentStack { levels: vec![0] }
    }

    pub(crate) fn top(&self) -> u32 {
        // invariante: `levels` nunca fica vazio (o `0` inicial nunca é retirado).
        *self.levels.last().unwrap_or(&0)
    }

    pub(crate) fn push(&mut self, level: u32) {
        self.levels.push(level);
    }

    // Desce até `level`. Devolve (nº de Dedents, se a pilha não aterrou
    // exatamente em `level` — indentação inconsistente).
    pub(crate) fn dedent_to(&mut self, level: u32) -> (u32, bool) {
        let mut count = 0u32;
        while self.levels.len() > 1 && self.top() > level {
            self.levels.pop();
            count += 1;
        }
        (count, self.top() != level)
    }

    // Usado no EOF: fecha todos os níveis pendentes.
    pub(crate) fn remaining_dedents(&mut self) -> u32 {
        let mut count = 0u32;
        while self.levels.len() > 1 {
            self.levels.pop();
            count += 1;
        }
        count
    }
}
