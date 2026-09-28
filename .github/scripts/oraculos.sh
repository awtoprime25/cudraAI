#!/bin/bash
# Corredores de exactidão do cudra.
#
# Cada oráculo confronta o compilador com o CPython e sai com 1 se houver
# qualquer divergência. É esta suite — e não os testes de UI — que sustenta a
# afirmação "o cudra faz o que o Python faz".
#
# Configurável pelo ambiente:
#   N      número de casos por modo        (omissão: 300)
#   SEEDS  sementes, separadas por espaço  (omissão: "1 2 3")
#
# Os binários têm de estar construídos antes; o job da CI faz isso.
set -euo pipefail
cd "$(dirname "$0")/../.."

N=${N:-300}
SEEDS=${SEEDS:-"1 2 3"}
target=${CARGO_TARGET_DIR:-target}
CUDRAC="$target/debug/cudrac"
DUMP_TOKENS="$target/debug/examples/dump_tokens"
DUMP_AST="$target/debug/examples/dump_ast"

for bin in "$CUDRAC" "$DUMP_TOKENS" "$DUMP_AST"; do
  if [[ ! -x "$bin" ]]; then
    echo "::error::falta o binário $bin — constrói-os antes deste script"
    exit 1
  fi
done

# Corre um oráculo e reporta o tempo. `set -e` garante que a primeira falha
# derruba o script: um oráculo a falhar no meio não deve parecer verde.
run() {
  local nome=$1
  shift
  echo
  echo "=== $nome"
  local t0=$SECONDS
  python3 "$@"
  echo "--- $nome ok ($((SECONDS - t0))s)"
}

# 1. Ataques ao frontend, com os casos embutidos nos próprios scripts: sem N,
#    porque o corpus é fixo e foi feito à mão.
run "sema_attack"   tests/oracle/sema_attack.py "$CUDRAC"
run "sema_attack2"  tests/oracle/sema_attack2.py "$CUDRAC"
run "run_attack"    tests/oracle/run_attack.py "$CUDRAC"
run "ui_subset"     tests/oracle/ui_subset_check.py "$CUDRAC"

# 2. Contrato das mensagens de truncagem: corta cada `.cdr` de `tests/ui` e
#    `tests/check-ok` em todas as posições UTF-8 e exige que o compilador não
#    entre em pânico nem devolva nada fora de {0, 1}.
run "trunc_fuzz"    tests/oracle/trunc_fuzz.py "$CUDRAC" "$PWD"

# 3. Lexer. `lexer_oracle` grava por semente, por isso o directório tem de ser
#    diferente para cada uma.
for seed in $SEEDS; do
  run "lexer_oracle semente $seed" \
    tests/oracle/lexer_oracle.py "$DUMP_TOKENS" "$N" "$seed" "/tmp/lexoracle_$seed"
  for mode in rich fuzz; do
    run "lexer_oracle_v2 semente $seed modo $mode" \
      tests/oracle/lexer_oracle_v2.py "$DUMP_TOKENS" "$N" "$seed" "$mode"
  done
done
run "lexer_attack"  tests/oracle/lexer_attack.py "$DUMP_TOKENS"

# 4. Parser: quatro modos, incluindo `deep`, que é o que apanha o estouro de
#    pilha nas cadeias de associatividade à esquerda.
for seed in $SEEDS; do
  for mode in valid wild fuzz; do
    run "parser_oracle semente $seed modo $mode" \
      tests/oracle/parser_oracle.py "$DUMP_AST" "$N" "$seed" "$mode"
  done
done
run "parser_oracle deep" \
  tests/oracle/parser_oracle.py "$DUMP_AST" "$N" "211" "deep"
run "parser_attack" tests/oracle/parser_attack.py "$DUMP_AST"

# 5. Programas completos: gera `.cdr` válidos e compara a saída do `cudrac`
#    com a do CPython. O modo `gen` só gera os ficheiros, o `check` é o que
#    compara as saídas — por isso são duas invocações, não uma.
for seed in $SEEDS; do
  run "program_oracle semente $seed (gen)" \
    tests/oracle/program_oracle.py "$CUDRAC" "$N" "$seed" "gen"
  run "program_oracle semente $seed (check)" \
    tests/oracle/program_oracle.py "$CUDRAC" "$N" "$seed" "check"
done

echo
echo "Todos os oráculos passaram: N=$N, sementes=$SEEDS"
