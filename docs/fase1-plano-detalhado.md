# Fase 1 — "Hello, native" · Plano detalhado de implementação

Versão 1.0 · 2026-09-23 · baseado em `docs/plano-arquitetura-mvp.md` (aprovado)

**Objetivo:** `cudrac build prog.cdr -o prog` produz um executável Linux x86-64 nativo, via LLVM. Para todo o programa do subconjunto da Fase 1, o executável imprime **exatamente** o mesmo stdout e devolve o mesmo exit code que `python3 prog.cdr`.

---

## 0. Pré-requisitos e convenções

### 0.1 Ambiente (o utilizador instala; exige admin/sudo)
- WSL2 com `Ubuntu-24.04`.
- Dentro do WSL:
  - `rustup` (stable ≥ 1.85);
  - LLVM 20 via `apt.llvm.org` (`llvm.sh 20 all`), que inclui `llvm-20-dev`, `libpolly-20-dev`, `clang-20` e `lld-20`;
  - `libzstd-dev`, `zlib1g-dev`, `build-essential`, `python3` (≥ 3.12), `hyperfine`, `pypy3` (opcional).
- Variável `LLVM_SYS_201_PREFIX=/usr/lib/llvm-20` no `~/.bashrc`.

**Estado (2026-09-23): instalado e verificado.**
- Distro `Ubuntu-24.04`, utilizador `awto`.
- LLVM 20.1.8 (com os targets NVPTX e AMDGPU), `clang-20`, Rust 1.98.1, Python 3.12.3, hyperfine 1.18.
- O `~/.bashrc` já tem `LLVM_SYS_201_PREFIX`, `CARGO_TARGET_DIR=$HOME/.cache/cudra-target` e `/usr/lib/llvm-20/bin` no PATH.
- Um teste de fumo com `inkwell = { version = "0.10.0", features = ["llvm20-1"] }` gerou um executável nativo e PTX, com sucesso.
- **Usar o inkwell 0.10.0.** Mudança de API relevante face a exemplos antigos: `build_call(...)?.try_as_basic_value()` devolve `ValueKind`; usar `.basic()` (e **não** `.left()`) para obter o `BasicValueEnum`.
- Os scripts de shell passados ao `wsl` a partir do PowerShell perdem as aspas: escrever os comandos num `.sh` e correr `wsl -d Ubuntu-24.04 -- bash /mnt/c/.../script.sh`.

### 0.2 Onde vive o código
- O código fonte fica em `C:\Users\awtop\Documents\cudraAI` (no WSL: `/mnt/c/Users/awtop/Documents/cudraAI`).
- Os artefactos de build ficam **fora** de `/mnt/c`, porque o 9P é lento. Para isso, `.cargo/config.toml` no repositório define `[build] target-dir = "/home/<user>/.cache/cudra-target"`. Alternativa: a variável `CARGO_TARGET_DIR`, exportada no `~/.bashrc` do WSL; preferir esta para não pôr o nome do utilizador no repositório.
- Todos os comandos correm como `wsl -d Ubuntu-24.04 --cd /mnt/c/Users/awtop/Documents/cudraAI -- bash -lc "<cmd>"`.
- `.gitattributes`: `* text=auto eol=lf`. `.gitignore`: `target/`, `*.o`, `/out/`, `__pycache__/`.
- `git init` + commits locais apenas. **Nunca** `git push`, **nunca** mexer no `git config`, **nunca** adicionar a linha `Co-Authored-By` nos commits.

### 0.3 Estilo de código
- Comentários mínimos: só onde o código não se explica sozinho. Sem comentários a justificar correções.
- Sem `unwrap()` em caminhos que dependem do input do utilizador. Os erros passam por `Diagnostic`.
- `cargo fmt` e `cargo clippy -- -D warnings` limpos no fim de cada parte.
- Não criar ficheiros README/LEIA-ME.

---

## 1. Âmbito da linguagem na Fase 1

### 1.1 Suportado
| Categoria | Detalhe |
|---|---|
| Tipos | `int` (i64), `float` (f64), `bool`, `None` (apenas como retorno implícito/`return` sem valor) |
| Literais | inteiros decimais/`0x`/`0o`/`0b` com `_`; floats `1.5`, `1.`, `.5`, `1e10`, `1_0.5e-3`; `True`/`False`/`None` |
| Instruções | atribuição `x = e`, atribuição anotada `x: float = e`, `+= -= *= /= //= %= **= &= |= ^= <<= >>=`, `if/elif/else`, `while`, `for v in range(stop)`, `range(start, stop)`, `range(start, stop, step)`, `break`, `continue`, `pass`, `return`, expressão como instrução |
| Funções | `def f(a: int, b: float) -> float:`. **Anotações obrigatórias nos parâmetros nesta fase.** O retorno é opcional: se faltar, é inferido a partir dos `return` (ou `None`). Recursão direta e mútua. Funções só ao nível de topo. |
| Expressões | aritmética `+ - * / // % **`, bits `& | ^ << >> ~`, unários `- + not`, comparações `== != < <= > >=` incluindo em cadeia, `and`/`or` (só com operandos `bool`), chamadas |
| Builtins | `print(*args)` (0+ argumentos, separador espaço, `\n`), `int(x)`, `float(x)`, `bool(x)`, `abs(x)`, `range(...)` (só dentro de `for`) |
| Imports | `from cudra import arg_int, arg_float` (os únicos nomes válidos). `import cudra` é aceite, mas não é necessário. |
| Topo | instruções soltas formam o `main` implícito; as variáveis do topo são locais do `main` |

