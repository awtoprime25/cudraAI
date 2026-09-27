# Handdown — cudraAI, Fase 1 (fim da ronda 2 da Parte D, 2026-09-27)

Passagem de trabalho escrita pelo agente **Freebuff** no fim da ronda 2 da Parte D.
O documento anterior, `handdown freebuff.md`, descreve o estado *antes* desta ronda
(Parte D sem commit, 13 ficheiros alterados a meio) e fica como histórico: **este** é o
documento de partida.

## 1. Estado do git (tudo local; nunca push)

- `master` = **`7bcc547`** — `fase1: merge da parte D — TIR, sema, cudrac check e --emit=tir`
  (`--no-ff` do ramo `fase1-parte-d`).
- `fase1-parte-d` = **`989c696`** — `fase1: parte D — ronda 2 (alcance do código morto,
  spans da assinatura e casos ui em falta)` — todo o trabalho desta ronda está aqui.
- `fase1-parte-b` e `fase1-parte-c` já integrados; os worktrees `cudraAI-parte-b`,
  `cudraAI-parte-c` e `cudraAI-parte-d` ainda existem e podem sair com `git worktree remove`.
- Nenhum commit leva atribuição a IA nem toca no git config (regra do dono).

## 2. Os 9 pontos da ronda 2 — o que ficou feito

1. **Chamada antes do def.** O código depois de um terminador (`return`, `while True` sem
   `break`) voltou a ser *verificado* — um `x = 2.5` morto depois de `x = 1` continua a ser
   erro de tipos — mas nunca entra na TIR. O novo campo `FuncChecker.dead` marca-o e as
   mensagens passaram a ter três formas:
   - linha reta no topo e alcançável → `'f' usada antes de ser definida` (seca, implica NameError);
   - dentro de `if`/`while`/`for` → `chamar 'f' antes da sua definição dentro de um bloco ainda não é suportado`;
   - inalcançável → `chamar 'f' antes da sua definição em código inalcançável ainda não é suportado`.
   O mesmo esquema para `arg_int` antes do `from cudra import` e para `cudra.arg_int` antes do
   `import cudra` (`usar '...' antes da sua importação ...`).
2. **Módulo `cudra` como valor.** `x = cudra`, `cudra` sozinho e `cudra + 1` dão
   `usar o módulo 'cudra' como valor ainda não é suportado` (o código já lá estava; faltavam os
   casos ui, agora são 3). `cudra.arg_int(1)` sem import nenhum dá o correto `nome 'cudra' não definido`.
3. **Sombra de builtins em locais de função.** Passou a haver erro também nos aumentos
   (`print += 1`); a atribuição e o alvo do `for` já eram apanhados. Quando o nome sombreado é
   um nome importado a mensagem é `fazer sombra ao nome importado '...' ainda não é suportado`
   (antes dizia "builtin" também para `arg_int`/`cudra`). `check_local_builtin_shadow` devolve
   `bool` para o chamador não somar um segundo diagnóstico.
4. **`print(1j)`** → `números complexos ainda não são suportados` (lexer, testes do lexer e snapshot).
5. **Cadeias longas.** O parser deixou de contar para a profundidade as cadeias `and`/`or` e os
   operadores binários associativos à esquerda (o `**` continua a contar); a mensagem é
   `expressões com mais de 1000 níveis de aninhamento ainda não são suportadas`. Medições:
   100 000 operandos em `and`, `or`, `+` e `<` → `check` 0.02–0.11 s e `--emit=tir` 0.03–0.13 s,
   sem stack overflow. A pilha do driver subiu de 256 MiB para **1 GiB** (a sema e a TIR
   percorrem a árvore por recursão, e estas cadeias são árvores de 100 000 nós).
   `tests/oracle/parser_oracle.py` foi atualizado como previsto (função `depth()`: nova
   `_left_chain()`, que exclui `BoolOp` e `BinOp` que não seja `Pow`); os 4 modos dão 0 falhas.
