use std::collections::HashSet;

use cudra_ast as ast;
use cudra_diagnostics::Diagnostic;
use cudra_span::Symbol;

use crate::{CheckCtx, FuncEntry, STy};

pub(crate) const BUILTINS: &[&str] = &["print", "int", "float", "bool", "abs", "range"];

/// `dir(builtins)` do CPython 3.12, sem os nossos e sem dunders/exceções.
pub(crate) const PY_BUILTINS: &[&str] = &[
    "abs",
    "aiter",
    "anext",
    "all",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "copyright",
    "credits",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "exit",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "license",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "quit",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
];

/// Resultado da resolução de topo, usado pelo `lower` para saber a ordem de
/// processamento das funções (calados antes de quem chama) e quais falharam
/// o requisito de anotação em ciclos de recursão.
pub(crate) struct ResolvePlan {
    /// grupos de funções (SCCs do grafo de chamadas), em ordem de
    /// dependência: quem é chamado aparece antes de quem chama.
    pub scc_order: Vec<Vec<Symbol>>,
    pub invalid_recursive: HashSet<Symbol>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TopKind {
    Var,
    Func,
    Import,
}

pub(crate) fn resolve(ctx: &mut CheckCtx, module: &ast::Module) -> ResolvePlan {
    let mut top_names: Vec<Option<TopKind>> = vec![None; ctx.interner.len()];
    let mut func_defs: Vec<(usize, &ast::FuncDef)> = Vec::new();

    for (pos, item) in module.items.iter().enumerate() {
        match item {
            ast::Item::Func(f) => {
                register_func_name(ctx, &mut top_names, f, pos);
                func_defs.push((pos, f));
            }
            ast::Item::Stmt(s) => {
                collect_top_names_in_stmt(ctx, &mut top_names, s, pos);
            }
        }
    }

    // Assinaturas (parâmetros/retorno anotados) + locais de cada função.
    for (pos, f) in &func_defs {
        if ctx.has_func(f.name.name) {
            continue; // duplicado: mantemos a primeira definição na tabela.
        }
        let is_builtin_shadow = BUILTINS.contains(&ctx.sym(f.name.name));
        if is_builtin_shadow {
            continue; // já reportado em `register_func_name`; não regista.
        }
        let params = resolve_params(ctx, f);
        let (ret, ret_annotated) = resolve_ret(ctx, f);
        let locals_hint = collect_locals(f);
        let id = cudra_tir::FuncId(ctx.func_order.len() as u32);
        ctx.func_order.push(f.name.name);
        ctx.functions[f.name.name.0 as usize] = Some(FuncEntry {
            id,
            params,
            ret,
            ret_annotated,
            def_pos: *pos,
            span: f.span,
            name_span: f.name.span,
            ready_pos: *pos,
            locals_hint,
        });
    }

    // Grafo de chamadas + dependências de imports, por índice de função.
    let order: Vec<Symbol> = ctx.func_order.clone();
    let n_funcs = order.len();
    let mut call_edges: Vec<Vec<Symbol>> = vec![Vec::new(); n_funcs];
    let mut import_deps: Vec<Vec<Symbol>> = vec![Vec::new(); n_funcs];
    for (_, f) in &func_defs {
        let Some(entry) = ctx.func(f.name.name) else {
            continue;
        };
        let idx = entry.id.0 as usize;
        let mut calls = Vec::new();
        let mut imports_used = Vec::new();
        scan_block_deps(
            &f.body.stmts,
            &entry.locals_hint,
            &mut calls,
            &mut imports_used,
        );
        call_edges[idx] = calls;
        import_deps[idx] = imports_used;
    }

    let mut sym_to_func = vec![u32::MAX; ctx.interner.len()];
    for (i, &s) in order.iter().enumerate() {
        sym_to_func[s.0 as usize] = i as u32;
    }
    let (sccs, scc_of) = compute_order(n_funcs, &call_edges, &sym_to_func);
    let mut invalid_recursive: HashSet<Symbol> = HashSet::new();

    // Requisito de anotação em ciclos (incluindo auto-recursão).
    for scc in &sccs {
        let cyclic = scc.len() > 1
            || (scc.len() == 1
                && call_edges[scc[0] as usize]
                    .iter()
                    .any(|c| sym_to_func[c.0 as usize] == scc[0]));
        if !cyclic {
            continue;
        }
        for &i in scc {
            let name = order[i as usize];
            let annotated = ctx.func(name).is_some_and(|e| e.ret_annotated);
            if !annotated {
                invalid_recursive.insert(name);
                let span = ctx
                    .func(name)
                    .map(|e| e.name_span)
                    .unwrap_or(name_span_of(module));
                ctx.err(
                    Diagnostic::error(
                        "funções recursivas sem tipo de retorno anotado ainda não são suportadas na Fase 1"
                            .to_string(),
                    )
                    .with_primary(span, format!("'{}' precisa de um tipo de retorno anotado", ctx.sym(name))),
                );
            }
        }
    }

    // posição "pronta" por SCC, propagada por DP sobre o DAG (já em ordem
    // de dependência): max(def_pos, imports usados, pronta(chamadas)).
    let mut ready = vec![0usize; n_funcs];
    for scc in &sccs {
        let mut r = scc
            .iter()
            .map(|&i| ctx.func(order[i as usize]).map_or(0, |e| e.def_pos))
            .max()
            .unwrap_or(0);
        for &i in scc {
            for &dep in &import_deps[i as usize] {
                let is_import = ctx.sym(dep) == "cudra" || ctx.imports.names.contains_key(&dep);
                if is_import {
                    r = r.max(import_position(ctx, dep));
                }
            }
            for &callee in &call_edges[i as usize] {
                let j = sym_to_func[callee.0 as usize];
                if j == u32::MAX || scc_of[j as usize] == scc_of[i as usize] {
                    continue;
                }
                r = r.max(ready[j as usize]);
            }
        }
        for &i in scc {
            ready[i as usize] = r;
        }
    }
    for (i, &name) in order.iter().enumerate() {
        if let Some(e) = ctx.func_mut(name) {
            e.ready_pos = ready[i];
        }
    }

    let scc_order: Vec<Vec<Symbol>> = sccs
        .iter()
        .map(|scc| scc.iter().map(|&i| order[i as usize]).collect())
        .collect();

    ResolvePlan {
        scc_order,
        invalid_recursive,
    }
}

fn name_span_of(module: &ast::Module) -> cudra_span::Span {
    module.items.first().map_or(
        cudra_span::Span::new(cudra_span::FileId(0), 0, 0),
        |i| match i {
            ast::Item::Func(f) => f.name.span,
            ast::Item::Stmt(s) => s.span,
        },
    )
}

pub(crate) fn import_position(ctx: &CheckCtx, name: Symbol) -> usize {
    if ctx.sym(name) == "cudra" {
        ctx.imports.cudra_module_pos.unwrap_or(usize::MAX)
    } else {
        *ctx.imports.names.get(&name).unwrap_or(&usize::MAX)
    }
}

fn register_func_name(
    ctx: &mut CheckCtx,
    top_names: &mut [Option<TopKind>],
    f: &ast::FuncDef,
    _pos: usize,
) {
    let name = f.name.name;
    if !f.decorators.is_empty() {
        ctx.err(
            Diagnostic::error("decoradores ainda não são suportados na Fase 1".to_string())
                .with_primary(f.decorators[0].span, "decorador aqui"),
        );
    }
    if BUILTINS.contains(&ctx.sym(name)) {
        let text = ctx.sym(name).to_string();
        ctx.err(
            Diagnostic::error(format!(
                "fazer sombra ao builtin '{text}' ainda não é suportado na Fase 1"
            ))
            .with_primary(f.name.span, "sombra aqui"),
        );
        return;
    }
    match top_names[name.0 as usize] {
        Some(TopKind::Func) => {
            let text = ctx.sym(name).to_string();
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir a função '{text}' ainda não é suportado na Fase 1"
                ))
                .with_primary(f.name.span, "segunda definição"),
            );
        }
        Some(TopKind::Var) => {
            let text = ctx.sym(name).to_string();
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é uma variável) ainda não é suportado na Fase 1"
                ))
                .with_primary(f.name.span, "def aqui"),
            );
        }
        Some(TopKind::Import) => {
            let text = ctx.sym(name).to_string();
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é um nome importado) ainda não é suportado na Fase 1"
                ))
                .with_primary(f.name.span, "def aqui"),
            );
        }
        None => {
            top_names[name.0 as usize] = Some(TopKind::Func);
        }
    }
}

