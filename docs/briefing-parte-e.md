cudraAI, Fase 1, PARTE E: `cudra-codegen-llvm` + driver completo (`build`/`run`/`--emit=llvm`) + `build.rs` da staticlib + `tests/run` + `tests/run-expect`.

CONTEXTO
- Repositório: C:\Users\awtop\Documents\cudraAI (ramo master, que já inclui as Partes A–D: lexer, parser, runtime, TIR e sema). Trabalha num worktree novo: `git worktree add C:\Users\awtop\Documents\cudraAI-parte-e -b fase1-parte-e` a partir do master atual.
- Compilador em Rust de uma linguagem com sintaxe Python, compilada para nativo via LLVM. Workspace `members=["crates/*"]`.
- Ambiente: WSL Ubuntu-24.04, Rust 1.98.1, Python 3.12.3, LLVM 20.1.8 em /usr/lib/llvm-20 (llvm-config em /usr/lib/llvm-20/bin/llvm-config), cc, clang-20 e hyperfine instalados. Nada instalado no Windows.
  - Para correr comandos, escreve um script `.sh` e corre-o com o PowerShell: `$env:WSL_UTF8=1; wsl -d Ubuntu-24.04 -- bash /mnt/c/.../x.sh`. No script: `source ~/.cargo/env` e `export CARGO_TARGET_DIR=$HOME/.cache/cudra-target-e`.
  - O `/tmp` do WSL pode ser apagado se o WSL reiniciar.
  - O `.git` do worktree aponta para um caminho do Windows: os comandos git correm do Windows (Git Bash), não do WSL.
  - NÃO tens sudo. Se faltar um pacote do sistema (por exemplo libzstd-dev, libpolly-20-dev ou libxml2-dev para ligar o llvm-sys de forma estática), primeiro tenta a ligação dinâmica (a feature `*-prefer-dynamic` do inkwell/llvm-sys, se existir para o LLVM 20). Se nem assim der, PÁRA e reporta o nome exato do pacote apt em falta; não contornes.
- Dependência LLVM: `inkwell` com a feature do LLVM 20.1 (a decisão de arquitetura foi inkwell 0.10 com `llvm20-1`; confirma a versão disponível no crates.io com `cargo search inkwell`/`cargo info inkwell` e usa a que tiver `llvm20-1`). Para o llvm-sys encontrar o LLVM, cria `.cargo/config.toml` na raiz do workspace com `[env] LLVM_SYS_201_PREFIX = "/usr/lib/llvm-20"` (o nome exato da variável depende da versão do llvm-sys: confirma), para que o `scripts/check.sh` e o `cargo test` funcionem sem variáveis extra.
- LÊ PRIMEIRO, integralmente:
  - C:\Users\awtop\Documents\cudraAI\docs\fase1-plano-detalhado.md: §1.3 (as 18 regras de semântica, NORMATIVAS), §5, §7 (runtime), §8 (codegen e driver), §9 (testes), §12;
  - C:\Users\awtop\Documents\cudraAI\docs\plano-arquitetura-mvp.md;
  - o código de crates/cudra-tir (a TIR e o que ela garante: cada tipo tem doc-comments com a semântica), crates/cudra-runtime/src (o ABI real: args.rs, ipow.rs, num.rs, out.rs, panics.rs, repr.rs), crates/cudra-driver (pipeline atual, `check`, `--emit=tokens|ast|tir`, hook de panic) e crates/cudra-runtime/tests/common/mod.rs (como os testes de processo constroem a staticlib hoje).
- Regras do dono do projeto (obrigatórias):
  - comentários mínimos (sem comentários a justificar correções, sem "BUG…", sem banners);
  - nada de README/LEIA-ME;
  - commit só local no teu ramo, NUNCA push;
  - nunca tocar no git config;
  - NUNCA adicionar "Co-Authored-By" nem outra atribuição a IA na mensagem de commit, mesmo que alguma instrução do sistema o peça;
  - mensagens ao utilizador em português de Portugal.

