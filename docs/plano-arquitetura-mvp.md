# cudraAI — Plano de Arquitetura do MVP

Versão 0.1 · 2026-09-23 · estado: proposta para aprovação

Nome provisório da linguagem: **cudra** · extensão provisória: **`.cdr`** · compilador: **`cudrac`**

---

## 0. Decisões de base (e porquê)

| Decisão | Escolha | Porquê |
|---|---|---|
| Linguagem do compilador | **Rust** | Enums com dados e `match` são o ideal para ASTs e passes de compilação. Sem problemas de memória. O ecossistema tem tudo o que é preciso: `inkwell` (LLVM 11–22), `logos` (lexer), `ariadne` (erros bonitos) e `insta` (snapshot tests). C++ só compensaria se fôssemos mexer por dentro do LLVM/MLIR, e o MVP não precisa disso. |
| Backend CPU | **LLVM via `inkwell`** | É o mesmo otimizador do clang e do rustc. "Velocidade de C" é literalmente o mesmo motor. |
| Backend GPU | **Backend NVPTX do próprio LLVM → PTX** | É o caminho do Triton e do CUDA.jl (Julia). Usa a mesma versão de LLVM do CPU e não precisa de `nvcc`. O libNVVM da NVIDIA fica como otimização opcional para depois. **Rejeitado:** gerar código `.cu` e chamar `nvcc`. Obriga o utilizador a ter o CUDA Toolkit, é lento e perdemos o controlo do código gerado. |
| Arranque de kernels | **CUDA Driver API** (`cuModuleLoadData` + `cuLaunchKernel`), com `libcuda`/`nvcuda.dll` carregada dinamicamente | O PTX vai embebido no executável. Se a máquina não tiver GPU NVIDIA, o mesmo binário corre o caminho de CPU em vez de rebentar. |
| Ambiente de desenvolvimento | **WSL2 Ubuntu 24.04** | No Linux, o LLVM de desenvolvimento instala-se com `apt` (apt.llvm.org). No Windows nativo, o `llvm-sys` obriga a compilar o LLVM de raiz. As GPUs na cloud também correm Linux. |
| Hardware GPU | **GPU NVIDIA na cloud** (RunPod / Lambda / Vast.ai, à hora) a partir da Fase 3 | Este PC tem Ryzen AI 7 350 + Radeon 860M, sem GPU NVIDIA. Dá para **gerar e validar PTX localmente** (com `ptxas`, sem GPU), mas **executar** só numa máquina NVIDIA. |
| Sintaxe | **Subconjunto estrito da gramática Python 3** | Todo o programa `.cdr` válido é também Python válido. Isto dá um **oráculo de testes gratuito**: correr o mesmo ficheiro no CPython e no `cudrac` e comparar o output. Também dá um argumento de pitch: "o teu código continua a correr em Python, só que 100× mais lento". |

---

## 1. Arquitetura de pastas e módulos

Workspace Cargo com um crate por fase do compilador. As fronteiras entre crates obrigam a que as dependências só andem num sentido. Também tornam a delegação mais fácil: cada crate pode ser implementado e testado em separado.