fn collect_top_names_in_stmt(
    ctx: &mut CheckCtx,
    top_names: &mut [Option<TopKind>],
    s: &ast::Stmt,
    pos: usize,
) {
    match &s.kind {
        ast::StmtKind::Assign {
            target: ast::Target::Name(id),
            ..
        }
        | ast::StmtKind::AugAssign {
            target: ast::Target::Name(id),
            ..
        } => declare_top_var(ctx, top_names, id),
        ast::StmtKind::For { var, body, .. } => {
            declare_top_var(ctx, top_names, var);
            for st in &body.stmts {
                collect_top_names_in_stmt(ctx, top_names, st, pos);
            }
        }
        ast::StmtKind::While { body, .. } => {
            for st in &body.stmts {
                collect_top_names_in_stmt(ctx, top_names, st, pos);
            }
        }
        ast::StmtKind::If {
            then,
            elifs,
            orelse,
            ..
        } => {
            for st in &then.stmts {
                collect_top_names_in_stmt(ctx, top_names, st, pos);
            }
            for (_, b) in elifs {
                for st in &b.stmts {
                    collect_top_names_in_stmt(ctx, top_names, st, pos);
                }
            }
            if let Some(o) = orelse {
                for st in &o.stmts {
                    collect_top_names_in_stmt(ctx, top_names, st, pos);
                }
            }
        }
        ast::StmtKind::Import { module } => {
            let text = ctx.sym(module.name).to_string();
            if text != "cudra" {
                ctx.err(
                    Diagnostic::error(format!("o módulo '{text}' ainda não é suportado na Fase 1"))
                        .with_primary(module.span, "import"),
                );
            } else if ctx.imports.cudra_module_pos.is_none() {
                ctx.imports.cudra_module_pos = Some(pos);
            }
        }
        ast::StmtKind::FromImport { module, names } => {
            let text = ctx.sym(module.name).to_string();
            if text != "cudra" {
                ctx.err(
                    Diagnostic::error(format!("o módulo '{text}' ainda não é suportado na Fase 1"))
                        .with_primary(module.span, "import"),
                );
                return;
            }
            for n in names {
                let nt = ctx.sym(n.name).to_string();
                if nt != "arg_int" && nt != "arg_float" {
                    ctx.err(
                        Diagnostic::error(format!("'cudra' não tem '{nt}'"))
                            .with_primary(n.span, "nome importado inválido"),
                    );
                    continue;
                }
                match top_names[n.name.0 as usize] {
                    Some(TopKind::Func) => {
                        ctx.err(
                            Diagnostic::error(format!(
                                "redefinir '{nt}' (já é uma função) ainda não é suportado na Fase 1"
                            ))
                            .with_primary(n.span, "import aqui"),
                        );
                    }
                    Some(TopKind::Var) => {
                        ctx.err(
                            Diagnostic::error(format!(
                                "redefinir '{nt}' (já é uma variável) ainda não é suportado na Fase 1"
                            ))
                            .with_primary(n.span, "import aqui"),
                        );
                    }
                    Some(TopKind::Import) | None => {
                        top_names[n.name.0 as usize] = Some(TopKind::Import);
                    }
                }
                let e = ctx.imports.names.entry(n.name).or_insert(pos);
                if pos < *e {
                    *e = pos;
                }
            }
        }
        _ => {}
    }
}