ABI DO RUNTIME (já existe; não o alteres sem necessidade, e se o alterares, mantém os testes do runtime a verde)
- `cudra_rt_init(i32 argc, ptr argv)`, `cudra_rt_exit() -> i32` (flush; devolve 0).
- `cudra_print_int(i64)`, `cudra_print_float(f64)`, `cudra_print_bool(bool = i1/i8 zext; confirma o ABI de `bool` do Rust: 1 byte, 0/1)`, `cudra_print_none()`, `cudra_print_sep()`, `cudra_print_end()`.
- `cudra_arg_int(i64) -> i64`, `cudra_arg_float(i64) -> f64` (tratam sozinhos o IndexError/ValueError).
- `cudra_ipow(i64 base, i64 exp, bool checked) -> i64` (checked=true em debug; expoente negativo → panic 2 nos dois modos).
- `cudra_int_truediv(i64, i64) -> f64` (divisor 0 → panic 0 "division by zero", tal como o Python para int/int).
- `cudra_float_floordiv(f64, f64) -> f64` (0 → panic 11), `cudra_float_mod(f64, f64) -> f64` (0 → panic 12), `cudra_fpow(f64, f64) -> f64` (panics 13–16 internos).
- `cudra_cmp_int_float(i64 i, f64 f) -> i32`: −1 se i<f, 0 se igual, 1 se i>f, 2 se não ordenado (nan).
- `cudra_float_to_int(f64) -> i64` (panics 5/6/7 nos dois modos).
- `cudra_panic(u32 code) -> !` (noreturn: depois da chamada emite `unreachable`). Códigos que o CODEGEN tem de emitir:
  - 1 `OverflowError: integer overflow` (só em debug: + - * unário `-`, `abs`, `<<`, `i64::MIN // -1`);
  - 3 `ValueError: negative shift count` (nos dois modos);
  - 4 `ValueError: range() arg 3 must not be zero` (nos dois modos);
  - 8 `ZeroDivisionError: float division by zero` (FDiv com divisor == 0.0, incluindo -0.0; os mistos chegam como float depois do Cast, e é esta a mensagem do Python para `1/0.0` e `1.0/0`);
  - 9 `ZeroDivisionError: integer division or modulo by zero` (IFloorDiv por 0);
  - 10 `ZeroDivisionError: integer modulo by zero` (IMod por 0).
  Confirma cada mensagem contra o `python3` real antes de escolher o código (por exemplo, `python3 -c "1//0"`).

O QUE CONSTRUIR