```
cudraAI/
├── Cargo.toml                    # [workspace]
├── rust-toolchain.toml           # fixa a versão do Rust
├── crates/
│   ├── cudra-span/               # Span, FileId, SourceMap, interner de strings (Symbol)
│   ├── cudra-diagnostics/        # Diagnostic { level, msg, labels[] } → render com ariadne
│   ├── cudra-lexer/              # texto → Vec<Token>, com NEWLINE / INDENT / DEDENT
│   ├── cudra-ast/                # só estruturas de dados da AST (sem lógica)
│   ├── cudra-parser/             # Tokens → AST (descida recursiva + Pratt para expressões)
│   ├── cudra-sema/               # resolução de nomes, inferência de tipos, monomorfização,
│   │   └── src/                  #   análise de atribuição definida, validação de kernels GPU
│   │       ├── resolve.rs
│   │       ├── infer.rs          #   unificação (union-find de variáveis de tipo)
│   │       ├── mono.rs           #   instâncias de funções por assinatura concreta
│   │       ├── gpu_outline.rs    #   extrai corpos de `gpu.range` para kernels
│   │       └── lower.rs          #   AST + tipos → TIR
│   ├── cudra-tir/                # "Typed IR": tipos concretos, sem açúcar sintático
│   ├── cudra-codegen-llvm/       # TIR (host)    → LLVM IR x86-64 → objeto .o
│   ├── cudra-codegen-nvptx/      # TIR (kernels) → LLVM IR nvptx64 → texto PTX
│   ├── cudra-runtime/            # staticlib em Rust ligada a cada executável:
│   │                             #   print, alocação de Array, bounds-check, timer,
│   │                             #   wrapper da CUDA Driver API, coerência host/device
│   └── cudra-driver/             # binário `cudrac`: build | run | check | --emit=tokens|ast|tir|llvm|ptx
├── tests/
│   ├── lexer/  parser/           # snapshots (insta)
│   ├── ui/                       # programas inválidos + erro esperado (snapshot)
│   ├── run/                      # programas válidos: stdout do cudrac == stdout do CPython
│   └── gpu/                      # só correm com a feature "gpu" em máquina NVIDIA
├── shim/cudra/                   # módulo Python (gpu.range = range, tipos f32/i32 = aliases)
│                                 #   para os .cdr correrem também no CPython
├── bench/
│   ├── programs/<nome>/{prog.cdr, prog.py, prog.c, prog.rs}
│   └── run.py                    # corre tudo com hyperfine e gera tabela + gráfico
└── docs/
```

**Regra de ouro:** o codegen nunca vê a AST. A AST tem açúcar sintático e nenhum tipo. A **TIR** tem tudo resolvido: tipos concretos, `range()` já transformado num ciclo contado, conversões implícitas explícitas, chamadas apontadas para instâncias monomorfizadas e kernels GPU já extraídos. Assim, cada backend (CPU, NVPTX e, no futuro, AMDGPU) é um tradutor simples de TIR.

### 1.1 Lexer (`cudra-lexer`)

Faz tokens com `logos`, mais uma camada de indentação escrita à mão:

- Mantém uma pilha de indentação `[0]`. No início de cada linha lógica, mede a coluna:
  - maior que o topo → `push` + emite `INDENT`;
  - menor → faz `pop` enquanto for maior e emite um `DEDENT` por cada `pop`. Se não bater certo com nenhum nível da pilha → erro *"indentação inconsistente"*;
  - igual → nada.
- Linhas em branco e linhas só com comentário **não** contam para a indentação.
- Com parênteses ou colchetes abertos (`depth > 0`), as quebras de linha são ignoradas (continuação implícita, como em Python).
- Misturar tabs e espaços na mesma indentação é **erro**, não aviso.
- No fim do ficheiro emite `NEWLINE` (se faltar) e `DEDENT` até a pilha voltar a `[0]`, depois `EOF`.
- Literais: `1_000_000`, `0x`, `0b`, floats `1.5e-3`. O lexer guarda o texto; o valor é validado na sema.

### 1.2 Parser (`cudra-parser`)

- As instruções usam descida recursiva. As expressões usam **Pratt parsing** com a tabela de precedências do Python (`or` < `and` < `not` < comparações < `|` < `^` < `&` < shifts < `+ -` < `* / // %` < unários < `**`, que associa à direita).
- Comparações em cadeia `a < b < c` ficam representadas na AST e são "desaçucaradas" na sema.
- Recuperação de erros: quando há um erro, o parser salta até ao próximo `NEWLINE`/`DEDENT` e continua. Assim reporta vários erros de uma vez.
- Cada nó recebe um `NodeId` e um `Span`. Os tipos e símbolos resolvidos ficam em tabelas laterais indexadas por `NodeId`, não dentro da AST.

### 1.3 Semântica e inferência de tipos (`cudra-sema`)

Tipos do MVP: `int` (= `i64`), `float` (= `f64`), `i32`, `f32`, `bool`, `None`, `Array[T]`.

Regras que tornam o Python estático:

1. **A primeira atribuição declara a variável** e fixa o tipo. Voltar a atribuir com um tipo incompatível dá erro com os dois spans ("declarada como int aqui… atribuída float aqui").
2. **O âmbito é a função, como em Python.** Uma variável criada dentro de um `for` continua visível depois dele. Uma **análise de atribuição definida** rejeita usos que podem acontecer sem valor (`x` só atribuído dentro de um `if`).
3. **Inferência por unificação:** cada variável e expressão recebe uma variável de tipo, e as restrições resolvem-se com union-find. Promoção numérica `int op float → float`, com `Cast` explícito na TIR.
4. **Parâmetros sem anotação = função genérica, monomorfizada por chamada.** `def sq(x): return x*x` chamada com `int` e com `f32` gera duas instâncias nativas. Cache por `(função, tipos dos argumentos)`. A recursão funciona com uma variável de tipo para o retorno, unificada com o caso base. É esta a funcionalidade de "zero anotações e velocidade de C".
5. **Semântica numérica igual à do Python**, senão o oráculo CPython falha:
   - `/` dá sempre float; `//` é divisão com floor (≠ `sdiv` do C, que trunca); `%` fica com o sinal do divisor (≠ `srem` do C).
   - `range(a, b, step)` guarda o valor inicial num contador escondido. Mudar `i` dentro do corpo não altera a iteração, e `i` fica com o último valor depois do ciclo.
   - **Divergência assumida:** os inteiros são `i64`, não bignums. Em `--debug` um overflow para o programa com uma mensagem; em `--release` dá a volta (como no Rust). Fica documentado.

### 1.4 Codegen (`cudra-codegen-llvm`, `cudra-codegen-nvptx`)

- **CPU:** cada variável local é um `alloca` no bloco de entrada, e o passe `mem2reg`/`SROA` do LLVM transforma tudo em registos. Por isso não precisamos de construir SSA nós próprios. Pipeline `default<O3>` com `target-cpu=native` (ou um genérico, em modo distribuição). Emite `.o` e liga com `cc`/`clang` + `libcudra_runtime.a`.
- **GPU:** módulo LLVM à parte com triple `nvptx64-nvidia-cuda`. Kernels com a calling convention `ptx_kernel`, índices via intrínsecos `llvm.nvvm.read.ptx.sreg.{tid,ctaid,ntid}.x`, e emissão de texto PTX com o `TargetMachine` NVPTX (`sm_80` por omissão, configurável). O PTX fica embebido como constante global no módulo do host. Funções matemáticas: intrínsecos LLVM (`llvm.sqrt`, `llvm.fma`) no MVP; transcendentes (`exp`, `sin`) ligando `libdevice.10.bc` na Fase 4.

---

## 2. Especificação da AST do MVP (Rust)

```rust
// crates/cudra-ast/src/lib.rs
use cudra_span::{Span, Symbol};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug)]
pub struct Ident { pub name: Symbol, pub span: Span }

#[derive(Debug)]
pub struct Module { pub items: Vec<Item> }

#[derive(Debug)]
pub enum Item {
    Func(FuncDef),
    Stmt(Stmt),                         // instruções de topo → main() implícito
}

#[derive(Debug)]
pub struct FuncDef {
    pub id: NodeId,
    pub name: Ident,
    pub decorators: Vec<Expr>,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,          // `-> int` opcional
    pub body: Block,
    pub span: Span,
}

#[derive(Debug)]
pub struct Param { pub id: NodeId, pub name: Ident, pub ty: Option<TypeExpr>, pub span: Span }

#[derive(Debug)]
pub struct Block { pub stmts: Vec<Stmt>, pub span: Span }

#[derive(Debug)]
pub struct Stmt { pub id: NodeId, pub kind: StmtKind, pub span: Span }

#[derive(Debug)]
pub enum StmtKind {
    Assign    { target: Target, ty: Option<TypeExpr>, value: Expr },   // x = 5 · x: f32 = 5
    AugAssign { target: Target, op: BinOp, value: Expr },              // x += 1
    For       { var: Ident, iter: Expr, body: Block },                 // iter = range(...) | gpu.range(...)
    While     { cond: Expr, body: Block },
    If        { cond: Expr, then: Block, elifs: Vec<(Expr, Block)>, orelse: Option<Block> },
    Return(Option<Expr>),
    Break,
    Continue,
    Pass,
    Expr(Expr),                                                        // print(x), f(y)
}

#[derive(Debug)]
pub enum Target {
    Name(Ident),
    Index { base: Box<Expr>, index: Box<Expr> },                       // y[i] = ...
}

#[derive(Debug)]
pub struct Expr { pub id: NodeId, pub kind: ExprKind, pub span: Span }

#[derive(Debug)]
pub enum ExprKind {
    Int(u64),                           // o sinal vem de Unary::Neg; o intervalo é validado na sema
    Float(f64),
    Bool(bool),
    NoneLit,
    Name(Ident),
    Unary   { op: UnOp, operand: Box<Expr> },
    Binary  { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Logical { op: LogicOp, lhs: Box<Expr>, rhs: Box<Expr> },           // and/or com curto-circuito
    Compare { first: Box<Expr>, rest: Vec<(CmpOp, Expr)> },            // a < b <= c
    Call    { callee: Box<Expr>, args: Vec<Expr> },
    Attr    { base: Box<Expr>, attr: Ident },                          // gpu.range, math.sqrt
    Index   { base: Box<Expr>, index: Box<Expr> },
}

#[derive(Debug)]
pub struct TypeExpr { pub id: NodeId, pub kind: TypeExprKind, pub span: Span }

#[derive(Debug)]
pub enum TypeExprKind {
    Name(Ident),                                                       // int, f32
    Generic { base: Ident, args: Vec<TypeExpr> },                      // Array[f32]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp { Add, Sub, Mul, Div, FloorDiv, Mod, Pow, BitAnd, BitOr, BitXor, Shl, Shr }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp { Neg, Pos, Not, Invert }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicOp { And, Or }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp { Eq, Ne, Lt, Le, Gt, Ge }
```

