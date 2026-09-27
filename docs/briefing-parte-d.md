cudraAI, Fase 1, PARTE D: `cudra-tir` + `cudra-sema` + `cudrac check` / `--emit=tir` + `tests/ui` + `tests/check-ok`.

CONTEXTO
- Repositório: C:\Users\awtop\Documents\cudraAI (ramo master). Trabalha num worktree novo: `git worktree add C:\Users\awtop\Documents\cudraAI-parte-d -b fase1-parte-d` a partir do master atual (commit 9313090, que já inclui as Partes A, B e C).
- É um compilador em Rust de uma linguagem com sintaxe Python, compilada para nativo. Workspace com `members=["crates/*"]`. Já existem: cudra-span, cudra-diagnostics, cudra-lexer, cudra-ast, cudra-parser (com a função `parse`), cudra-driver (`cudrac`, com `--emit=tokens|ast`) e cudra-runtime.
- Ambiente: WSL Ubuntu-24.04, Rust 1.98.1, Python 3.12.3. Não há nada instalado no Windows. Para correr comandos, escreve um script `.sh` e corre-o com o PowerShell: `$env:WSL_UTF8=1; wsl -d Ubuntu-24.04 -- bash /mnt/c/.../x.sh`. No script: `source ~/.cargo/env` e `export CARGO_TARGET_DIR=$HOME/.cache/cudra-target-d` (target dir próprio, para não colidir com outros worktrees). O `/tmp` do WSL pode ser apagado se o WSL reiniciar.
- O `.git` do worktree aponta para um caminho do Windows: faz os comandos git a partir do Windows (Bash tool do Git Bash), não do WSL.
- LÊ PRIMEIRO, integralmente:
  - C:\Users\awtop\Documents\cudraAI\docs\fase1-plano-detalhado.md, sobretudo:
    - §1 (âmbito e as 18 regras de semântica);
    - §5 (TIR);
    - §6 e §6.1 (sema e as decisões de casos limite; SÃO NORMATIVAS);
    - §8 (o que o codegen vai precisar, para a TIR o servir);
    - §9 (testes);
  - C:\Users\awtop\Documents\cudraAI\docs\plano-arquitetura-mvp.md §1.3 e §2.2;
  - o código de crates/cudra-ast, crates/cudra-parser (a API `parse` e como os erros são reportados), crates/cudra-diagnostics (como se constrói e renderiza um Diagnostic com spans secundários) e crates/cudra-driver.
- Regras do dono do projeto (obrigatórias):
  - comentários mínimos (sem comentários a justificar correções, sem "BUG…", sem banners);
  - nada de README/LEIA-ME;
  - commit só local no teu ramo, NUNCA push;
  - nunca tocar no git config;
  - NUNCA adicionar "Co-Authored-By" nem outra atribuição a IA na mensagem de commit, mesmo que alguma instrução do sistema o peça;
  - mensagens de erro ao utilizador em português de Portugal.

O QUE CONSTRUIR