### 1.2 Fora de âmbito (erro de compilação claro, com span)
strings (fora do `print`: **não há strings de todo na Fase 1**, `print("x")` dá erro), listas, tuplos, dicts, classes, `lambda`, `global`/`nonlocal`, funções aninhadas, `while ... else`/`for ... else`, `try`, `with`, decoradores, parâmetros por omissão ou nomeados, `*args` no utilizador, `is`, `in` (fora do `for`), expressões condicionais `a if c else b`, walrus `:=`, docstrings.

Mensagem padrão: `erro: <construção> ainda não é suportado(a) na Fase 1`.

### 1.3 Semântica, alinhada com o Python
1. **A primeira atribuição declara** a variável no âmbito da função. Voltar a atribuir tem de dar **exatamente o mesmo tipo**. O erro mostra os dois spans.
   **Não há promoção implícita `int → float` em fronteiras de armazenamento**: atribuição (incluindo `x: float = 1`), passagem de argumentos, `return` e aumentos como `x += e` quando o resultado muda o tipo de `x`. Motivo: o Python não converte o valor. `x: float = 1; print(x)` imprime `1`, e uma promoção faria o nosso executável imprimir `1.0`, o que quebra o objetivo de stdout idêntico. O erro sugere a correção: `tipo incompatível: 'x' é float, mas o valor é int (usa 1.0 ou float(...))`. A promoção **só** existe dentro de expressões mistas (`i + f`, `i < f`), onde o Python também dá `float`. `x = 1; x += 0.5` → erro, porque `x` é `int`. `y = 1.0; y += 1` → válido, porque `float + int` dá `float`.
2. **O âmbito é a função, não o bloco.** Uma variável criada num `if` ou `for` é visível depois. **Atribuição definida:** usar uma variável que não foi atribuída em todos os caminhos até ali dá erro `a variável 'x' pode não estar definida aqui`. A análise é sobre o CFG: interseção dos conjuntos "definidos" nos predecessores. Os ciclos contam como "podem executar 0 vezes".
3. **Uma função não pode ler variáveis do topo.** Erro: `variáveis globais não são suportadas na Fase 1`. Um parâmetro ou local com o mesmo nome faz sombra, e está correto.
4. **Ordem no topo:** chamar uma função no topo antes de a sua linha `def` dá erro de compilação `'f' usada antes de ser definida` (em Python seria NameError em runtime). Dentro de funções, chamar funções definidas mais abaixo é válido.
5. `/` devolve sempre `float`. `//` e `%` entre `int` usam semântica de floor (o sinal do `%` segue o divisor). `i64::MIN % -1` → `0`, sem executar `srem`, que no x86 dá SIGFPE.
   - **`int / int`** tem de ser o arredondamento correto do quociente exato, como no `long_true_divide` do CPython. Se `|a|` e `|b|` são ≤ 2^53, basta `(a as f64) / (b as f64)`. Senão, é preciso um helper do runtime, `cudra_int_truediv(a, b)`, que arredonda o quociente exato (ties-to-even) com aritmética `u128`. Verificado: `9223372036854775807 / 1285` → `7177721429458969.0`, enquanto a divisão ingénua dá `…970.0`.
   - **Entre `float`** (ou misto, promovendo o `int`), `//` e `%` seguem EXATAMENTE o `float_divmod` do CPython. **Não** usar `floor(a/b)`: `1 // 0.1` é `9.0`, não `10.0`. O algoritmo:
     ```
     mod = fmod(a, b); div = (a - mod) / b
     if mod != 0: if (b < 0) != (mod < 0): mod += b; div -= 1
     else: mod = copysign(0.0, b)
     if div != 0: fd = floor(div); if div - fd > 0.5: fd += 1
     else: fd = copysign(0.0, a / b)
     ```
     Valores verificados no CPython: `1 // 0.1 = 9.0`, `1 % 0.1 = 0.09999999999999995`, `-4.0 % 2.0 = 0.0`, `4.0 % -2.0 = -0.0`, `-0.0 // 1.0 = -0.0`, `0.0 // -1.0 = -0.0`, `-7 // 2.0 = -4.0`, `inf // 1 = nan`, `inf % 1 = nan`, `1 % inf = 1.0`, `-1 % inf = inf`, `-1 // inf = -1.0`, `1e308 // 1e-308 = inf`, `-1e-300 // 1e300 = -1.0`.