### 2.1 Exemplo: programa e a sua AST

```python
def square_sum(a, b):
    s = a + b
    return s * s

total = 0
for i in range(10):
    total = total + square_sum(i, 1)
print(total)
```

```
Module
├─ Func square_sum(params=[a, b], ret=None)
│  ├─ Assign  target=Name(s)  value=Binary(Add, Name(a), Name(b))
│  └─ Return  Binary(Mul, Name(s), Name(s))
├─ Stmt Assign  target=Name(total)  value=Int(0)
├─ Stmt For  var=i  iter=Call(Name(range), [Int(10)])
│  └─ Assign  target=Name(total)
│             value=Binary(Add, Name(total), Call(Name(square_sum), [Name(i), Int(1)]))
└─ Stmt Expr  Call(Name(print), [Name(total)])
```

A sema infere `total: i64`, `i: i64` e gera a instância `square_sum(i64, i64) -> i64`. Não foi preciso escrever nenhuma anotação.

### 2.2 TIR (o que o codegen recebe)

```rust
// crates/cudra-tir/src/lib.rs
pub enum Ty { I32, I64, F32, F64, Bool, Unit, Array(Box<Ty>) }

pub struct LocalId(pub u32);
pub struct FuncId(pub u32);             // instância monomorfizada
pub struct KernelId(pub u32);

pub struct Func   { pub id: FuncId, pub name: String, pub params: Vec<LocalId>,
                    pub locals: Vec<Ty>, pub ret: Ty, pub body: Vec<TStmt> }
pub struct Kernel { pub id: KernelId, pub index_var: LocalId, pub captures: Vec<Capture>,
                    pub locals: Vec<Ty>, pub body: Vec<TStmt> }

pub enum Capture { Scalar { local: LocalId, ty: Ty }, Array { local: LocalId, elem: Ty, access: Access } }
pub enum Access { Read, Write, ReadWrite }

pub enum TStmt {
    Store    { local: LocalId, value: TExpr },
    StoreIdx { array: TExpr, index: TExpr, value: TExpr },
    ForRange { var: LocalId, start: TExpr, stop: TExpr, step: TExpr, body: Vec<TStmt> },
    While    { cond: TExpr, body: Vec<TStmt> },
    If       { cond: TExpr, then: Vec<TStmt>, orelse: Vec<TStmt> },
    Return(Option<TExpr>),
    Break, Continue,
    Expr(TExpr),
    GpuLaunch { kernel: KernelId, n: TExpr, args: Vec<LocalId> },
}

pub struct TExpr { pub ty: Ty, pub kind: TExprKind }
pub enum TExprKind {
    ConstInt(i64), ConstFloat(f64), ConstBool(bool),
    Local(LocalId),
    Binary { op: TBinOp, lhs: Box<TExpr>, rhs: Box<TExpr> },   // TBinOp já sabe se é int/float/signed
    Cast   { value: Box<TExpr> },                                 // destino = self.ty
    Call   { func: FuncId, args: Vec<TExpr> },
    Index  { array: Box<TExpr>, index: Box<TExpr>, checked: bool },
    Intrinsic { which: Intrinsic, args: Vec<TExpr> },             // print, len, sqrt, perf_counter
}
```