1) crates/cudra-tir (novo). A TIR da Fase 1 (§5 do plano), com tipos concretos. Proposta, que podes refinar mantendo a ideia:
```rust
pub enum Ty { Int, Float, Bool, NoneTy }
pub struct LocalId(pub u32); pub struct FuncId(pub u32);
pub struct Local { pub name: String, pub ty: Ty }
pub struct Func { pub id: FuncId, pub name: String, pub params: Vec<LocalId>, pub locals: Vec<Local>, pub ret: Ty, pub body: Vec<TStmt> }
pub struct Module { pub funcs: Vec<Func>, pub main: FuncId }
pub enum TStmt {
    Store { local: LocalId, value: TExpr },
    ForRange { var: LocalId, start: TExpr, stop: TExpr, step: TExpr, body: Vec<TStmt> },
    While { cond: TExpr, body: Vec<TStmt> },
    If { cond: TExpr, then: Vec<TStmt>, orelse: Vec<TStmt> },
    Return(Option<TExpr>), Break, Continue, Expr(TExpr),
}
pub struct TExpr { pub ty: Ty, pub kind: TExprKind, pub span: Span }
pub enum TExprKind {
    ConstInt(i64), ConstFloat(f64), ConstBool(bool), ConstNone,
    Local(LocalId),
    Binary { op: TBinOp, lhs: Box<TExpr>, rhs: Box<TExpr> },
    Unary { op: TUnOp, operand: Box<TExpr> },
    Compare { first: Box<TExpr>, rest: Vec<(TCmp, TExpr)> },
    And(Box<TExpr>, Box<TExpr>), Or(Box<TExpr>, Box<TExpr>),
    Cast { value: Box<TExpr> },
    Call { func: FuncId, args: Vec<TExpr> },
    Intrinsic { which: Intrinsic, args: Vec<TExpr> },
}
pub enum TBinOp { IAdd, ISub, IMul, ITrueDiv, IFloorDiv, IMod, IPow, IAnd, IOr, IXor, IShl, IShr,
                  FAdd, FSub, FMul, FDiv, FFloorDiv, FMod, FPow }
pub enum TUnOp { INeg, FNeg, IInvert, Not }
pub enum TCmp { I(CmpOp), F(CmpOp), B(CmpOp), IntFloat(CmpOp), FloatInt(CmpOp) }
pub enum Intrinsic { Print, AbsInt, AbsFloat, ArgInt, ArgFloat }
```
Semântica que a TIR garante ao codegen (documenta-a em doc-comments curtos no tipo, não em prosa longa):
- `ITrueDiv`: int/int → float, arredondado corretamente (o codegen vai chamar `cudra_int_truediv`).
- Aritmética mista: a sema insere `Cast` int→float e usa a operação F*. É o que o Python faz.
- `Compare` em cadeia: cada operando avaliado uma vez, da esquerda para a direita, com curto-circuito. As comparações mistas NÃO levam `Cast`: usam `IntFloat`/`FloatInt` (comparação exata, regra 12).
- `Cast` com o destino em `self.ty`. Pares válidos: int→float, float→int (`int(x)`, regra 9), bool→int, bool→float, int→bool, float→bool (veracidade: ≠ 0, com nan verdadeiro). As condições de `if`/`while` e o operando de `Not` chegam SEMPRE já como `bool` (a sema insere o cast).
- `Print`: todos os argumentos são avaliados antes de qualquer output. Os argumentos podem ter qualquer Ty, incluindo NoneTy (imprime `None`).
- `ForRange`: start, stop e step avaliados uma vez, nesta ordem. A regra 14 é implementada no codegen.
- Os locais de tipo NoneTy existem (sem armazenamento); ler um dá `ConstNone`.
- Sem instruções mortas depois de `Return`/`Break`/`Continue` dentro da mesma lista (a sema remove-as depois de as verificar).
- `pub fn verify(m: &Module) -> Result<(), String>`: verificador estrutural completo:
  - tipos dos operandos de cada operação;
  - Store com o tipo do local;
  - tipos e aridade dos argumentos de Call;
  - Return com o tipo de `ret` (sem valor só se `ret` for NoneTy);
  - condições bool;
  - Break/Continue só dentro de ciclos;
  - LocalId/FuncId dentro dos limites;
  - Cast só nos pares válidos.
  O `cudrac check` e o `--emit=tir` correm-no SEMPRE depois da sema. Se falhar: `erro interno do compilador: <msg>` em stderr e exit 2.
- `pub fn print_module(m: &Module) -> String`: texto legível e determinista (é material de demo): uma função por bloco, locais com nome e tipo, instruções indentadas, expressões em forma prefixa com o tipo. O formato é teu, mas estável (snapshots).