6. **Divisão/módulo por zero** (int ou float, em `/ // %`): mensagem em stderr `ZeroDivisionError: division by zero` e exit code 1. Não é preciso igualar o texto do CPython, só o exit code e o stdout até àquele ponto. **Atenção:** o stdout tem de ser flushed antes de sair.
7. `**`:
   - `int ** int` com expoente ≥ 0 → `int` (exponenciação binária no runtime);
   - expoente negativo → erro de runtime `ValueError: negative int exponent not supported` + exit 1 (divergência documentada: o Python devolveria um float);
   - qualquer operando `float` → `float`, via o helper do runtime `cudra_fpow(x, y)`, que replica o `float_pow` do CPython 3.12 (casos especiais primeiro, depois o `pow` da libm):
     - `y == 0` → `1.0`, mesmo com `x` nan;
     - `x == 1` → `1.0`, mesmo com `y` nan;
     - nan → nan;
     - `y` ±inf e `x` ±inf seguem as regras do CPython (`(-1.0) ** inf = 1.0`);
     - `x == 0` com `y < 0` → `ZeroDivisionError` + exit 1;
     - `x < 0` com `y` finito e não inteiro → o Python devolve um **complexo**. Para nós é um erro de runtime `ValueError: potência de base negativa com expoente fracionário ainda não é suportada` + exit 1 (divergência documentada, em `run-expect/`);
     - um resultado infinito a partir de `x` e `y` finitos → `OverflowError` + exit 1 (`10.0 ** 400`);
     - `(-0.0) ** 3 = -0.0` (expoente inteiro ímpar mantém o sinal).
   - associa à direita, e `-2**2 == -4`: o `**` liga mais forte do que o unário à esquerda.
8. **Overflow de inteiros** (`+ - *`, `**`, `-x`, `abs`):
   - build debug → `OverflowError: integer overflow` + exit 1;
   - build `--release` → dá a volta (wrapping), silenciosamente;
   - `<<`/`>>` com contagem negativa → `ValueError: negative shift count` + exit 1 nos dois modos;
   - `x << n` conta como aritmética com overflow: em debug, se o resultado não cabe em i64 (`n >= 64 && x != 0`, ou `(x << n) >> n != x`) → `OverflowError`; em release, `n >= 64` → 0, senão `shl` normal. `0 << n` é sempre 0;
   - `x >> n` com `n >= 64` → 0 se `x >= 0`, −1 se `x < 0` (igual ao Python) nos dois modos. Nunca emitir `shl`/`ashr` do LLVM com contagem ≥ 64 (é poison).
9. `int(x: float)`: trunca para zero. NaN → `ValueError`, ±inf ou fora do intervalo i64 → `OverflowError`, ambos com exit 1, **nos dois modos**. `int(bool)`/`int(int)` → identidade. `float(int)` → `sitofp`. `bool(x)` → `x != 0`.
10. **Veracidade:** as condições de `if`/`while`/`not` aceitam `bool`, `int` (≠ 0) e `float` (≠ 0.0). No float, usar `fcmp une x, 0.0`, porque `nan` é verdadeiro. Nas comparações de floats: `==` → `oeq`, `!=` → `une` (`nan != nan` é `True`), `< <= > >=` → `olt ole ogt oge`. `and`/`or` exigem os dois operandos `bool`, senão erro `and/or com operandos não-bool ainda não é suportado`.
11. `bool` em aritmética: **erro** nesta fase (`True + 1`), para simplificar. As comparações entre `bool` (`==`, `!=`) são válidas.
12. **Comparações mistas** `int`/`float`: são EXATAS, como no `float_richcompare` do CPython. **Não** basta promover o `int`: `2**53+1 == 9007199254740992.0` é `False` e `9223372036854775807 < 9.223372036854776e18` é `True`. Se `|i| ≤ 2^53`, pode converter-se o `int` para `float`. Senão, compara-se o `float` (se for finito) com o `int` em `i128`, tendo em conta a parte fracionária. `nan` → só o `!=` é verdadeiro. Implementar num helper do runtime ou inline. Na **aritmética** mista, a promoção `sitofp` está correta (igual ao `PyLong_AsDouble`).
13. **Comparação em cadeia:** `a < b < c` ≡ `a < b and b < c`, com `b` avaliado **uma vez** (guardado num temporário) e curto-circuito.
14. `range`:
    - todos os argumentos `int`; `step == 0` → `ValueError: range() arg 3 must not be zero` + exit 1;
    - o número de iterações é calculado **uma vez à entrada** (`len = max(0, ceil((stop-start)/step))` sem overflow). O ciclo corre sobre um contador escondido e atribui `v = start + k*step` em cada iteração;
    - mudar `v` no corpo não afeta a iteração; depois do ciclo, `v` fica com o último valor atribuído;
    - se o ciclo corre 0 vezes, `v` não fica definida (conta para a atribuição definida).
15. `print` de floats igual ao `repr` do Python:
    - dígitos mais curtos que fazem round-trip (usar o `{:e}` do Rust, que dá os dígitos mínimos, e depois reformatar);
    - notação fixa se o expoente decimal estiver em `-4 <= exp < 16`, senão científica `d.ddde+XX`/`e-XX` (expoente com pelo menos 2 dígitos, sem `.0` se a mantissa tiver um só dígito: `1e+16`);
    - na notação fixa, acrescentar `.0` se não houver parte decimal;
    - casos especiais: `inf`, `-inf`, `nan`, `-0.0`.
    - **Casos de teste obrigatórios:** `0.1`, `1.0`, `-0.0`, `1e16`, `1e15`, `123456789012345678.0`, `0.0001`, `0.00001`, `1/3`, `2.5e-5`, `1e308*10` (→ `inf`), `-(1e308*10)`, `inf - inf` (→ `nan`; `0.0/0.0` não serve, dá ZeroDivisionError). Valores verificados no CPython: `1e+16`, `1000000000000000.0`, `1.2345678901234568e+17`, `0.0001`, `1e-05`, `2.5e-05`, `0.3333333333333333`.