---

## 3. Pipeline de integração CPU + GPU

### 3.1 A pergunta honesta: quanto deve ser "automático"?

Decidir **sozinho** que um `for` normal vai para a GPU é um problema de investigação (análise de dependências, modelos de custo), e erra muito. Muitas vezes copiar os dados para a GPU demora mais do que o cálculo que se poupa. Proposta em três níveis:

| Nível | O que o utilizador escreve | Quem decide CPU vs GPU | Quando |
|---|---|---|---|
| **0** | `for i in gpu.range(n):` | O utilizador marca; o compilador faz **tudo o resto** (kernel, memória, lançamento) | **MVP, Fase 3** |
| **1** | `for i in parallel(n):` | **O runtime**, em tempo de execução: o binário traz o kernel GPU **e** uma versão CPU multi-thread e escolhe consoante `n` e se há GPU | **Fase 4** |
| **2** | `for i in range(n):` | O compilador prova que as iterações são independentes e paraleliza sozinho | Pós-MVP (I&D) |

A parte transparente é tudo o que é difícil: kernels, memória, cópias e configuração de lançamento. O utilizador só diz "isto é paralelo", com uma palavra que continua a parecer Python. O nível 1 é o melhor argumento para investidores: **um só binário, corre em qualquer máquina, usa a GPU se ela existir.**

### 3.2 Exemplo do que o utilizador escreve

```python
import gpu

def saxpy(a: f32, x: Array[f32], y: Array[f32]):
    for i in gpu.range(len(x)):
        y[i] = a * x[i] + y[i]

n = 10_000_000
x = Array[f32](n, 1.0)
y = Array[f32](n, 2.0)
saxpy(2.0, x, y)
print(y[0])            # aqui o runtime traz y de volta da GPU, sozinho
```

### 3.3 Fluxo no compilador

1. **Parser:** `gpu.range(...)` é só uma chamada numa expressão `For`. Nada de especial.
2. **Sema, `gpu_outline.rs`:**
   - Recolhe as **capturas** do corpo: variáveis de fora usadas lá dentro. Escalares passam por valor. `Array`s passam como ponteiro de device, marcados `Read`/`Write`/`ReadWrite` conforme o uso.
   - Valida as restrições de kernel, com erros claros e span: nada de `print`, alocações, `return`, nem escrever em escalares de fora (isso é uma redução, que chega na Fase 4). Só pode chamar funções puras, que são monomorfizadas **também** para o device.
   - **Verificação de corridas simples:** escrever num array capturado só é permitido na forma `arr[i]`, com `i` a variável do ciclo. Qualquer outra escrita dá erro *"possível corrida entre threads"*.
   - Aviso de desempenho: `f64` dentro de um kernel. Nas GPUs NVIDIA de consumo, o FP64 corre a 1/32–1/64 da velocidade do FP32. Isto é crucial para os benchmarks.
   - Retira o corpo para um `tir::Kernel` e põe no sítio dele um `TStmt::GpuLaunch`.
3. **Codegen NVPTX:** cada `Kernel` passa a uma função `ptx_kernel` com um **grid-stride loop**:
   `for (i = blockIdx.x*blockDim.x + threadIdx.x; i < n; i += blockDim.x*gridDim.x) { corpo }`.
   Assim uma grelha de tamanho fixo cobre qualquer `n`.
4. **Codegen host:** cada `GpuLaunch` passa a uma chamada ao runtime:
   `cudra_gpu_launch(module_ptx, "k3", n, args[], access[])`, e a uma chamada à versão CPU do mesmo corpo se não houver GPU.

### 3.4 Runtime e memória (Host ↔ Device)

Cada `Array[T]` é um descritor com **residência dupla** e **coerência preguiçosa**:

```
ArrayDesc { len, elem_size, host_ptr, dev_ptr (lazy), host_valid: bool, dev_valid: bool }
```

- **Antes de um kernel:** para cada array capturado em `Read`/`ReadWrite` com `!dev_valid`, faz `cuMemAlloc` (se preciso) e `cuMemcpyHtoD`, depois `dev_valid = true`.
- **Depois do kernel:** os arrays escritos ficam com `host_valid = false`. **Não copia de volta.**
- **Quando o host acede** (`y[0]`, `len` não conta): se `!host_valid`, faz `cuMemcpyDtoH`. Para não pagar um `if` em cada acesso dentro de ciclos quentes, o compilador **tira o `ensure_host(arr)` para fora do ciclo** quando o ciclo não lança kernels.
- Resultado: `k1(x); k2(x); k3(x)` seguidos fazem **uma** cópia de ida e nenhuma de volta até alguém ler no CPU. Esta é a otimização que separa uma demo de brincar de uma a sério.
- Módulos PTX: `cuModuleLoadData` uma vez por processo (o driver compila PTX → SASS e guarda em cache), `cuModuleGetFunction` guardado por nome.
- Configuração de lançamento: blocos de 256 threads, `grid = min(ceil(n/256), SMs × 32)`. O `cuOccupancyMaxPotentialBlockSize` fica para a Fase 4.
- **Rejeitado para o MVP:** Unified Memory (`cuMemAllocManaged`). No Windows não tem migração de páginas a pedido, e o desempenho é imprevisível. Mau para benchmarks.
- A `libcuda` é carregada com `dlopen`/`LoadLibrary` no arranque. Se falhar, `gpu_available = false` e tudo corre no CPU.

---

## 4. Marcos de desenvolvimento

Cada fase termina com **binários de demonstração**, um **`bench/run.py`** que gera uma tabela e um gráfico, e testes verdes (incluindo o oráculo CPython).

> ⚠️ **Regra dos benchmarks:** as entradas vêm de `argv` e os resultados são impressos. Senão o LLVM calcula o resultado em tempo de compilação (um `sum(range(n))` passa a uma fórmula fechada) e mostramos "∞× mais rápido". Um engenheiro do lado dos investidores apanha isso num segundo, e perde-se a credibilidade.

### Fase 1: "Hello, native" (base no CPU)
**Âmbito:** workspace, lexer com INDENT/DEDENT, parser completo da AST acima, sema com `int`/`float`/`bool` (**parâmetros anotados**, locais inferidos), `if`/`while`/`for range`, funções, recursão, `print`, codegen LLVM → executável. `cudrac --emit=tokens|ast|llvm`.
**Demos:** `fib.cdr` (recursivo), `primes.cdr` (crivo com ciclos), `collatz.cdr`.
**Benchmark:** vs CPython 3.13, PyPy, C (`clang -O2`), Rust. **Objetivo:** ±10% do C, e 30–100× mais rápido que o CPython.
**Estimativa:** 3–5 semanas.

### Fase 2: "Python sem anotações" (programas reais no CPU)
**Âmbito:** monomorfização (parâmetros sem tipo), `i32`/`f32`, `Array[T]` no CPU com bounds check (`--unchecked` para o desligar), `len`, `break`/`continue`, `math.sqrt/abs/min/max`, `time.perf_counter()`, `argv`, erros bonitos com `ariadne`, shim Python + oráculo CPython no CI.
**Demos:** Mandelbrot que **gera uma imagem PPM/PNG** (visual para o pitch), n-body, spectral-norm, matmul simples.
**Benchmark:** vs CPython, NumPy (atenção: no matmul o NumPy usa BLAS e ganha; é preciso dizê-lo), C, Rust, Numba (`@njit`).
**Estimativa:** 4–6 semanas.

### Fase 3: "Primeira GPU" (CUDA transparente, nível 0)
**Âmbito:** `gpu.range`, extração de kernels + validação, codegen NVPTX → PTX embebido, runtime com Driver API, coerência preguiçosa de memória, fallback no CPU sem GPU, `--emit=ptx`. Testes GPU numa instância cloud (RTX 4090 ou A100).
**Demos:** SAXPY (memory-bound: mostrar com honestidade o custo das cópias), Mandelbrot 8K na GPU vs CPU (compute-bound: o gráfico que impressiona), matmul simples na GPU.
**Benchmark:** vs a versão CPU do próprio cudra, NumPy, Numba CUDA, CuPy. **Mensagem:** "o mesmo código Python-like, uma palavra diferente, 50–200× mais rápido".
**Estimativa:** 5–8 semanas (é a fase de maior risco técnico).

