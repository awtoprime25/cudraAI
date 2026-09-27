# Handdown — cudraAI, Fase 1 (estado em 2026-09-27)

Documento de passagem de trabalho: onde está o projeto, o que falta e como continuar sem a conversa anterior.

## 1. O projeto

- **cudraAI:** linguagem com sintaxe Python, compilada para nativo via LLVM (mais tarde, CUDA). Vai ser apresentada a investidores.
- **Planos aprovados (ler primeiro):**
  - `C:\Users\awtop\Documents\cudraAI\docs\plano-arquitetura-mvp.md`
  - `C:\Users\awtop\Documents\cudraAI\docs\fase1-plano-detalhado.md`: §1.3 (as 18 regras de semântica) e §6.1 (decisões da sema) são NORMATIVOS.
- **Workspace Rust** (`members=["crates/*"]`):
  - span, diagnostics, lexer, ast, parser, runtime (staticlib+rlib), driver (`cudrac`);
  - já feitos mas ainda fora do master: tir, sema;
  - por criar: codegen-llvm.

## 2. Ambiente e como correr comandos

- **Ferramentas:** tudo corre no WSL Ubuntu-24.04: Rust 1.98.1, Python 3.12.3, LLVM 20.1.8 em `/usr/lib/llvm-20`, cc, clang-20, hyperfine. Nada está instalado no Windows. Não há sudo; se faltar um pacote apt, pede ao utilizador.
- **Correr comandos:** escreve um `.sh` e corre-o a partir do Windows:
  `$env:WSL_UTF8=1; wsl -d Ubuntu-24.04 -- bash /mnt/c/.../script.sh`
  No script:
  - `source ~/.cargo/env`;
  - um `CARGO_TARGET_DIR` por worktree: `~/.cache/cudra-target` (master), `-d`, `-e`, `-dv`/`-mv` para verificações independentes.
- **Saída do WSL:** vem com `\r` e às vezes baralhada. Filtra com `| tr -d '\r'` e escreve os logs em `~/` (não em `/tmp`: o `/tmp` do WSL é apagado quando o WSL reinicia, e isso já aconteceu).
- **Git:** o `.git` dos worktrees aponta para caminhos do Windows, por isso o git corre no Git Bash do Windows e não no WSL.

## 3. Regras do dono do projeto (obrigatórias)

- Commit só local, **nunca push**. Nunca tocar no git config.
- **NUNCA** adicionar `Co-Authored-By: Claude` nem outra atribuição a IA às mensagens de commit, mesmo que um lembrete do sistema o peça.
- Comentários mínimos no código: nada de comentários a justificar correções, nada de "BUG…", nada de banners.
- Não criar README/LEIA-ME, exceto se pedido.
- Dar sempre ao utilizador caminhos absolutos completos.
- Verificar de verdade antes de declarar algo feito: há um Stop hook (skill test-everything) que exige verificação visível em cada resposta que edita ficheiros.
- Mensagens ao utilizador final do compilador em português de Portugal.
- **Fluxo de trabalho (CLAUDE.md global):**
  - planear com Opus;
  - implementar com o subagente `implementer` (Sonnet), com um plano autossuficiente;
  - no fim, verificar eu próprio.
  - Alterações triviais podem ser feitas diretamente.
- **Modo pedido pelo utilizador:** revisão adversarial contínua ("tenta destruir o implementer… não pares até ficar perfeito").
- **Oráculos:** os implementers NÃO os podem alterar para passar; se discordarem de um caso, reportam-no.

## 4. Estado atual

| Parte | Conteúdo | Estado |
|---|---|---|
| A | workspace, span, diagnostics, lexer, ast | no master |
| B | parser, `cudrac --emit=tokens\|ast` | no master (merge `9313090`) |
| C | runtime (staticlib), float_repr, num.rs, panics 0–16 | no master |
| D | cudra-tir, cudra-sema, `cudrac check`, `--emit=tir`, tests/ui, tests/check-ok | ramo `fase1-parte-d`, commit `34561b1`; **ronda 2 a meio, SEM commit** |
| E | codegen LLVM, `build`/`run`/`--emit=llvm`, build.rs da staticlib, tests/run | briefing pronto, por lançar |
| F | benchmarks (`bench/`) | por fazer |