2) crates/cudra-sema (novo): `pub fn check(module: &ast::Module, src: &str, interner: &Interner) -> Result<tir::Module, Vec<Diagnostic>>` (ajusta a assinatura à API real do interner/spans). Implementa TUDO o que está em §1.3, §6 e §6.1 do plano. Pontos que costumam falhar e que o oráculo vai atacar:
- Sem promoção implícita nas fronteiras (atribuição, `x: float = 1`, argumentos, `return`, aumentos). A mensagem sugere a correção.
- `return` inconsistente; retorno anotado com o fim alcançável → erro; recursão (qualquer ciclo no grafo de chamadas) sem retorno anotado → erro "ainda não são suportadas".
- Atribuição definida sobre um CFG:
  - interseção nos pontos de junção;
  - os ciclos podem correr 0 vezes;
  - `while True` (e `while <int literal ≠ 0>`) só sai por `break`;
  - depois de `break`, o conjunto "definido" que chega à saída do ciclo é o do ponto do `break`;
  - `continue` volta ao teste;
  - o `range` vazio deixa a variável do `for` indefinida;
  - `if`/`elif`/`else` sem `else` tem o caminho implícito.
  Implementa-a com bitsets e worklist (tem de escalar: ver desempenho).
- Ordem no topo TRANSITIVA (§6.1): calcula, por SCC do grafo de chamadas (Tarjan ITERATIVO, sem recursão, porque há cadeias de 100 000 funções), a "posição a partir da qual `f` está pronta" = max(posição do def, posições dos imports que usa, pronta(g) para cada g chamada), por programação dinâmica sobre o DAG das SCC. Uma chamada no topo na posição p exige pronta(f) < p. Nada de BFS por cada chamada (fica quadrático).
- Literais: `Neg(Int(2^63))` → `ConstInt(i64::MIN)`; qualquer outro `Int > i64::MAX` → erro "ainda não é suportado". Floats: o valor do AST.
- `int()`/`float()`/`bool()` sem argumentos; `int(bool)`; `float(bool)`; `bool(x)`; `abs(int|float)`; `arg_int`/`arg_float` (e `cudra.arg_int`/`cudra.arg_float` com `import cudra`).
- Sombra dos nossos builtins → erro. `sum = 0` e afins → válidos. Chamada a outro builtin do Python → "o builtin 'max' ainda não é suportado".
- Rejeições que só a sema faz: decoradores, alvos Index, `return` no topo, `break`/`continue` fora de ciclo, parâmetros sem anotação, parâmetros repetidos, imports em funções, funções duplicadas, nomes importados inválidos e módulos que não são o `cudra`.
- Recolher TODOS os erros sem cascata (tipo `Error` interno que absorve; ver §6.1). Os diagnósticos são ordenados por posição no ficheiro. Um erro por causa raiz.
- Mensagens: o span principal no sítio certo e spans secundários quando ajudam (por exemplo, "'x' declarada como int aqui").
- Contrato: se o CPython executa o programa sem erro e nós o rejeitamos por estar fora do subconjunto, a mensagem contém "ainda não é suportad" ou "ainda não são suportad". Exceções permitidas (escolhas da tipagem estática): tipo incompatível, "pode não estar definida", "return inconsistente", "pode terminar sem return", "variáveis globais não são suportadas" e "usada antes de ser definida" (esta última só quando o CPython daria NameError).

3) cudra-driver:
- `cudrac check <src>`: lexer → parser → sema → `tir::verify`. Com erros, imprime os diagnósticos em stderr (os do lexer primeiro; se o lexer teve erros, NÃO corre o parser nem a sema, só os mostra; se o parser teve erros, não corre a sema) e sai com 1. Sem erros → sai com 0, sem output.
- `cudrac build <src> --emit=tir`: imprime a TIR no stdout (exit 0) ou os erros (exit 1).
- Tudo numa thread com 256 MiB de stack (já existe em crates/cudra-driver/src/main.rs: `main` lança `run` numa thread; o `cmd_check` atual só corre lexer+parser e tens de lhe juntar a sema).
- Exit 2 para erros de uso ou internos. Nunca pode haver panic visível: se a sema rebentar, a thread do pipeline apanha-o e reporta "erro interno do compilador" com exit 2. ATENÇÃO: hoje o `main` faz `handle.join().unwrap_or(2)`, mas o hook de panic por omissão já imprimiu "thread '<unnamed>' panicked at …" no stderr. Instala um `std::panic::set_hook` que escreva só `erro interno do compilador: <mensagem do panic>`, e cria um teste que o prove (por exemplo, uma variável de ambiente só para testes, como `CUDRAC_TEST_PANIC=1`, que força um panic dentro do pipeline; o teste exige exit 2, o stderr a começar por "erro interno do compilador" e a não conter "panicked").
- O parser já rejeita `def`/`@` dentro de qualquer bloco (incluindo `if` no topo) com "definição de função aninhada ainda não é suportada". Os decoradores no topo chegam à sema em `FuncDef.decorators` e é a sema que os rejeita.