6. **Desempenho do `ciclo_100k`.** Perfil feito: **já não há nada quadrático** (a causa da
   ronda 1, `scc.contains` por chamada, foi substituída por um `HashSet` por SCC). O que resta
   é custo linear por função, repartido entre parser e sema. Cortei as alocações de `String`
   por nome nos caminhos quentes (`resolve_params`, `resolve_type_expr`, `declare_top_var`).
   No master, máquina ociosa: `ciclo_100k` **0.65 s**, `cadeia_100k_rev` 0.68–0.76 s,
   `100k_linhas` 0.09–0.12 s (era 1.84 s no início da ronda).
   **O objetivo de < 0.5 s NÃO foi cumprido.** Perfil de `ciclo_100k` (instrumentação
   temporária, já removida): lex 22 ms, parser ~180 ms, sema ~310 ms (resolve ~160 ms:
   topo 40, assinaturas+scan 30–40, Tarjan 24–35, recursão+ready 24–36; lower ~170 ms),
   verify 2 ms, e ~130 ms entre arranque do processo, leitura e libertação das estruturas.
   Pistas para quem pegar nisto: um hasher trivial para `HashMap<Symbol, _>` (as chaves são
   u32 sequenciais do interner e o SipHash é o que custa), tabela de funções indexada por
   `FuncId` em vez de mapas por símbolo, reutilizar/poolizar o `FuncChecker` e olhar para o parser.