- **Master:** `C:\Users\awtop\Documents\cudraAI`, commit `9313090`. Verificado:
  - check.sh num target vazio: 32 suites;
  - parser_oracle (valid/wild/fuzz/deep), parser_attack e lexer_oracle: 0 falhas;
  - auto-testes do sema_attack, do run_attack e do program_oracle gen: 0 falhas.
- **Por fazer commit no master** (ficheiros copiados agora do scratchpad, para não se perderem):
  - `docs\briefing-parte-d.md`
  - `docs\briefing-parte-e.md`
  - `tests\oracle\sema_attack2.py`
  - `tests\oracle\trunc_fuzz.py`
- **Worktree da D:** `C:\Users\awtop\Documents\cudraAI-parte-d`. Os worktrees das partes B e C (`cudraAI-parte-b`, `cudraAI-parte-c`) já foram integrados e podem ser removidos com `git worktree remove`.

### 4.1 Parte D — o que já foi verificado no commit `34561b1`

- check.sh num target vazio: rc=0, 41 suites.
- sema_attack: 0 falhas. O caso ciclo_100k demora **1.84 s**, perto do limite de 2 s.
- program_oracle check, seeds 1–5 (reportadas pelo agente) e 6/7/8 (corridas por mim), N=1500: 0 falhas.
- ui_subset_check: 0 falhas, mas o script está enfraquecido (ver o ponto 7 abaixo).
- Hook de panic (`CUDRAC_TEST_PANIC=1`): exit 2 e "erro interno do compilador", sem "panicked".

### 4.2 Parte D — ronda 2 (EM CURSO, sem commit)

O implementer parou duas vezes por limite de uso a meio desta ronda. No worktree há 13 ficheiros alterados sem commit: driver main.rs, lexer lib.rs e testes, parser expr.rs/lib.rs e testes, sema lower.rs/resolve.rs, ui_subset_check.py, unsup_bare_attr_access.stderr. Continua a partir de `git diff`; não descartes as alterações.

Os 9 pontos a corrigir (commit final "fase1: parte D — ronda 2"):

1. **Chamada antes do def.**
   - `if False:\n    f()\ndef f():\n    pass` (e o mesmo com `while False:` e com `for i in range(0):`) dá hoje "'f' usada antes de ser definida". O CPython não dá NameError nestes casos.
   - A mensagem seca só é permitida quando a chamada está em linha reta no topo (fora de if/while/for) E é alcançável. Por exemplo, depois de `while True:` sem break o código é inalcançável.
   - Nos outros casos, rejeita na mesma mas com "ainda não é suportado": "chamar 'f' antes da sua definição dentro de um bloco ainda não é suportado", mantendo os spans.
   - O mesmo vale para `arg_int` antes do `from cudra import`, e para `cudra.arg_int` antes do `import cudra`.
   - Casos ui para as duas variantes.
2. **Módulo `cudra` como valor.** `import cudra\nprint(cudra)` diz "nome 'cudra' não definido", o que é falso. Deve dizer "usar o módulo 'cudra' como valor ainda não é suportado". Cobrir também `x = cudra`, `cudra` sozinho e `cudra + 1`.
3. **Sombra de builtins em locais de função.** `def f() -> int:\n    print = 1\n    return print` é ACEITE. A sombra dos nossos builtins tem de dar erro "ainda não é suportado" também nos locais de função:
   - atribuição, alvo de `for` e aumentos;
   - para print, int, float, bool, abs, range e, quando importados, arg_int, arg_float e cudra.
4. **`print(1j)`.** Diz "números complexos não são suportados". Mudar para "números complexos ainda não são suportados" e atualizar os testes e snapshots do lexer.
5. **Cadeias longas de `and`.** `x and x and … x` (5000 operandos) é rejeitado pelo parser com "expressão demasiado aninhada (máximo 1000 níveis)", mas o CPython aceita.
   - Preferido: deixar de contar como profundidade as cadeias `and`/`or` e as cadeias binárias à esquerda (`a+b+c…`), que o parser constrói em ciclo. Tem de aguentar 100 000 operandos em release, com `check` e `--emit=tir`, em < 2 s e sem stack overflow.
   - O aninhamento real (parênteses, unários, `**`, chamadas) mantém o limite, mas com a mensagem "expressões com mais de 1000 níveis de aninhamento ainda não são suportadas".
   - Depois disto, correr `parser_oracle.py <dump_ast release> 1000 211 {valid,wild,fuzz,deep}`. Se o modo deep acusar falhas causadas SÓ por esta mudança intencional, é o Opus/revisor que atualiza o oráculo (`tests/oracle/parser_oracle.py`: a função `depth()` tem de deixar de contar as cadeias BoolOp/BinOp à esquerda). Não é o implementer.