fn declare_top_var(ctx: &mut CheckCtx, top_names: &mut [Option<TopKind>], id: &ast::Ident) {
    if BUILTINS.contains(&ctx.sym(id.name)) && top_names[id.name.0 as usize].is_none() {
        let text = ctx.sym(id.name).to_string();
        ctx.err(
            Diagnostic::error(format!(
                "fazer sombra ao builtin '{text}' ainda não é suportado na Fase 1"
            ))
            .with_primary(id.span, "sombra aqui"),
        );
    }
    match top_names[id.name.0 as usize] {
        Some(TopKind::Func) => {
            let text = ctx.sym(id.name).to_string();
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é uma função) ainda não é suportado na Fase 1"
                ))
                .with_primary(id.span, "atribuição aqui"),
            );
        }
        Some(TopKind::Import) => {
            let text = ctx.sym(id.name).to_string();
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é um nome importado) ainda não é suportado na Fase 1"
                ))
                .with_primary(id.span, "atribuição aqui"),
            );
        }
        Some(TopKind::Var) => {}
        None => {
            top_names[id.name.0 as usize] = Some(TopKind::Var);
            ctx.top_vars.insert(id.name);
        }
    }
}

fn resolve_params(ctx: &mut CheckCtx, f: &ast::FuncDef) -> Vec<(Symbol, STy)> {
    let mut out = Vec::with_capacity(f.params.len());
    for p in &f.params {
        if out.iter().any(|(s, _)| *s == p.name.name) {
            ctx.err(
                Diagnostic::error(format!("parâmetro '{}' repetido", ctx.sym(p.name.name)))
                    .with_primary(p.span, "repetido"),
            );
        }
        if BUILTINS.contains(&ctx.sym(p.name.name)) {
            let text = ctx.sym(p.name.name).to_string();
            ctx.err(
                Diagnostic::error(format!(
                    "fazer sombra ao builtin '{text}' ainda não é suportado na Fase 1"
                ))
                .with_primary(p.name.span, "sombra aqui"),
            );
        }
        let ty = match &p.ty {
            Some(t) => resolve_type_expr(ctx, t),
            None => {
                ctx.err(
                    Diagnostic::error(
                        "parâmetros sem anotação de tipo ainda não são suportados na Fase 1",
                    )
                    .with_primary(p.span, "falta a anotação"),
                );
                STy::Error
            }
        };
        out.push((p.name.name, ty));
    }
    out
}