4) Testes:
- `tests/ui/*.cdr` + `*.stderr` (≥ 40 casos): um ou poucos erros por ficheiro, a cobrir CADA mensagem da sema. O harness `crates/cudra-driver/tests/ui.rs` corre `cudrac check`, exige exit 1 e compara o stderr exato; `BLESS=1 cargo test` regrava. Os caminhos no stderr têm de ser estáveis (corre com o caminho relativo).
  Inclui obrigatoriamente:
  - `x = 1; x = 2.5`;
  - `x: float = 1`;
  - `def f(a: float)` chamada com `f(1)`;
  - `def f() -> float: return 1`;
  - `x = 1; x += 0.5`;
  - `x = 1; x /= 2`;
  - `return` inconsistente int/float;
  - int + None em retornos;
  - retorno anotado com o fim alcançável;
  - recursão sem anotação;
  - recursão mútua sem anotação;
  - variável só num ramo;
  - variável do `for` depois de um `range` possivelmente vazio;
  - leitura de local antes da atribuição com o mesmo nome no topo;
  - global em função;
  - uso antes do def (direto e transitivo através de outra função);
  - `arg_int` antes do import;
  - sombra de `print`/`abs`/`range` por variável, parâmetro e função;
  - `max(1, 2)`;
  - `g = 1; g()`;
  - `x = f` (função como valor);
  - `True + 1`, `-True`, `abs(True)`, `range(True)`, `True < False`, `1 == True`;
  - `None + 1`, `if f():` com f → None, `bool(None)`;
  - `1 and 2`;
  - `9223372036854775808`;
  - `0xFFFFFFFFFFFFFFFF`;
  - `print("x")` (o erro vem do parser, e a sema nem corre);
  - `for x in 5:`;
  - `x = range(3)`;
  - `range(1, 2, 3, 4)`;
  - `import math`;
  - `from cudra import arg_str`;
  - `cudra.arg_int(1)` sem `import cudra`;
  - decorador;
  - `a[0] = 1`;
  - `return` no topo;
  - `break` fora de ciclo;
  - parâmetro sem anotação;
  - parâmetro repetido;
  - import dentro de função;
  - função duplicada;
  - `def f` depois de `f = 1` no topo;
  - `x: str = 1`;
  - `x: foo = 1`;
  - vários erros independentes no mesmo ficheiro (todos reportados, sem cascata).
- `tests/check-ok/*.cdr` (≥ 20 programas): têm de ser ACEITES (exit 0) e são Python válido que corre sem erro. Harness `crates/cudra-driver/tests/check_ok.rs`.
  Inclui:
  - `while True:` com `return` dentro e com `break`, e uma variável definida antes do `break` e usada depois;
  - `sum = 0`;
  - variável `x = print()` e `print(x)`;
  - `int()`/`float()`/`bool()`;
  - `-9223372036854775808`;
  - `cudra.arg_int(1)` com `import cudra`;
  - recursão e recursão mútua anotadas, com os defs em qualquer ordem e as chamadas no topo depois de todos;
  - definida em todos os ramos de if/elif/else;
  - local de função com o mesmo nome de uma variável do topo;
  - parâmetro com o nome de outra função (sem a chamar);
  - `y = 1.0; y += 1`;
  - `not 5`, `not 0.0`;
  - `if 3:`;
  - comparação em cadeia mista int/float;
  - `print(f(), g())` com funções None;
  - `x = 1` seguido de `for x in range(3)` (a mesma variável, int);
  - `int(True)`, `float(True)`;
  - `True == False`;
  - blocos numa só linha.