7. **`ui_subset_check.py`.** A regra passou a aplicar-se a **todos** os `tests/ui/*.cdr`:
   se o CPython aceita, o `.stderr` gravado tem de conter "ainda não é/são suportad" ou uma das
   exceções permitidas (tipo incompatível, "tipo de retorno incompatível", "pode não estar
   definida", "return inconsistente", "pode terminar sem return", "variáveis globais não são
   suportadas"); a frase seca "usada antes de ser definida" só é permitida quando o CPython dá
   NameError. Demonstração feita com `.stderr` adulterados temporários: o script acusa
   `SEM_AINDA_NAO` (2 casos) e `FORWARD_REF_TERSE_SEM_NAMEERROR` (1 caso) e sai com 1; sem eles,
   0 falhas. Os 4 casos ui com a frase seca legítima contam em
   `forward_ref_terse_com_nameerror` (é a prova de que a regra é exercitada).
8. **Casos em falta em `tests/ui`.** 18 novos, de 86 para **104**: `unsup_for_over_int`,
   `unsup_range_too_many_args`, `static_call_non_function`, `unsup_unknown_type_annotation`,
   `unsup_param_repeated`, `unsup_call_before_def_direct`, `unsup_call_before_def_transitive`,
   `unsup_call_before_def_in_block`, `unsup_arg_int_before_import`,
   `unsup_cudra_arg_int_before_import`, `unsup_cudra_arg_int_no_import`, `unsup_from_import_arg_str`,
   `unsup_print_string`, `static_local_shadows_global_read_first`, `static_many_independent_errors`
   (5 erros independentes, por ordem, sem cascata), `unsup_cudra_module_assign`,
   `unsup_cudra_module_bare`, `unsup_cudra_module_plus_int`.
9. **Spans de "pode terminar sem return".** O label principal passou a apontar para o nome e a
   anotação (`declarada com '-> int'`) e há um secundário curto no fim alcançável do corpo
   (`a execução pode chegar ao fim aqui`). Snapshots de `static_maybe_no_return_annotated` e
   `static_while_cond_not_bool` regravados.

**Extra, fora da lista (para revisão):** quando a anotação de retorno não resolve (`def f() -> str`)
já não se soma "pode terminar sem return" ao erro do tipo — e o mesmo para a atribuição: `x: str = 1`
passou a dar **um** erro em vez de dois (`<erro>` deixou de vazar para a mensagem; o local fica com o
tipo do valor). Isto muda `tests/ui/unsup_str_annotation.stderr`.

## 3. Verificação (tudo corrido de verdade, números reais)

No master `7bcc547`, com `CARGO_TARGET_DIR=~/.cache/cudra-target-mv` **apagado antes**:

| Verificação | Resultado |
|---|---|
| `bash scripts/check.sh` (fmt + clippy -D warnings + testes, target vazio) | **rc=0**, 41 suites, 0 FAILED |
| `sema_attack.py` | **0 falhas** (OK 72, STATIC 26, UNSUP 43, PYERR 31) |
| `sema_attack2.py` | **0 falhas** (UNSUP 42, PYERR 16, OK 67, STATIC 24, ANY 2, OKU 10) |
| `ui_subset_check.py` | **0 falhas** (104 casos ui, 36 check-ok) |
| `trunc_fuzz.py` | **0 falhas** (6103 cortes UTF-8) |
| `parser_oracle.py` valid/wild/fuzz/deep (N=1000, seed 211) | **0 falhas** nos 4 modos |
| `program_oracle.py ... 1500 <seed> check` (seeds 1, 2, 3, 9, 10, 11) | **0 falhas** em todas |
| `ciclo_100k` (sema_attack) | 0.65–0.66 s |
| `cadeia_100k_rev` | 0.68–0.76 s |
| `100k_linhas` | 0.09–0.12 s |

Repetido, de forma independente, numa cópia limpa em `~/dv` (WSL) com o target
`~/.cache/cudra-target-dv` apagado: `check.sh` rc=0 e os cinco oráculos/oráculo-ataques
(sema_attack, sema_attack2, ui_subset, trunc_fuzz, parser_oracle) com 0 falhas.

## 4. Notas de ambiente (custaram tempo nesta ronda)

- O shell que corre os comandos expande `$VAR` antes de chegar ao WSL: **não usar variáveis de
  shell** nas linhas de comando; escrever um `.sh` e chamá-lo com
  `wsl -d Ubuntu-24.04 -- bash /mnt/c/.../x.sh`. Precisa de `MSYS_NO_PATHCONV=1` (senão os
  caminhos `/mnt/c/...` são convertidos para caminhos do Windows).
- Os scripts de apoio desta ronda ficaram (ignorados pelo git) em
  `cudraAI-parte-d/.freebuff/` (`fb.sh`, `verify_d.sh`, `demo_ui.sh`, `chains.sh`, `perf2.sh`,
  `newui.sh`, `oracle_seeds.sh`, `tmp/`) e em `cudraAI/.freebuff/fb-master.sh`.
  `fb.sh <build|check|bless|attack1|attack2|ui|trunc|oracle|parser>` é o atalho geral.
- Medições de desempenho só valem com a máquina ociosa: o mesmo `ciclo_100k` varia de 0.65 s a
  0.88 s conforme os builds em paralelo.
- Medir a cópia `~/dv` serve de segunda opinião: não tem `.git`, `target/` nem `.freebuff/`.

## 5. Próximo passo: Parte E (codegen LLVM)

`docs/briefing-parte-e.md` está pronto e autossuficiente (ABI do runtime, códigos de pânico
1/3/4/8/9/10, `i64::MIN // -1` sem `sdiv`, shifts 0..=63, `range` com contagem em u64, comparações
mistas por `cudra_cmp_int_float`, `Print` que avalia tudo primeiro, `build.rs` da staticlib,
`.cargo/config.toml` com `LLVM_SYS_201_PREFIX=/usr/lib/llvm-20`, `tests/run` com ≥40 casos em debug
e release, `tests/run-expect`, critérios de aceitação). Fluxo do dono: planear com Opus, implementar
com o subagente `implementer` num worktree `cudraAI-parte-e` (ramo `fase1-parte-e`, criado a partir
deste master, target `~/.cache/cudra-target-e`) e o revisor atacar depois (run_attack, program_oracle
`run`, fuzz de argv, clippy a partir de um target vazio, `--emit=llvm` em debug e release).

Pendências mais pequenas que sobram desta ronda:
- o objetivo de < 0.5 s do `ciclo_100k` (perfil e pistas no ponto 6 acima);
- `crlf_300k` do lexer continua por medir com a máquina pouco carregada (§7 do handdown anterior);
- os worktrees b/c/d podem ser removidos;
- Parte F (benchmarks) por fazer.