fn resolve_ret(ctx: &mut CheckCtx, f: &ast::FuncDef) -> (STy, bool) {
    match &f.ret {
        Some(t) => (resolve_type_expr(ctx, t), true),
        None => (STy::NoneTy, false),
    }
}

pub(crate) fn resolve_type_expr(ctx: &mut CheckCtx, t: &ast::TypeExpr) -> STy {
    match &t.kind {
        ast::TypeExprKind::Name(id) => {
            // os tipos válidos comparam-se sem alocar; só os erros
            // constroem a mensagem.
            let known = match ctx.sym(id.name) {
                "int" => Some(STy::Int),
                "float" => Some(STy::Float),
                "bool" => Some(STy::Bool),
                "None" => Some(STy::NoneTy),
                _ => None,
            };
            if let Some(ty) = known {
                return ty;
            }
            let text = ctx.sym(id.name).to_string();
            if matches!(
                text.as_str(),
                "str" | "list" | "dict" | "tuple" | "set" | "bytes"
            ) {
                ctx.err(
                    Diagnostic::error(format!("o tipo '{text}' ainda não é suportado na Fase 1"))
                        .with_primary(t.span, "tipo"),
                );
            } else {
                ctx.err(
                    Diagnostic::error(format!("tipo desconhecido '{text}'"))
                        .with_primary(t.span, "tipo"),
                );
            }
            STy::Error
        }
        ast::TypeExprKind::Generic { base, .. } => {
            let text = ctx.sym(base.name).to_string();
            ctx.err(
                Diagnostic::error(format!(
                    "o tipo '{text}[...]' ainda não é suportado na Fase 1"
                ))
                .with_primary(t.span, "tipo"),
            );
            STy::Error
        }
    }
}

fn collect_locals(f: &ast::FuncDef) -> Vec<Symbol> {
    let mut v: Vec<Symbol> = f.params.iter().map(|p| p.name.name).collect();
    walk_block_locals(&f.body.stmts, &mut v);
    v.sort_unstable();
    v.dedup();
    v
}