16. `print` de `bool` → `True`/`False`; de `None` → `None`; de `int` → decimal.
17. Funções sem `return` explícito devolvem `None`. Usar esse valor em aritmética dá erro de tipo.
18. `arg_int(i)` / `arg_float(i)`: lêem `argv[i]` (`i` é `int`, `argv[0]` é o programa). Índice em falta → `IndexError` + exit 1; valor inválido → `ValueError` + exit 1.

---

## 2. Estrutura de ficheiros a criar

```
C:\Users\awtop\Documents\cudraAI\
├── Cargo.toml                         # [workspace] resolver = "2", members = ["crates/*"]
├── rust-toolchain.toml                # channel = "stable"
├── .gitattributes  .gitignore
├── crates/
│   ├── cudra-span/src/lib.rs          # FileId, Span, SourceMap, Symbol, Interner
│   ├── cudra-diagnostics/src/lib.rs   # Diagnostic, Label, Level, render (ariadne)
│   ├── cudra-lexer/src/{lib.rs, token.rs, indent.rs}
│   ├── cudra-ast/src/lib.rs           # exatamente a AST da §3
│   ├── cudra-parser/src/{lib.rs, stmt.rs, expr.rs}
│   ├── cudra-tir/src/lib.rs           # TIR da §5
│   ├── cudra-sema/src/{lib.rs, resolve.rs, infer.rs, definite.rs, lower.rs}
│   ├── cudra-runtime/src/lib.rs       # crate-type = ["staticlib"]
│   ├── cudra-codegen-llvm/src/lib.rs  # (+ módulos à vontade)
│   └── cudra-driver/
│       ├── src/main.rs                # binário `cudrac`
│       └── tests/{run.rs, ui.rs}      # harness de testes end-to-end
├── shim/cudra/__init__.py             # arg_int, arg_float para o CPython
├── tests/
│   ├── run/*.cdr                      # oráculo CPython (cabeçalho opcional "# args: 30")
│   ├── run-expect/*.cdr + *.stdout    # casos com divergência documentada (overflow debug, **-1)
│   └── ui/*.cdr + *.stderr            # erros de compilação esperados
├── bench/
│   ├── programs/{fib,primes,collatz,nbody_scalar}/{prog.cdr,prog.py,prog.c,prog.rs}
│   └── run.py
└── scripts/check.sh                   # fmt + clippy + test + ui + run (tudo)
```

Dependências externas permitidas: `logos`, `ariadne`, `inkwell` (`0.10.0`, feature `llvm20-1`), `insta` (dev), `clap` (derive), `tempfile` (dev/driver), `thiserror`. Mais nenhuma sem justificar no resumo.

---

## 3. Tokens e lexer (`cudra-lexer`)

```rust
pub enum TokenKind {
    Int, Float, Name,                        // texto via Span
    True, False, NoneKw,
    Def, Return, If, Elif, Else, While, For, In, Break, Continue, Pass,
    And, Or, Not, Import, From,
    // palavras reservadas do Python que não suportamos → token próprio para dar erro claro
    Reserved,                                // class lambda global nonlocal try except finally raise with as yield async await del assert is
    LParen, RParen, LBracket, RBracket, LBrace, RBrace, Colon, Comma, Semi, Dot, Ellipsis, Arrow, At,
    AtEq, ColonEq,                           // @= e := : maximal munch igual ao CPython, para erros claros no parser
    Plus, Minus, Star, Slash, DoubleSlash, Percent, DoubleStar,
    Amp, Pipe, Caret, Tilde, Shl, Shr,
    Lt, Le, Gt, Ge, EqEq, NotEq, Eq,
    PlusEq, MinusEq, StarEq, SlashEq, DoubleSlashEq, PercentEq, DoubleStarEq,
    AmpEq, PipeEq, CaretEq, ShlEq, ShrEq,
    Str,                                     // reconhecido só para dar erro "strings não suportadas"
    Newline, Indent, Dedent, Eof,
}
pub struct Token { pub kind: TokenKind, pub span: Span }
pub fn lex(file: FileId, src: &str) -> (Vec<Token>, Vec<Diagnostic>);
```