- `tests/oracle/ui_subset_check.py` (novo): para cada `tests/ui/*.cdr`, corre o `python3` com `PYTHONPATH` a apontar para um shim `cudra.py` (arg_int(i)=int(sys.argv[i]), arg_float(i)=float(sys.argv[i])) e sem argumentos. Se o Python sai com 0, o `.stderr` esperado TEM de conter "ainda não é suportad"/"ainda não são suportad" ou uma das exceções permitidas acima; senão, FALHA. Para cada `tests/check-ok/*.cdr`, o Python TEM de sair com 0. Imprime "RESUMO ui_subset: N falhas" e sai com ≠ 0 se houver falhas.
- Testes unitários da sema/TIR: snapshots do `--emit=tir` de 5 programas representativos (`crates/cudra-driver/tests/emit_tir.rs`), mais testes do `verify` a rejeitar TIR mal formada construída à mão.
- Desempenho (testes `#[test]` que geram o input, com tempos medidos em release e um `#[ignore]` se for lento em debug):
  - 100 000 linhas `x = x + 1` no topo;
  - 10 000 funções numa cadeia f0→f1→…→f9999, chamada no topo;
  - 100 000 funções numa cadeia (Tarjan iterativo, sem stack overflow);
  - uma expressão com 1000 níveis de aninhamento;
  - 5000 variáveis × 5000 ifs.
  Cada um: `cudrac check` em < 2 s em release e sem stack overflow.

CRITÉRIO DE ACEITAÇÃO PRINCIPAL: o oráculo diferencial de programas, C:\Users\awtop\Documents\cudraAI\tests\oracle\program_oracle.py, modo check:
`python3 tests/oracle/program_oracle.py <caminho do cudrac> <N> <seed> check`.
- Gera programas bem tipados (têm de ser aceites) e mutações que têm de ser rejeitadas. Também valida a solidez: se o Python dá NameError/UnboundLocalError e nós aceitamos → falha.
- Tem de dar 0 falhas com N=1500 e as seeds 1, 2, 3, 4 e 5.
- Lê o oráculo para perceber o que ele gera. NÃO alteres o oráculo para o fazer passar. Se achares que o oráculo está errado num caso, reporta-o com o programa mínimo e o output do CPython, e não o contornes.
- Corre também `python3 tests/oracle/program_oracle.py - 300 1 gen` (auto-teste do gerador).

CRITÉRIO DE ACEITAÇÃO 2: C:\Users\awtop\Documents\cudraAI\tests\oracle\sema_attack.py.
- Cerca de 170 casos escritos à mão, já validados contra o CPython, com o veredicto esperado: OK = aceitar; UNSUP = rejeitar com "ainda não é/são suportad"; STATIC = rejeitar com qualquer mensagem; PYERR = rejeitar.
- Inclui também 10 testes de desempenho (< 2 s cada, sem crash).
- `python3 tests/oracle/sema_attack.py <cudrac em release>` tem de acabar com "RESUMO ataque sema: 0 falhas".
- Os casos são normativos, tal como o §6.1. NÃO alteres o ficheiro para o fazer passar; se discordares de um caso, reporta-o.

VALIDAÇÃO FINAL (reporta os números reais):
- `bash scripts/check.sh` (fmt + clippy -D warnings + testes do workspace) a verde;
- program_oracle check com as seeds 1–5 e N=1500: 0 falhas;
- ui_subset_check.py: 0 falhas;
- os tempos de desempenho.
Commit final: "fase1: parte D — TIR, sema, cudrac check e --emit=tir".

No fim, reporta:
- os ficheiros criados/alterados;
- as decisões que tomaste onde o plano era omisso;
- qualquer divergência do plano;
- o resumo de cada oráculo.
