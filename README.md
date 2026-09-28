# cudraAI

> Uma linguagem com a **sintaxe de Python** e a **velocidade do C**, compilada para código nativo
> via LLVM — a caminho de correr na GPU sem escrever uma linha de CUDA.

`cudra` é uma linguagem de programação estaticamente tipada cuja sintaxe é um **subconjunto
estrito da gramática do Python 3**. O compilador chama-se `cudrac` e o código-fonte tem extensão
`.cdr`. Um programa `.cdr` válido é, por construção, também um programa Python válido.

A ideia é simples de dizer e difícil de fazer: escrever `fib` em Python, correr a 100× a velocidade
do CPython, e mais tarde escrever **a mesma coisa** na GPU — mudando uma palavra, não a linguagem.

```python
# fib.cdr
def fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

print(fib(30))
```

```console
$ cudrac run fib.cdr          # compila para LLVM IR, liga com o runtime, executa
$ cudrac check fib.cdr        # só verifica tipos, sem gerar código
$ cudrac build fib.cdr --release -o fib
$ cudrac build fib.cdr --emit=llvm   # dumps: tokens | ast | tir | llvm
```

---

## O que há de diferente: o oráculo CPython

A escolha mais importante do projecto não é técnica, é de **testes**. Como `cudra` usa a sintaxe do
Python, qualquer programa escrito na linguagem é um programa Python. Isso dá um oráculo de
correcção **gratuito e exacto**:

> Compilar o mesmo ficheiro com o `cudrac` e correr com o CPython. Se o stdout divergir, o
> compilador está errado.

A consequence é que o compilador tem de reproduzir a semântica numérica do Python à risca — e não
pode fingir que `//` é `sdiv` do C (que trunca) nem que `%` é `srem` (que segue o sinal do dividendo).
`/` dá sempre `float`; `//` faz *floor*; `%` fica com o sinal do divisor. `range()` guarda o valor
inicial num contador escondido, por isso mexer na variável do ciclo não altera a iteração. Os
inteiros são `i64`, não bignums — divergência assumida e documentada.

Isto vale tanto para quem escreve código em `cudra` como para quem escreve o compilador: transforma
"o resultado parece certo" em "o resultado é igual ao do CPython, em milhares de casos aleatórios".

## Como o Python fica estático

A sintaxe é Python; o sistema de tipos não é. Regras que tornam o programa verificável antes de
correr:

1. **A primeira atribuição declara a variável** e fixa o tipo. Reatribuir com um tipo incompatível
   é erro, com os dois spans apontados ("declarada como `int` aqui… atribuída `float` aqui").
2. **O âmbito é a função**, como em Python: uma variável criada dentro de um `for` continua visível
   depois dele. Uma análise de *atribuição definida* rejeita usos que podem acontecer sem valor
   (`x` só atribuído dentro de um `if`).
3. **Promoção numérica**: `int op float → float`, com o `Cast` explícito na IR.
4. **O que ainda não é suportado é erro explícito**, nunca comportamento indefinido. O contrato de
   mensagens é auditado: o que o Python não faz hoje, o `cudra` diz que ainda não faz — com a mesma
   frase em todos os 157 casos de teste.

## Estado actual

**Fase 1 (CPU) — completa.** O `cudrac` tem o pipeline inteiro a funcionar:

```
.cdr → lexer (logos + INDENT/DEDENT) → parser (descida recursiva + Pratt)
     → sema (resolução de nomes, inferência, análise de atribuição definida)
     → TIR (tipos concretos, verificada estruturalmente)
     → LLVM IR (inkwell) → .o → executável
```

* `cudrac build` / `run` / `check`, com `--release` (`default<O3>`, overflow definido) e modo debug
  (overflow de `i64` dá pânico com mensagem, como no Python).
* `--emit=tokens|ast|tir|llvm` para inspeccionar cada fase.
* Tipos actuais: `int` (i64), `float` (f64), `bool`, `None`, e as conversões `float()` / `int()` /
  `bool()`.
* Runtime em Rust (staticlib, só `std`) com `print` fiel ao Python, parsing de `argv`, `repr` de
  floats, pânicos com mensagens e códigos de saída iguais aos do CPython.

**Ainda não feito** ( roadmap abaixo): GPU, `i32`/`f32`, `Array[T]`, funções sem anotação de tipo,
`math.*`, strings, importação de módulos.

| | |
|---|---|
| Crates | 10 |
| Linhas de Rust | ~19 700 |
| Casos de teste `.cdr` | 254 (157 `ui`, 48 `run`, 13 `run-expect`, 36 `check-ok`) |
| Oráculos / fuzzers em Python | 12 |
| Commits | 35 |

## Arquitectura

Um crate por fase do compilador. As fronteiras entre crates obrigam as dependências a andarem num
só sentido, e o codegen nunca vê a AST.

| Crate | Linhas | Responsabilidade |
|---|---:|---|
| `cudra-span` | 536 | `Span`, `FileId`, `SourceMap`, interner de strings (`Symbol`) |
| `cudra-diagnostics` | 610 | `Diagnostic` com labels e spans secundários, render com `ariadne` |
| `cudra-lexer` | 3 292 | Texto → tokens, com pilha de indentação e `INDENT`/`DEDENT` |
| `cudra-ast` | 199 | Só as estruturas de dados da AST — sem lógica |
| `cudra-parser` | 2 837 | Tokens → AST, com recuperação de erros (reporta vários de uma vez) |
| `cudra-sema` | 3 373 | Nomes, tipos, atribuição definida, TIR |
| `cudra-tir` | 1 311 | IR tipada + `verify` estrutural antes do codegen |
| `cudra-codegen-llvm` | 1 678 | TIR → LLVM IR → objeto, via `inkwell` |
| `cudra-runtime` | 3 847 | `staticlib` ligada a cada executável: `print`, `argv`, pânicos, aritmética |
| `cudra-driver` | 2 087 | O binário `cudrac` e a ligação com `cc` |