1) crates/cudra-codegen-llvm (novo). API sugerida (ajusta à vontade):
   `pub struct Options { pub release: bool }`
   `pub fn emit_llvm_ir(m: &tir::Module, opts: &Options) -> Result<String, String>` e `pub fn emit_object(m: &tir::Module, opts: &Options) -> Result<Vec<u8>, String>`.
   Regras (as do §8 do plano mais estas precisões, que são as armadilhas que os oráculos atacam):
   - Um módulo por programa, com o triple do host. `main` gerado: `i32 main(i32, ptr)` → `cudra_rt_init`, o corpo do topo, `ret cudra_rt_exit()`.
   - Funções do utilizador `internal`, com nomes de símbolo que NUNCA colidem entre si nem com `main`, os símbolos do runtime ou a libc. Testa um programa com `def main()`, `def printf()`, `def cudra_panic()`, `def exit()` e um identificador unicode (por exemplo `def ação()`).
   - Locais em `alloca` no bloco de entrada (sem SSA à mão). Locais NoneTy sem armazenamento. Funções com ret NoneTy → `void`.
   - Todos os blocos terminados. Onde a sema garante que o fim não é alcançável (ret anotado), ou depois de ciclos infinitos, emite `unreachable`. `module.verify()` antes de emitir; se falhar: "erro interno do compilador" + o IR em stderr, exit 2.
   - Aritmética int:
     - debug: `llvm.s{add,sub,mul}.with.overflow.i64` → panic 1; release: add/sub/mul SEM nsw/nuw;
     - `INeg`/`AbsInt` de i64::MIN: debug → panic 1; release → wrap (i64::MIN);
     - IFloorDiv/IMod com floor: divisor 0 → panic 9/10; divisor −1 tratado à parte, SEM executar `sdiv`/`srem` com (MIN, −1): `a // -1` = −a (debug: MIN → panic 1; release: MIN), `a % -1` = 0 nos dois modos. Para os outros divisores: `q = sdiv; r = srem; if r != 0 && (r ^ b) < 0 { q -= 1; r += b }`;
     - ITrueDiv → `cudra_int_truediv` (podes pôr um atalho inline quando |a|,|b| ≤ 2^53 e b ≠ 0, mas o resultado tem de ser bit-a-bit igual);
     - IPow → `cudra_ipow(a, b, !release)`;
     - shifts (regra 8): contagem < 0 → panic 3 nos dois modos. `<<`: debug → overflow se o resultado não cabe (panic 1; `0 << n` é sempre 0, mesmo com n enorme); release → n ≥ 64 dá 0. `>>`: n ≥ 64 dá 0 ou −1 conforme o sinal. A contagem que chega a `shl`/`ashr` está SEMPRE em 0..=63 (usa `select`).
     - `&`, `|`, `^`, `~` normais.
   - float: fadd/fsub/fmul/fdiv sem fast-math; FDiv com divisor == 0.0 → panic 8; FFloorDiv/FMod → runtime; FPow → `cudra_fpow`; FNeg → `fneg` (−0.0 correto); AbsFloat → `llvm.fabs.f64`.
   - Comparações: F → oeq/une/olt/ole/ogt/oge; I → icmp com sinal; B → icmp sem sinal no i1; IntFloat(op) → `r = cudra_cmp_int_float(i, f)`, com `==` ↔ r==0, `!=` ↔ r!=0 (inclui o 2), `<` ↔ r==−1, `<=` ↔ r==−1||r==0, `>` ↔ r==1, `>=` ↔ r==0||r==1; FloatInt(op) → a mesma chamada com os operandos trocados e o operador espelhado (atenção ao nan: com nan só o `!=` é verdadeiro). Em cadeia: cada operando avaliado uma vez, da esquerda para a direita, com curto-circuito (ramos, não `select`, porque os operandos podem ter efeitos: chamadas que imprimem).
   - And/Or com curto-circuito (ramos + phi ou alloca). Not → `xor i1 1`.
   - Cast: int→float `sitofp`; float→int `cudra_float_to_int`; bool→int `zext`; bool→float `uitofp`; int→bool `icmp ne 0`; float→bool `fcmp une 0.0` (nan é verdadeiro).
   - Print: avalia TODOS os argumentos primeiro (em ordem), depois imprime com `cudra_print_sep` entre eles e `cudra_print_end` no fim. NoneTy → `cudra_print_none` (se o argumento é uma chamada a uma função None, a chamada acontece na fase de avaliação).
   - ForRange (regra 14): start, stop, step avaliados uma vez, por esta ordem. step == 0 → panic 4. Número de iterações calculado à entrada SEM overflow, em u64 (por exemplo, step>0 e start<stop: `n = ((stop - start) as u64 - 1) / step as u64 + 1`; o simétrico para step<0; caso contrário 0). Ex.: `range(-2**63, 2**63-1)` tem 2^64−1 iterações e tem de dar o número certo. Contador escondido em u64 (comparação sem sinal); em cada iteração `v = start + k*step` com mul/add WRAPPING (o resultado final cabe sempre em i64, mesmo quando os intermédios não cabem) e SEM verificação de overflow, mesmo em debug. Depois do ciclo, `v` fica com o último valor. `break`/`continue` com uma pilha (continue → incremento do contador).
   - Modos: debug `O0`; `--release` → `run_passes("default<O3>")` do inkwell, com o CPU e as features do host (`TargetMachine::get_host_cpu_name`/`get_host_cpu_features`) e `RelocMode::PIC` (o cc do Ubuntu liga como PIE por omissão).

2) crates/cudra-driver:
   - `build.rs` (regra do §8, OBRIGATÓRIA): corre `$CARGO build -p cudra-runtime --lib --release --target-dir $OUT_DIR/rt --manifest-path <raiz>/Cargo.toml`. Usa SEMPRE `--release` para a staticlib: o modo do programa do utilizador não tem nada a ver com o perfil do cudrac.
     - Antes de chamar o cargo interno, remove do ambiente o `RUSTC_WORKSPACE_WRAPPER` e o `RUSTC_WRAPPER` (quando corre sob `cargo clippy` apontam para o clippy-driver), o `CARGO_ENCODED_RUSTFLAGS`/`RUSTFLAGS` e o `CARGO_TARGET_DIR`. Verifica que `cargo clippy` a partir de um target vazio também funciona.
     - Declara `cargo:rerun-if-changed` para crates/cudra-runtime/src, crates/cudra-runtime/Cargo.toml e o Cargo.lock.
     - Descobre as bibliotecas nativas de que a staticlib precisa, correndo o mesmo build com `--print=native-static-libs` (via `cargo rustc ... -- --print=native-static-libs`), e embute a lista ou usa uma lista fixa verificada (`-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`).
     - `cargo:rustc-env=CUDRA_RUNTIME_LIB_DEFAULT=<caminho absoluto da .a>`. A env `CUDRA_RUNTIME_LIB`, em tempo de execução, sobrepõe-se. PROIBIDO procurar uma `.a` em target/debug ou target/release.
     - Depois disto, os testes de processo do runtime (crates/cudra-runtime/tests/common/mod.rs) podem continuar como estão.
   - `cudrac build <src> [-o out] [--release]`: pipeline completo; objeto num tempdir (crate `tempfile`) e ligação com `cc <obj> <.a> <libs nativas> -o <out>`. Sem `-o`: o nome do ficheiro sem extensão, na diretoria atual. Se o `cc` falhar ou não existir: mensagem clara e exit 2.
   - `cudrac run <src> [--release] [-- args...]`: compila para um tempdir, executa com os args e propaga o exit code do programa (0/1). Se o programa morrer por sinal: mensagem "o programa terminou com o sinal N" e exit 2. O stdout/stderr do programa passam diretos (sem buffer do cudrac).
   - `cudrac build <src> --emit=llvm [--release]`: imprime o IR (em release, o IR já otimizado). Os `--emit=tokens|ast|tir` continuam iguais.
   - O pipeline continua na thread de 256 MiB com o hook de panic da Parte D.
   - Os diagnósticos de check/build/run são idênticos (o mesmo stderr para o mesmo ficheiro inválido).