Regras e casos limite (**cada um com teste**):
- `\r\n` e `\r` são equivalentes a `\n`. A primeira linha pode ter BOM UTF-8; ignorar.
- Comentários `#` até ao fim da linha. Linhas vazias ou só com comentário não emitem `Newline`, nem mexem na indentação.
- Dentro de `()`/`[]` com `depth > 0`: sem `Newline`/`Indent`/`Dedent`. Um `)` sem par dá erro, mas o lexer continua.
- Continuação explícita com `\` no fim da linha: suportar (junta as linhas).
- Indentação: tab = erro `tabs na indentação não são suportados, usa espaços`. Dedent para um nível que não existe na pilha → erro `indentação inconsistente`, e recuperar assumindo o nível mais próximo abaixo.
- Um ficheiro que não acaba em `\n` fica equivalente a um que acaba. Ficheiro vazio → só `Eof`.
- `Newline` só no fim de linhas lógicas não vazias. Nunca dois `Newline` seguidos.
- Um `Int` com `_` duplo ou no fim (`1__0`, `10_`) dá erro. Um literal `Int` que não cabe em u64 dá erro na sema, não no lexer.
- Identificadores: ASCII `[A-Za-z_][A-Za-z0-9_]*`. Não-ASCII dá erro `identificadores não-ASCII ainda não são suportados`.
- Snapshot tests com `insta`: listas de `(kind, texto)` para ≥ 15 programas pequenos, incluindo ciclos aninhados com vários dedents seguidos no EOF.
- Adições da revisão adversarial (A2, validadas contra o CPython 3.12):
  - Uma linha só com espaços e `\` começa uma linha lógica. `\` antes do EOF e parênteses por fechar no EOF dão erro.
  - Prefixos de string válidos: `r u b f rb br fr rf`, com qualquer capitalização. Qualquer outro prefixo é `Name` + `Str`.
  - f-strings PEP 701: um só `Str`, com aspas iguais aninhadas, campos multi-linha, comentários e format specs aninhados.
  - `{ } ;` também são tokens. `(`/`[`/`{` têm de fechar com o par certo.
  - Limites do CPython: 100 níveis de indentação e 200 parênteses aninhados.
  - Diagnósticos próprios para dígitos inválidos em literais com base.

## 4. AST e parser (`cudra-ast`, `cudra-parser`)

A AST é **igual à §2 de `docs/plano-arquitetura-mvp.md`**, com duas adições:

```rust
StmtKind::Import     { module: Ident }                             // import cudra
StmtKind::FromImport { module: Ident, names: Vec<Ident> }          // from cudra import arg_int
```

(O `Item::Stmt` também pode ter imports; os imports só são permitidos no topo. Num corpo de função → erro.)

Parser:
- `pub fn parse(tokens: &[Token], interner: &mut Interner) -> (Module, Vec<Diagnostic>)`.
- As instruções usam descida recursiva. As expressões usam Pratt, com esta tabela (da mais fraca para a mais forte):
  `or` < `and` < `not` (prefixo) < comparações (em cadeia, não associativas) < `|` < `^` < `&` < `<< >>` < `+ -` < `* / // %` < unário `- + ~` < `**` (associa à direita; o operando direito é um unário) < chamada/atributo/índice (postfix).
- `x: T = e` com o alvo `Name` dá `Assign` com `ty`. `x: T` sem valor → erro.
- Um alvo de atribuição é `Name` ou `Index`. O parser aceita `Target::Index`, e é a sema que o rejeita na Fase 1, como aos decoradores. Atributos, tuplos, listas e `*x` como alvo → `ainda não é suportado`. Literais, chamadas e operações como alvo → erro de sintaxe (o CPython também rejeita).
- `parse(file, src, tokens, interner)` recebe o texto fonte para ler nomes e literais pelos spans.
- Aceites pelo parser e rejeitados só na sema (o `ast.parse` do CPython também os aceita): `return` fora de função, `break`/`continue` fora de ciclo, imports dentro de funções, parâmetros sem anotação, parâmetros repetidos, decoradores e alvos `Index`.
- Tipos: `Nome`, `None` (como `TypeExprKind::Name("None")`) e `Nome[T, …]`, com vírgula final opcional. Qualquer outra expressão que o CPython aceite como anotação → `anotação de tipo ainda não é suportada`.
- Soft keywords: uma linha lógica que começa com o nome `match` e acaba em `:` é uma instrução `match` → não suportada (salta-se o bloco). `type` seguido de um nome e depois de `=` ou `[` é um alias de tipo → não suportado. Em todos os outros casos, `match`, `case` e `type` são nomes normais.
- Recuperação de construções não suportadas: reporta **um** erro, salta a instrução inteira (o cabeçalho, o bloco indentado e as cláusulas de continuação `elif`/`else`/`except`/`finally` ao mesmo nível) e continua. Nunca pode gerar erros em cascata.
- Se o lexer reportou erros, o driver e o `dump_ast` mostram só os erros do lexer. O parser corre na mesma, para garantir que não rebenta.
- Blocos: `:` `Newline` `Indent` stmt+ `Dedent`. Blocos numa só linha (`if x: y = 1`) → suportar (uma instrução simples depois do `:`).
- Recuperação: se uma instrução falha, reporta, avança até ao próximo `Newline` ou `Dedent` e continua. Nunca entrar em ciclo infinito: garantir avanço de pelo menos um token por iteração.
- `;` separa instruções simples na mesma linha, como no Python (`x = 1; y = 2`, `if c: a = 1; b = 2`, e um `;` final é permitido).
- `Str` (incluindo f-strings), `{`/`}`, `...`, `:=`, o operador binário `@` e `@=` → erro `… ainda não é suportado(a) na Fase 1`, com span. Os decoradores (`@nome`, `@a.b`, `@a.b(args)` antes de `def`) são parseados para `FuncDef.decorators`, porque a Fase 3 precisa deles para `@gpu.kernel`, e é a sema que os rejeita na Fase 1.
- **Contrato:** tudo o que o CPython aceita e nós rejeitamos por estar fora do subconjunto usa uma mensagem com `ainda não é suportad`. O oráculo do parser usa isto para distinguir uma divergência intencional de um bug.
- Profundidade: o lexer já limita os parênteses (200) e a indentação (100). O CPython 3.12 aguenta cerca de 3000 unários e 3000 `a+a+…`. O nosso parser limita a **profundidade da AST de expressões a 1000** (incluindo cadeias associativas à esquerda) → erro `expressão demasiado aninhada (máximo 1000 níveis)`. Isto é uma divergência intencional. Teste: 100 000 `-` seguidos e 100 000 `a+` seguidos terminam sem stack overflow e com 1 só erro.
- O `cudrac` corre o pipeline todo numa thread com uma stack de 256 MiB (`std::thread::Builder::stack_size`), porque as fases recursivas (sema, codegen, `Drop` da AST) têm de aguentar 1000 níveis mesmo no Windows, onde a stack da thread principal é de 1 MiB.
- Factos do CPython que o parser tem de replicar: `(x) = 1`, `((x)) += 1` e `for (a) in b:` são válidos; `if a: pass; pass` e `x = 1;` são válidos; `x = 1;;`, `;` sozinho, `if a: if b: pass`, `from a import b,` e `from a import ()` são inválidos; `def f(a,)`, `f(a,)` e `from a import (b,)` são válidos.
- `NodeId` atribuídos sequencialmente pelo parser.
- `cudrac --emit=ast` imprime a AST em árvore legível. Esse formato é o dos snapshots.

## 5. TIR (`cudra-tir`)

Igual à §2.2 do plano de arquitetura, **sem** `Array`, `Kernel`, `Capture` ou `GpuLaunch` nesta fase (entram na Fase 2/3). Acrescentar:
- `Ty::NoneTy` (em vez de `Unit`, com o mesmo significado).
- `TBinOp` tipado: `IAdd, ISub, IMul, IFloorDiv, IMod, IPow, IAnd, IOr, IXor, IShl, IShr, FAdd, FSub, FMul, FDiv, FFloorDiv, FMod, FPow`.
- `TCmp`: `ICmp(CmpOp)`, `FCmp(CmpOp)`, `BCmp(Eq|Ne)`.
- `TExprKind::Not`, `INeg`, `FNeg`, `IInvert`, `And/Or` com curto-circuito, `Intrinsic` com `Print(Vec<TExpr>)`, `Abs`, `ArgInt`, `ArgFloat`, e casts explícitos.
- `TStmt::ForRange { var, start, stop, step, body }`: o codegen implementa a regra 14 da §1.3.
- `Module { funcs: Vec<Func>, main: FuncId }`.
- `cudrac --emit=tir` imprime a TIR legível.

## 6. Semântica (`cudra-sema`)

`pub fn check(module: &ast::Module, interner: &Interner) -> Result<tir::Module, Vec<Diagnostic>>`

Passos:
1. **`resolve.rs`:** tabela de funções do topo (nomes duplicados → erro). Imports válidos. Builtins (`print`, `int`, `float`, `bool`, `abs`, `range`); fazer sombra a um builtin com uma função → erro. Ordem das chamadas no topo (regra 4).
2. **`infer.rs`:** por função. Assinatura das anotações; locais com o tipo da primeira atribuição, e as seguintes com o tipo exato (regra 1: sem promoção nas fronteiras). Tipo de retorno: anotado (e todos os `return` têm de ter esse tipo exato), ou o tipo comum de todos os `return`. Tipos diferentes, incluindo `int` + `float` e `None` + valor → erro `return inconsistente`. Recursão sem retorno anotado: se a única informação vier da própria chamada recursiva → erro `anota o tipo de retorno de 'f'`.
3. **`definite.rs`:** atribuição definida sobre um CFG simples construído a partir da AST.
4. **`lower.rs`:** gera a TIR. Insere `Cast` nas promoções dentro de expressões mistas. Em `print(a, b, …)`, avalia TODOS os argumentos (para temporários) antes da primeira chamada `cudra_print_*`, como no Python: `print(f(), g())`, com `f` e `g` a imprimirem, mostra primeiro o output de `f` e de `g`, e só depois a linha do `print`. Se um argumento der erro, nada dessa linha é impresso. Desaçucara `+=` e companhia, as comparações em cadeia e a veracidade (condição `int`/`float` → comparação ≠ 0).
5. Recolhe **todos** os erros possíveis antes de parar. Não parar no primeiro.

Mensagens de erro em português, com o span principal e spans secundários quando ajudam. Exemplo:
```
erro: tipo incompatível na atribuição a 'x'
  ┌─ exemplo.cdr:4:5
3 │     x = 1
  │     ─ 'x' declarada como int aqui
4 │     x = 2.5
  │     ^^^^^^^ atribuído float
```

## 7. Runtime (`cudra-runtime`, staticlib)

ABI C, `#[no_mangle] extern "C"`, sem `panic` a atravessar a fronteira (`panic = "abort"` no perfil do crate ou `catch_unwind`):
- `cudra_rt_init(argc: i32, argv: *const *const c_char)`, chamada pelo `main` gerado.
- Output com buffer próprio (1 MiB), com flush no fim e antes de qualquer saída por erro:
  - `cudra_print_int(i64)`, `cudra_print_float(f64)`, `cudra_print_bool(bool)`, `cudra_print_none()`;
  - `cudra_print_sep()` (espaço), `cudra_print_end()` (`\n`).
- `cudra_ipow(base: i64, exp: i64, checked: bool) -> i64`.
- `cudra_arg_int(i: i64) -> i64`, `cudra_arg_float(i: i64) -> f64`.
- `cudra_panic(kind: u32, ...) -> !`: escreve `<Kind>Error: <msg>` em stderr, faz flush do stdout e `exit(1)`. Tipos: `ZeroDivision`, `Overflow`, `Value`, `Index`.
- `cudra_rt_exit() -> i32`: flush do stdout e devolve 0.
- `cudra_float_repr(f64, buf) -> len`, a implementar a regra 15 da §1.3, com testes unitários próprios no crate (todos os casos obrigatórios).
- Helpers numéricos: o codegen chama-os em vez de gerar ele próprio a lógica, e cada um tem um oráculo contra o CPython com valores aleatórios e de fronteira.
  - `cudra_int_truediv(a, b) -> f64` (regra 5);
  - `cudra_float_floordiv(a, b) -> f64` e `cudra_float_mod(a, b) -> f64` (o `float_divmod` da regra 5; `b == 0` → `ZeroDivisionError`);
  - `cudra_fpow(x, y) -> f64` (regra 7);
  - `cudra_cmp_int_float(i: i64, f: f64) -> i32`: −1/0/1, ou 2 se não ordenado (nan) (regra 12).

## 8. Codegen (`cudra-codegen-llvm`) e driver (`cudra-driver`)

Codegen:
- Um módulo LLVM por programa, com triple do host.
- Cada `Func` da TIR passa a uma função LLVM `internal` (nome `cdr_<nome>`). O `main` gera `i32 main(i32, ptr)`: chama `cudra_rt_init`, depois o corpo do topo, depois `cudra_rt_exit`.
- Locais: `alloca` no bloco de entrada, como descrito no plano de arquitetura. **Não** construir SSA à mão.
- Tipos: `int` → `i64`, `float` → `double`, `bool` → `i1` (`i8` na fronteira com o runtime, se preciso).
- Aritmética `int`:
  - em debug usar `llvm.{sadd,ssub,smul}.with.overflow.i64` e, em overflow, `cudra_panic(Overflow)`; em release, `add/sub/mul` normais (**sem** `nsw`, para o wrapping ser definido);
  - `//` e `%` com floor: `q = sdiv a b; r = srem a b; if (r != 0) && ((r ^ b) < 0) { q -= 1; r += b }`;
  - zero no divisor → `cudra_panic(ZeroDivision)`;
  - `i64::MIN // -1`: overflow em debug; em release o resultado é `i64::MIN`, **sem executar `sdiv`**, que no x86 dá SIGFPE.
- Shifts conforme a regra 8: ramos/`select` antes do `shl`/`ashr`, para a contagem que chega à instrução estar sempre em `0..=63`.
- `float`: `fadd`/`fsub`/`fmul`/`fdiv` normais (sem fast-math). `//` e `%` como na regra 5. `llvm.pow.f64`, `llvm.fabs.f64`, `llvm.floor.f64`, e `fmod` via a instrução `frem`.
- `ForRange` conforme a regra 14. `break`/`continue` com uma pilha de `(bloco_continue, bloco_break)`.
- Modos:
  - debug: `O0`, verificações ligadas;
  - `--release`: pipeline `default<O3>` via `run_passes` do inkwell, `target-cpu` do host.
- Emite um objeto `.o` num tempdir e liga com `cc <obj> <libcudra_runtime.a> -lm -o <out>`. O driver descobre a `.a` pelo `OUT_DIR`/caminho do build; aceitar também a env `CUDRA_RUNTIME_LIB`.
- Verificar o módulo com `module.verify()` antes de emitir. Se falhar, é um bug interno: mensagem `erro interno do compilador` + o IR em stderr.

Driver `cudrac` (clap):
- `cudrac build <src> [-o out] [--release]`.
- `cudrac run <src> [--release] [-- args...]`.
- `cudrac check <src>`.
- `cudrac build <src> --emit=tokens|ast|tir|llvm`: imprime no stdout e não liga.
- Exit codes: 0 = ok, 1 = erro de compilação (diagnósticos em stderr), 2 = erro de uso ou interno.

## 9. Testes

1. **Unitários/snapshot:** lexer (≥ 15 casos), parser (≥ 20 casos, incluindo precedências `-2**2`, `a < b < c`, `not a == b`, blocos numa só linha e recuperação de erros), `float_repr` (todos os casos da regra 15 + 1000 floats aleatórios com seed fixa, comparando com um ficheiro de referência gerado pelo `repr()` do Python 3.12+ e guardado em `crates/cudra-runtime/tests/float_repr_ref.txt`).
2. **`tests/run/` (oráculo):** o harness (`crates/cudra-driver/tests/run.rs`) compila cada `.cdr` em **debug e em release**, executa-o com os `# args:` e compara stdout + exit code com `python3 <ficheiro>` (com `PYTHONPATH=shim`). Mínimo de **40 programas**, a cobrir todas as regras da §1.3. Incluir, por exemplo:
   - `fib`, `primes` (crivo com `while`), `collatz`, `gcd`, `pow`;
   - aritmética com negativos em `//` e `%` (int e float, com todas as combinações de sinal);
   - promoção int/float;
   - comparações em cadeia com efeitos visíveis (a função chamada imprime);
   - `range` com passo negativo, vazio, passo que não divide o intervalo, e alteração da variável do ciclo no corpo;
   - variável do ciclo depois do ciclo;
   - `break`/`continue` em ciclos aninhados;
   - divisão por zero a meio do output (o stdout parcial tem de coincidir);
   - `print` de vários tipos e sem argumentos;
   - recursão mútua;
   - `int()`/`float()`/`bool()`/`abs()`;
   - `>>` com contagem ≥ 64 em positivos e negativos, e `0 << 100` (`1 << 64` vai para `run-expect/`, porque diverge do Python);
   - blocos numa só linha.
3. **`tests/run-expect/`:** casos com divergência documentada face ao Python, com o stdout e o exit code esperados em ficheiro. Por exemplo: overflow em debug → exit 1; o mesmo em release → valor com wrapping; `1 << 64` (debug → exit 1, release → 0); `2 ** -1` → exit 1.
4. **`tests/ui/`:** ≥ 25 programas inválidos com o stderr esperado. Cobrir cada erro da §1.2 e da §6 (tipo incompatível, pode não estar definida, global em função, uso antes de `def`, retorno inconsistente, tab, indentação inconsistente, string, `class`, parâmetro sem anotação, recursão sem anotar o retorno, etc.). Atualizar com `BLESS=1 cargo test`.
5. `scripts/check.sh`: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace`. Tem de passar a verde no fim.

## 10. Benchmarks (`bench/`)

- 4 programas: `fib` (recursivo, n=35 via `arg_int`), `primes` (crivo com contagem até N, sem listas: contar primos com divisão por tentativa), `collatz` (maior sequência até N), `nbody_scalar` (2 corpos com floats, K passos: sem arrays, só escalares).
- Cada um em `prog.cdr`, `prog.py` (idêntico ao `.cdr`; pode ser o mesmo ficheiro via symlink ou cópia), `prog.c` (`clang -O2`) e `prog.rs` (`rustc -O`). **Todos os valores de entrada vêm de argv e o resultado é impresso**, para não haver constant folding.
- `bench/run.py`: compila tudo, confirma que **todos os outputs coincidem**, corre `hyperfine --warmup 2 --runs 10 --export-json` e gera uma tabela markdown (`bench/results/fase1.md`) com a razão face ao C e ao CPython.
- Meta de aceitação: `cudra --release` a ±15% do C em `fib`/`primes`/`collatz`, e ≥ 30× mais rápido que o CPython. Se falhar, o resultado é **reportado tal como está**, não se afinam os números.

## 11. Divisão em partes para o `implementer`

| Parte | Conteúdo | Depende de | Validação no fim da parte |
|---|---|---|---|
| **A** | workspace, `.gitattributes`/`.gitignore`, `git init`, `cudra-span`, `cudra-diagnostics`, `cudra-lexer` + testes, `cudra-ast` | — | `cargo test -p cudra-lexer` a verde, snapshots revistos |
| **B** | `cudra-parser` + testes, `cudrac --emit=tokens/ast` (driver mínimo) | A | `cargo test -p cudra-parser`; `cudrac build x.cdr --emit=ast` funciona |
| **C** | `cudra-runtime` completo + testes do `float_repr` com a referência Python | A (só o workspace) | `cargo test -p cudra-runtime` |
| **D** | `cudra-tir`, `cudra-sema` + `tests/ui/`, `--emit=tir` | B | UI tests a verde |
| **E** | `cudra-codegen-llvm`, driver completo, `tests/run/`, `tests/run-expect/`, `scripts/check.sh` | C, D | `scripts/check.sh` a verde |
| **F** | `bench/` + resultados | E | `bench/results/fase1.md` gerado |

**B e C correm em paralelo** depois de A. O resto é sequencial. Cada parte acaba com um commit local (`fase1: parte X — …`).

## 12. Critérios de aceitação da Fase 1

- [ ] `scripts/check.sh` a verde no WSL.
- [ ] ≥ 40 testes de oráculo passam em debug **e** release; ≥ 25 testes UI; snapshots do lexer e do parser.
- [ ] `cudrac run bench/programs/fib/prog.cdr --release -- 35` imprime o mesmo que o Python.
- [ ] `--emit=tokens|ast|tir|llvm` funcionam (são material de demo).
- [ ] Tabela de benchmarks gerada, com números reais.
- [ ] Nenhum `unwrap()` que o input do utilizador consiga fazer rebentar. Um fuzz simples: correr o lexer + parser + sema sobre todos os ficheiros de teste truncados em cada posição de byte → nenhum panic.