fn walk_block_locals(stmts: &[ast::Stmt], s: &mut Vec<Symbol>) {
    for st in stmts {
        walk_stmt_locals(st, s);
    }
}

fn walk_stmt_locals(st: &ast::Stmt, s: &mut Vec<Symbol>) {
    match &st.kind {
        ast::StmtKind::Assign {
            target: ast::Target::Name(id),
            ..
        }
        | ast::StmtKind::AugAssign {
            target: ast::Target::Name(id),
            ..
        } => {
            s.push(id.name);
        }
        ast::StmtKind::For { var, body, .. } => {
            s.push(var.name);
            walk_block_locals(&body.stmts, s);
        }
        ast::StmtKind::While { body, .. } => walk_block_locals(&body.stmts, s),
        ast::StmtKind::If {
            then,
            elifs,
            orelse,
            ..
        } => {
            walk_block_locals(&then.stmts, s);
            for (_, b) in elifs {
                walk_block_locals(&b.stmts, s);
            }
            if let Some(o) = orelse {
                walk_block_locals(&o.stmts, s);
            }
        }
        _ => {}
    }
}

/// Recolhe (a) chamadas a funções de topo que não estão na sombra de um
/// local desta função, e (b) o uso direto de nomes importados
/// (`arg_int`/`arg_float`/`cudra`), em todo o corpo.
fn scan_block_deps(
    stmts: &[ast::Stmt],
    locals: &[Symbol],
    calls: &mut Vec<Symbol>,
    imports_used: &mut Vec<Symbol>,
) {
    for st in stmts {
        scan_stmt_deps(st, locals, calls, imports_used);
    }
}

fn scan_stmt_deps(
    st: &ast::Stmt,
    locals: &[Symbol],
    calls: &mut Vec<Symbol>,
    imports_used: &mut Vec<Symbol>,
) {
    use ast::StmtKind::*;
    match &st.kind {
        Assign { target, value, .. } => {
            scan_target_deps(target, locals, calls, imports_used);
            scan_expr_deps(value, locals, calls, imports_used);
        }
        AugAssign { target, value, .. } => {
            scan_target_deps(target, locals, calls, imports_used);
            scan_expr_deps(value, locals, calls, imports_used);
        }
        For { iter, body, .. } => {
            scan_expr_deps(iter, locals, calls, imports_used);
            scan_block_deps(&body.stmts, locals, calls, imports_used);
        }
        While { cond, body } => {
            scan_expr_deps(cond, locals, calls, imports_used);
            scan_block_deps(&body.stmts, locals, calls, imports_used);
        }
        If {
            cond,
            then,
            elifs,
            orelse,
        } => {
            scan_expr_deps(cond, locals, calls, imports_used);
            scan_block_deps(&then.stmts, locals, calls, imports_used);
            for (c, b) in elifs {
                scan_expr_deps(c, locals, calls, imports_used);
                scan_block_deps(&b.stmts, locals, calls, imports_used);
            }
            if let Some(o) = orelse {
                scan_block_deps(&o.stmts, locals, calls, imports_used);
            }
        }
        Return(Some(e)) => scan_expr_deps(e, locals, calls, imports_used),
        Expr(e) => scan_expr_deps(e, locals, calls, imports_used),
        _ => {}
    }
}

fn scan_target_deps(
    target: &ast::Target,
    locals: &[Symbol],
    calls: &mut Vec<Symbol>,
    imports_used: &mut Vec<Symbol>,
) {
    if let ast::Target::Index { base, index } = target {
        scan_expr_deps(base, locals, calls, imports_used);
        scan_expr_deps(index, locals, calls, imports_used);
    }
}

// versão sem `ctx` (não precisa resolver texto de símbolos para o scan).
fn scan_expr_deps(
    e: &ast::Expr,
    locals: &[Symbol],
    calls: &mut Vec<Symbol>,
    imports_used: &mut Vec<Symbol>,
) {
    walk_expr(e, &mut |ex| {
        if let ast::ExprKind::Call { callee, .. } = &ex.kind {
            if let ast::ExprKind::Name(id) = &callee.kind {
                if locals.binary_search(&id.name).is_err() {
                    calls.push(id.name);
                }
            }
        }
        if let ast::ExprKind::Name(id) = &ex.kind {
            imports_used.push(id.name);
        }
    });
}