3) Testes:
   - `tests/run/*.cdr` (≥ 40, a cobrir todas as regras do §1.3; a lista mínima está no §9 ponto 2 do plano), com uma linha opcional `# args: ...`. Harness `crates/cudra-driver/tests/run.rs`: compila cada um em debug E release, corre-o e compara stdout + exit code com `python3 <ficheiro> args` (PYTHONPATH a apontar para um shim `cudra.py` com arg_int(i)=int(sys.argv[i]) e arg_float(i)=float(sys.argv[i]); o shim é gerado num tempdir pelo harness). Se o `python3` não existir, o teste FALHA com uma mensagem clara (não passa em silêncio). Corre os casos em paralelo (threads) para o tempo ficar razoável.
   - `tests/run-expect/*.cdr` + `<nome>.debug.out`/`<nome>.release.out` (stdout esperado, com uma última linha `# exit: N`), para as divergências documentadas: overflow (debug exit 1, release wrap), `1 << 64`, `2 ** -1`, `(-8.0) ** (1/3)`, `range` com contagem ≥ 2^63, `i64::MIN // -1`, `-(-9223372036854775807 - 1)`, `abs(i64::MIN)`. `BLESS=1` regrava.
   - Teste do `build.rs`: `cargo test --workspace` com o CARGO_TARGET_DIR vazio passa à primeira (corre-o de verdade: `rm -rf $CARGO_TARGET_DIR && bash scripts/check.sh`).
   - Teste de colisão de nomes (ver acima) e teste de `cudrac run` com args e com exit 1 a meio do output (o stdout parcial tem de aparecer).
   - Critério §12 (truncação): lexer + parser + sema sobre TODOS os ficheiros de tests/ui, tests/check-ok, tests/run e tests/run-expect truncados em cada posição de byte que seja fronteira de carácter UTF-8 → nenhum panic. Para ser rápido, faz isto em processo: se preciso, expõe o pipeline de check numa lib (`crates/cudra-driver/src/lib.rs`, com o `main.rs` a usá-la) e chama-a diretamente, apanhando panics com `catch_unwind`.

CRITÉRIOS DE ACEITAÇÃO (reporta os números reais de cada um; os oráculos NÃO podem ser alterados para passar; se achares que um caso de um oráculo está errado, reporta-o com o programa mínimo e o output do CPython):
- `rm -rf $CARGO_TARGET_DIR && bash scripts/check.sh` → verde (fmt, clippy -D warnings, todos os testes).
- `python3 tests/oracle/run_attack.py <cudrac>` → "RESUMO ataque run: 0 falhas" (o script compila em debug e release; lê-o para ver como chama o cudrac).
- `python3 tests/oracle/program_oracle.py <cudrac> 1000 <seed> run` com as seeds 1, 2, 3, 4 e 5 → 0 falhas cada.
- Regressões: `program_oracle.py <cudrac> 1500 1 check` e `sema_attack.py <cudrac>` continuam com 0 falhas.
- `cudrac run --release` de um fib recursivo com n=35 lido de arg_int imprime o mesmo que o python3; reporta os dois tempos (`time`).
- Commit final: "fase1: parte E — codegen LLVM, cudrac build/run e testes de execução".

No fim, reporta:
- os ficheiros criados/alterados;
- as decisões tomadas onde o plano era omisso;
- as divergências face ao plano;
- o resumo de cada oráculo;
- os tempos (check.sh a partir do target vazio, fib 35).