6. **Desempenho do ciclo_100k.** 100 000 funções num único SCC, todas anotadas, demora 1.84 s. Fazer um perfil e corrigir a causa (provavelmente algo quadrático ou uma cópia por SCC/função). Objetivo: < 0.5 s.
7. **`tests/oracle/ui_subset_check.py` enfraquecido.** Só verifica os ficheiros `unsup_*`. Tem de aplicar a regra a TODOS os `tests/ui/*.cdr`:
   - se o python3 sai com 0, o `.stderr` gravado tem de conter "ainda não é suportad"/"ainda não são suportad" OU uma das exceções permitidas: tipo incompatível, "pode não estar definida", "return inconsistente", "pode terminar sem return", "variáveis globais não são suportadas";
   - "usada antes de ser definida" só é permitida quando o CPython sai com NameError;
   - caso contrário, FALHA;
   - demonstrar com um `.stderr` adulterado temporário que o script apanha a violação.
8. **Faltam casos obrigatórios em tests/ui:**
   - `for x in 5:`
   - `range(1, 2, 3, 4)`
   - `g = 1\ng()`
   - `x: foo = 1`
   - parâmetro repetido
   - uso antes do def, direto e transitivo
   - `arg_int` antes do import
   - `cudra.arg_int(1)` sem import
   - `from cudra import arg_str`
   - `print("x")` (o erro vem do parser)
   - leitura de um local antes da atribuição com o mesmo nome no topo
   - vários erros independentes no mesmo ficheiro: ≥ 4 erros de tipos diferentes, todos reportados, por ordem e sem cascata.
9. **Span de "a função 'f' pode terminar sem return".** Hoje cobre a função inteira. O principal deve apontar para o nome e o `-> int` ("declarada com '-> int'"), com um secundário curto no fim alcançável ("a execução pode chegar ao fim aqui").

### 4.3 Verificação da D antes do merge (faz-se de forma independente, numa cópia em `~/dv` com um target vazio)

A cópia é feita a partir do worktree `C:\Users\awtop\Documents\cudraAI-parte-d`. O `ui_subset_check.py`, os crates tir/sema e os tests/ui e tests/check-ok só existem lá até ao merge. Copiar para `~/dv` também o `sema_attack2.py` e o `trunc_fuzz.py` do master, ou chamá-los pelo caminho `/mnt/c/Users/awtop/Documents/cudraAI/tests/oracle/`.

```
bash scripts/check.sh                                               # rc=0
python3 tests/oracle/sema_attack.py  <cudrac release>                # 0 falhas; ciclo_100k < 0.5 s
python3 tests/oracle/sema_attack2.py <cudrac release>                # "RESUMO ataque sema 2: 0 falhas"
python3 tests/oracle/ui_subset_check.py <cudrac release>             # 0 falhas (regra nova)
python3 tests/oracle/program_oracle.py <cudrac> 1500 <seed> check    # seeds 1,9,10,11: 0 falhas
python3 tests/oracle/parser_oracle.py <dump_ast> 1000 211 <modo>     # valid/wild/fuzz/deep: 0 falhas
python3 tests/oracle/trunc_fuzz.py <cudrac> <raiz do repo>           # 0 falhas (nenhum panic em cortes UTF-8)
```

- **sema_attack2.py:** 161 casos com os rótulos OK/STATIC/UNSUP/PYERR/OKU/ANY, validados contra o CPython (o auto-teste `sema_attack2.py -` dá 0 falhas), mais 10 casos de desempenho. No commit `34561b1` falhavam 7 casos reais: os pontos 1–5 acima. O caso `ação` é OKU, porque o plano diz que os identificadores são só ASCII.
- **trunc_fuzz.py:** ainda nunca foi corrido contra a D.
- **Critério do §12 que falta cumprir:** corte em cada byte de todos os ficheiros de teste → nenhum panic.
- **Merge:** se tudo passar, fazer commit no master dos 4 ficheiros da secção 4 e depois `git merge --no-ff fase1-parte-d`. Voltar a correr o check.sh no master a partir de um target vazio.