// aplica `f` a `e` e recursivamente a todas as subexpressões.
fn walk_expr(e: &ast::Expr, f: &mut impl FnMut(&ast::Expr)) {
    f(e);
    match &e.kind {
        ast::ExprKind::Unary { operand, .. } => walk_expr(operand, f),
        ast::ExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(lhs, f);
            walk_expr(rhs, f);
        }
        ast::ExprKind::Logical { lhs, rhs, .. } => {
            walk_expr(lhs, f);
            walk_expr(rhs, f);
        }
        ast::ExprKind::Compare { first, rest } => {
            walk_expr(first, f);
            for (_, e2) in rest {
                walk_expr(e2, f);
            }
        }
        ast::ExprKind::Call { callee, args } => {
            walk_expr(callee, f);
            for a in args {
                walk_expr(a, f);
            }
        }
        ast::ExprKind::Attr { base, .. } => walk_expr(base, f),
        ast::ExprKind::Index { base, index } => {
            walk_expr(base, f);
            walk_expr(index, f);
        }
        _ => {}
    }
}

/// SCCs do grafo de chamadas, em ordem de dependência (quem é chamado vem
/// primeiro), mais o id do SCC de cada função.
///
/// A adjacência vai numa representação CSR (dois `Vec` contíguos) em vez de
/// uma `Vec` por função: num grafo com uma `Vec` por nó, 100000 funções
/// custam 100000 alocações só para o grafo.
fn compute_order(
    n: usize,
    call_edges: &[Vec<Symbol>],
    sym_to_func: &[u32],
) -> (Vec<Vec<u32>>, Vec<u32>) {
    let mut starts: Vec<u32> = Vec::with_capacity(n + 1);
    let mut targets: Vec<u32> = Vec::with_capacity(n);
    for edges in call_edges.iter().take(n) {
        starts.push(targets.len() as u32);
        for c in edges {
            let j = sym_to_func[c.0 as usize];
            if j != u32::MAX {
                targets.push(j);
            }
        }
    }
    starts.push(targets.len() as u32);
    tarjan(n, &starts, &targets)
}

fn tarjan(n: usize, starts: &[u32], targets: &[u32]) -> (Vec<Vec<u32>>, Vec<u32>) {
    let mut index = vec![-1i64; n];
    let mut lowlink = vec![0i64; n];
    let mut on_stack = vec![false; n];
    let mut scc_id = vec![u32::MAX; n];
    let mut stack: Vec<u32> = Vec::new();
    let mut next_index: i64 = 0;
    let mut sccs: Vec<Vec<u32>> = Vec::new();

    for start in 0..n {
        if index[start] != -1 {
            continue;
        }
        let mut work: Vec<(u32, u32)> = vec![(start as u32, starts[start])];
        index[start] = next_index;
        lowlink[start] = next_index;
        next_index += 1;
        stack.push(start as u32);
        on_stack[start] = true;

        while let Some(&mut (v, ref mut next)) = work.last_mut() {
            let vi = v as usize;
            let end = starts[vi + 1];
            if *next < end {
                let w = targets[*next as usize];
                *next += 1;
                let wi = w as usize;
                if index[wi] == -1 {
                    index[wi] = next_index;
                    lowlink[wi] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[wi] = true;
                    work.push((w, starts[wi]));
                } else if on_stack[wi] && index[wi] < lowlink[vi] {
                    lowlink[vi] = index[wi];
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    let pi = parent as usize;
                    if lowlink[vi] < lowlink[pi] {
                        lowlink[pi] = lowlink[vi];
                    }
                }
                if lowlink[vi] == index[vi] {
                    let id = sccs.len() as u32;
                    let mut comp = Vec::new();
                    loop {
                        let w = stack.pop().expect("pilha do Tarjan não pode esvaziar aqui");
                        on_stack[w as usize] = false;
                        scc_id[w as usize] = id;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    sccs.push(comp);
                }
            }
        }
    }
    (sccs, scc_id)
}