**Regra de ouro:** a AST tem açúcar sintático e nenhum tipo; a TIR tem tudo resolvido — tipos
concretos, `range()` já transformado num ciclo contado, conversões explícitas, chamadas apontadas
para instâncias. Por isso cada backend (CPU hoje, NVPTX amanhã, AMDGPU no dia a seguir) é um
tradutor simples de TIR.

## Compilar a partir do código

Requisitos: **Rust stable** (edition 2021, ≥ 1.85), **LLVM 20** e um compilador C (`cc`/`clang`)
para ligar o executável. O LLVM é encontrado por `LLVM_SYS_201_PREFIX`; o valor por omissão está em
[`.cargo/config.toml`](.cargo/config.toml) e aponta para `/usr/lib/llvm-20` (ajustar se o seu LLVM
estiver noutro sítio).

```bash
# Ubuntu/Debian: sudo apt install llvm-20-dev clang
rustup component add rustfmt clippy

cargo build --release          # o binário fica em target/release/cudrac
bash scripts/check.sh          # fmt + clippy -D warnings + testes
```

Os testes de execução precisam de um `cc` no `PATH`; os de oráculo precisam de Python 3.

## Testes

Quatro camadas, e a última é a que interessa:

| Directório | O que é | Como corre |
|---|---|---|
| `tests/ui/` | 157 programas inválidos + o `.stderr` exacto do erro | `cargo test` (snapshots `insta`) |
| `tests/check-ok/` | 36 programas válidos que só devem passar o `check` | `cargo test` |
| `tests/run/` | 48 programas válidos: o stdout do `cudrac` tem de ser **igual ao do CPython** | `cargo test` |
| `tests/run-expect/` | 13 casos-limite com stdout gravado para debug **e** release | `cargo test` |
| `tests/oracle/` | 12 oráculos e fuzzers em Python | `bash scripts/oracle.sh` |

Os oráculos não são só "o CPytorn diz o mesmo":

* `program_oracle.py` gera programas aleatórios válidos e compara as saídas.
* `sema_attack.py` / `sema_attack2.py` atacam o frontend com centenas de construções, classificando
  cada resposta como OK / erro estático / "ainda não suportado" / erro do Python.
* `ui_subset_check.py` verifica o contrato das mensagens: se o CPython aceita um programa, o erro
  que o `cudra` dá tem de ser uma limitação declarada, nunca um NAMEERROR disfarçado.
* `trunc_fuzz.py` corta ficheiros em todas as posições UTF-8 e exige que o compilador não entre em
  pânico nem se interesse.
* `parser_oracle.py` confronta o parser com o `ast` do CPython em quatro modos (valid, wild, fuzz,
  deep).

## Roadmap

| Fase | Âmbito | Objectivo |
|---|---|---|
| **1 — "Hello, native"** | ✅ pipeline completo no CPU, parâmetros anotados | Paridade com o C, 30–100× mais rápido que o CPython |
| **2 — "Python sem anotações"** | monomorfização, `i32`/`f32`, `Array[T]`, `math.*`, shim Python | Programas reais no CPU, perto do Numba mas sem decorador |
| **3 — "Primeira GPU"** | `gpu.range`, extração de kernels, codegen NVPTX → PTX embebido, CUDA Driver API, fallback no CPU | O mesmo código, uma palavra diferente, 50–200× |
| **4 — "Parallel everywhere"** | `parallel(n)` com decisão CPU/GPU em runtime, reduções, libdevice, biblioteca de ML em `cudra` | Um MLP escrito em `cudra` a correr na GPU |

Duas regras que valem para todas as fases, e que estão escritas no plano porque custam caro ignorá-las:

* **Os benchmarks recebem entradas por `argv`.** Senão o LLVM avalia o resultado em tempo de
  compilação (`sum(range(n))` vira uma fórmula fechada) e announces "∞× mais rápido" para um
  programa que nem corre.
* **Não se promete mais do que o LLVM dá.** A promessa honesta é paridade com C/Rust — é o mesmo
  motor. Dizer mais é perder a credibilidade com qualquer pessoa que leia o IR.

## Estrutura

```
cudraAI/
├── crates/            # os 10 crates do compilador
├── tests/             # ui · check-ok · run · run-expect · oracle
├── scripts/           # check.sh (fmt+clippy+testes) · oracle.sh · parser_oracle.sh
└── docs/              # plano de arquitectura e briefings de implementação
```

## Documentação

* [`docs/plano-arquitetura-mvp.md`](docs/plano-arquitetura-mvp.md) — decisões de base, arquitectura,
  especificação da AST e da TIR, marcos, riscos.
* [`docs/fase1-plano-detalhado.md`](docs/fase1-plano-detalhado.md) — plano da Fase 1, ficheiro a
  ficheiro, com critérios de aceitação.
* `handdown *.md` — registos de passagem de trabalho entre rondas: o que ficou feito, o que foi
  medido e o que está pendente.

## Estado do projecto

Em desenvolvimento activo. O pipeline do CPU está completo e verificado contra o CPython; a parte
de GPU ainda não começou. Números de desempenho citados aqui são de máquina ociosa em WSL2 — o
`check` de 100 000 linhas de código corre em ~0.1 s e o `ciclo_100k` em ~0.65 s.