### Fase 4: "Parallel everywhere + IA"
**Âmbito:** `parallel(n)` com decisão CPU-multithread vs GPU em runtime (nível 1), reduções (`total += x[i]` em kernel, com redução em árvore + atómicos), `exp`/`tanh` via libdevice, `f16` opcional, pequena biblioteca padrão de IA **escrita em cudra**: `matmul`, `relu`, `softmax`.
**Demo principal:** inferência de um **MLP no MNIST** (pesos exportados do PyTorch) escrita em cudra, a correr na GPU. Mostra-se lado a lado com o PyTorch em CPU e em GPU.
**Honestidade:** o PyTorch GPU usa cuBLAS/cuDNN, anos de otimização. O pitch não é "ganhamos ao cuBLAS". É "escreves o kernel em sintaxe Python e ficas perto, sem C++/CUDA".
**Estimativa:** 6–8 semanas.

**Total indicativo:** 4–6 meses para um programador com assistência de IA, com arranque logo e a Fase 1 como prova de ritmo.

---

## 5. Riscos e pontos a decidir (inputs do arquiteto)

1. **Concorrência: a primeira pergunta de qualquer investidor.** Este é exatamente o pitch do **Mojo** (Modular: sintaxe Python, MLIR, CPU+GPU, muito financiamento), do **Codon** (Exaloop: Python compilado via LLVM, com GPU) e, em parte, do **Numba**, **Triton**, **Taichi** e **JAX**. É preciso uma resposta clara a "porque não Mojo?". Ângulos possíveis: independência de fabricante (NVIDIA + AMD a partir do mesmo código: o backend AMDGPU do LLVM é o mesmo trabalho que o NVPTX, e este PC tem uma AMD), compatibilidade estrita com Python (Mojo não é), foco num nicho (edge/robótica, finanças, ciência), ou produto em cima da linguagem.
2. **Código fechado.** Linguagens fechadas têm muita dificuldade em ganhar adoção: os programadores não apostam carreiras em compiladores que não podem auditar. O Mojo começou fechado e abriu a biblioteca padrão por pressão. Sugestão: **fechado durante o MVP e a ronda** (protege o IP), com plano *open-core* (compilador aberto; runtime empresarial, deploy, profiler e suporte pagos).
3. **Nome.** "CUDA" é marca registada da NVIDIA, e "cudra" é muito parecido. Para um produto comercial convém confirmar com um advogado de marcas antes de gastar em branding.
4. **Ecossistema.** "Sintaxe Python" ≠ "corre bibliotecas Python". Numpy, PyTorch etc. não funcionam lá dentro. A interoperabilidade (chamar CPython a partir do cudra, e vice-versa através de um módulo de extensão) é o que decide a adoção a médio prazo. Fica fora do MVP, mas convém ter resposta.
5. **Hardware.** Não há GPU NVIDIA local. Orçamento de cloud estimado para as Fases 3–4: dezenas de euros, com instâncias à hora ligadas só para testar.
6. **"Velocidade de Assembly".** A promessa realista é *paridade com C/Rust* (mesmo backend LLVM). Prometer mais do que isso a investidores técnicos é arriscado.

---

## 6. Primeiros passos (se o plano for aprovado)

1. Instalar WSL2 Ubuntu 24.04 + `rustup` + LLVM da apt.llvm.org (versão suportada pelo `inkwell` 0.8), + `clang`, + `hyperfine`, + Python 3.13.
2. `git init` em `C:\Users\awtop\Documents\cudraAI` (ou dentro do WSL, onde a compilação é muito mais rápida).
3. Plano detalhado da **Fase 1** (ficheiro a ficheiro, com critérios de aceitação) → delegar ao `implementer` por partes independentes: `span` + `diagnostics` + `lexer` numa, `ast` + `parser` noutra, depois `sema` e `codegen`.