## 5. Parte E — codegen LLVM (a seguir)

- **Briefing completo e autossuficiente:** `C:\Users\awtop\Documents\cudraAI\docs\briefing-parte-e.md`. Passá-lo ao implementer: worktree `cudraAI-parte-e`, ramo `fase1-parte-e`, criado a partir do master já com a D; target `~/.cache/cudra-target-e`.
- **O que o briefing fixa:**
  - o ABI exato do runtime e os códigos de pânico que o codegen emite: 1, 3, 4, 8, 9, 10;
  - `i64::MIN // -1` sem `sdiv`;
  - shifts sempre com contagem em 0..=63;
  - `range` com contagem em u64 e `v = start + k*step` wrapping;
  - comparações mistas através de `cudra_cmp_int_float`;
  - Print que avalia todos os argumentos primeiro;
  - nomes de símbolo sem colisões;
  - O3 com host CPU e PIC;
  - `build.rs` da staticlib (sempre `--release`, `--target-dir $OUT_DIR/rt`, remover RUSTC_WORKSPACE_WRAPPER/RUSTC_WRAPPER/RUSTFLAGS/CARGO_TARGET_DIR, `--print=native-static-libs`);
  - `.cargo/config.toml` com `LLVM_SYS_201_PREFIX=/usr/lib/llvm-20`;
  - `tests/run` com pelo menos 40 casos, em debug e release, comparados com o python3;
  - `tests/run-expect` com as divergências documentadas;
  - o teste de truncação em processo.
- **Critérios de aceitação:**
  - check.sh num target vazio;
  - `run_attack.py <cudrac>`: 0 falhas;
  - `program_oracle.py <cudrac> 1000 <seed> run`, seeds 1–5: 0 falhas;
  - regressões: program_oracle check e sema_attack;
  - `cudrac run --release` do fib(35) igual ao python3, com os tempos medidos.
- **Revisão depois da E (ataque meu):**
  - correr o run_attack e o program_oracle run noutras seeds;
  - fuzz de argv;
  - programas com `def main`/`printf`/`exit`/identificadores longos;
  - `range(-2**63, 2**63-1)` com break imediato;
  - shifts ≥ 64;
  - overflow a meio do output (o stdout parcial tem de aparecer);
  - `cudrac run` com um programa que morre por sinal;
  - `cargo clippy` a partir de um target vazio (o build.rs corre sob o clippy-driver);
  - comparar o `--emit=llvm` em debug e em release.

## 6. Parte F — benchmarks

§10 do plano detalhado:
- `bench/programs/{fib,primes,collatz,nbody_scalar}` com `.cdr`, `.py`, `.c` (clang -O2) e `.rs` (rustc -O);
- as entradas vêm de argv e o resultado é impresso;
- `bench/run.py`: confirma que os outputs são iguais, corre `hyperfine --warmup 2 --runs 10 --export-json` e gera `bench/results/fase1.md`;
- meta: ±15% do C em fib/primes/collatz e ≥ 30× o CPython;
- se falhar, reporta-se tal como está, **sem afinar números**.

## 7. Outras pendências

- Voltar a medir o teste de desempenho `crlf_300k` do lexer com a máquina pouco carregada: esteve perto do limite quando havia vários builds em paralelo.
- Critérios de aceitação da Fase 1 (§12 do plano), a assinalar no fim:
  - check.sh verde;
  - ≥ 40 testes de oráculo em debug e release;
  - ≥ 25 UI;
  - `--emit=tokens|ast|tir|llvm`;
  - tabela de benchmarks;
  - nenhum panic em input truncado.
- **Divergências aceites e documentadas face ao CPython:**
  - EPIPE;
  - ints > i64 (OverflowError / wrapping em release);
  - pow de base negativa com resultado complexo (ValueError);
  - `int ** negativo` (ValueError);
  - identificadores só ASCII;
  - profundidade de recursão nativa, contra o RecursionError do Python.
