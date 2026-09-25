use std::collections::{HashMap, HashSet};

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
    let mut top_names: HashMap<Symbol, TopKind> = HashMap::new();
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
        if ctx.functions.contains_key(&f.name.name) {
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
        ctx.functions.insert(
            f.name.name,
            FuncEntry {
                id,
                params,
                ret,
                ret_annotated,
                def_pos: *pos,
                span: f.span,
                name_span: f.name.span,
                ready_pos: *pos,
                locals_hint,
            },
        );
    }

    // Grafo de chamadas + dependências de imports, por função.
    let mut call_edges: HashMap<Symbol, Vec<Symbol>> = HashMap::new();
    let mut import_deps: HashMap<Symbol, Vec<Symbol>> = HashMap::new();
    for (_, f) in &func_defs {
        if !ctx.functions.contains_key(&f.name.name) {
            continue;
        }
        let locals = &ctx.functions[&f.name.name].locals_hint;
        let mut calls = Vec::new();
        let mut imports_used = Vec::new();
        scan_block_deps(&f.body.stmts, locals, &mut calls, &mut imports_used);
        call_edges.insert(f.name.name, calls);
        import_deps.insert(f.name.name, imports_used);
    }

    let scc_order = compute_order(ctx, &call_edges);
    let mut invalid_recursive: HashSet<Symbol> = HashSet::new();

    // Requisito de anotação em ciclos (incluindo auto-recursão).
    for scc in &scc_order {
        let cyclic = scc.len() > 1
            || (scc.len() == 1
                && call_edges
                    .get(&scc[0])
                    .map(|c| c.contains(&scc[0]))
                    .unwrap_or(false));
        if !cyclic {
            continue;
        }
        for &name in scc {
            let annotated = ctx.functions[&name].ret_annotated;
            if !annotated {
                invalid_recursive.insert(name);
                let span = ctx.functions[&name].name_span;
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
    let mut scc_ready: HashMap<Symbol, usize> = HashMap::new();
    for scc in &scc_order {
        let mut ready = scc
            .iter()
            .map(|s| ctx.functions[s].def_pos)
            .max()
            .unwrap_or(0);
        for &name in scc {
            for dep in import_deps.get(&name).into_iter().flatten() {
                let is_import = ctx.sym(*dep) == "cudra" || ctx.imports.names.contains_key(dep);
                if is_import {
                    let p = import_position(ctx, *dep);
                    ready = ready.max(p);
                }
            }
            for callee in call_edges.get(&name).into_iter().flatten() {
                if scc.contains(callee) {
                    continue;
                }
                if let Some(&r) = scc_ready.get(callee) {
                    ready = ready.max(r);
                }
            }
        }
        for &name in scc {
            scc_ready.insert(name, ready);
        }
    }
    for (name, r) in &scc_ready {
        ctx.functions.get_mut(name).unwrap().ready_pos = *r;
    }

    ResolvePlan {
        scc_order,
        invalid_recursive,
    }
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
    top_names: &mut HashMap<Symbol, TopKind>,
    f: &ast::FuncDef,
    _pos: usize,
) {
    let name = f.name.name;
    let text = ctx.sym(name).to_string();
    if !f.decorators.is_empty() {
        ctx.err(
            Diagnostic::error("decoradores ainda não são suportados na Fase 1".to_string())
                .with_primary(f.decorators[0].span, "decorador aqui"),
        );
    }
    if BUILTINS.contains(&text.as_str()) {
        ctx.err(
            Diagnostic::error(format!(
                "fazer sombra ao builtin '{text}' ainda não é suportado na Fase 1"
            ))
            .with_primary(f.name.span, "sombra aqui"),
        );
        return;
    }
    match top_names.get(&name) {
        Some(TopKind::Func) => {
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir a função '{text}' ainda não é suportado na Fase 1"
                ))
                .with_primary(f.name.span, "segunda definição"),
            );
        }
        Some(TopKind::Var) => {
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é uma variável) ainda não é suportado na Fase 1"
                ))
                .with_primary(f.name.span, "def aqui"),
            );
        }
        Some(TopKind::Import) => {
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é um nome importado) ainda não é suportado na Fase 1"
                ))
                .with_primary(f.name.span, "def aqui"),
            );
        }
        None => {
            top_names.insert(name, TopKind::Func);
        }
    }
}

fn collect_top_names_in_stmt(
    ctx: &mut CheckCtx,
    top_names: &mut HashMap<Symbol, TopKind>,
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
                match top_names.get(&n.name) {
                    Some(TopKind::Func) => {
                        ctx.err(
                            Diagnostic::error(format!(
                                "redefinir '{nt}' (já é uma função) ainda não é suportado"
                            ))
                            .with_primary(n.span, "import aqui"),
                        );
                    }
                    Some(TopKind::Var) => {
                        ctx.err(
                            Diagnostic::error(format!(
                                "redefinir '{nt}' (já é uma variável) ainda não é suportado"
                            ))
                            .with_primary(n.span, "import aqui"),
                        );
                    }
                    Some(TopKind::Import) | None => {
                        top_names.insert(n.name, TopKind::Import);
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

fn declare_top_var(ctx: &mut CheckCtx, top_names: &mut HashMap<Symbol, TopKind>, id: &ast::Ident) {
    let text = ctx.sym(id.name).to_string();
    if BUILTINS.contains(&text.as_str()) && !top_names.contains_key(&id.name) {
        ctx.err(
            Diagnostic::error(format!(
                "fazer sombra ao builtin '{text}' ainda não é suportado na Fase 1"
            ))
            .with_primary(id.span, "sombra aqui"),
        );
    }
    match top_names.get(&id.name) {
        Some(TopKind::Func) => {
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é uma função) ainda não é suportado na Fase 1"
                ))
                .with_primary(id.span, "atribuição aqui"),
            );
        }
        Some(TopKind::Import) => {
            ctx.err(
                Diagnostic::error(format!(
                    "redefinir '{text}' (já é um nome importado) ainda não é suportado na Fase 1"
                ))
                .with_primary(id.span, "atribuição aqui"),
            );
        }
        Some(TopKind::Var) => {}
        None => {
            top_names.insert(id.name, TopKind::Var);
            ctx.top_vars.insert(id.name);
        }
    }
}

fn resolve_params(ctx: &mut CheckCtx, f: &ast::FuncDef) -> Vec<(Symbol, STy)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in &f.params {
        if !seen.insert(p.name.name) {
            ctx.err(
                Diagnostic::error(format!("parâmetro '{}' repetido", ctx.sym(p.name.name)))
                    .with_primary(p.span, "repetido"),
            );
        }
        let text = ctx.sym(p.name.name).to_string();
        if BUILTINS.contains(&text.as_str()) {
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
            let text = ctx.sym(id.name).to_string();
            match text.as_str() {
                "int" => STy::Int,
                "float" => STy::Float,
                "bool" => STy::Bool,
                "None" => STy::NoneTy,
                "str" | "list" | "dict" | "tuple" | "set" | "bytes" => {
                    ctx.err(
                        Diagnostic::error(format!(
                            "o tipo '{text}' ainda não é suportado na Fase 1"
                        ))
                        .with_primary(t.span, "tipo"),
                    );
                    STy::Error
                }
                _ => {
                    ctx.err(
                        Diagnostic::error(format!("tipo desconhecido '{text}'"))
                            .with_primary(t.span, "tipo"),
                    );
                    STy::Error
                }
            }
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

fn collect_locals(f: &ast::FuncDef) -> HashSet<Symbol> {
    let mut s: HashSet<Symbol> = f.params.iter().map(|p| p.name.name).collect();
    walk_block_locals(&f.body.stmts, &mut s);
    s
}

fn walk_block_locals(stmts: &[ast::Stmt], s: &mut HashSet<Symbol>) {
    for st in stmts {
        walk_stmt_locals(st, s);
    }
}

fn walk_stmt_locals(st: &ast::Stmt, s: &mut HashSet<Symbol>) {
    match &st.kind {
        ast::StmtKind::Assign {
            target: ast::Target::Name(id),
            ..
        }
        | ast::StmtKind::AugAssign {
            target: ast::Target::Name(id),
            ..
        } => {
            s.insert(id.name);
        }
        ast::StmtKind::For { var, body, .. } => {
            s.insert(var.name);
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
    locals: &HashSet<Symbol>,
    calls: &mut Vec<Symbol>,
    imports_used: &mut Vec<Symbol>,
) {
    for st in stmts {
        scan_stmt_deps(st, locals, calls, imports_used);
    }
}

fn scan_stmt_deps(
    st: &ast::Stmt,
    locals: &HashSet<Symbol>,
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
    locals: &HashSet<Symbol>,
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
    locals: &HashSet<Symbol>,
    calls: &mut Vec<Symbol>,
    imports_used: &mut Vec<Symbol>,
) {
    walk_expr(e, &mut |ex| {
        if let ast::ExprKind::Call { callee, .. } = &ex.kind {
            if let ast::ExprKind::Name(id) = &callee.kind {
                if !locals.contains(&id.name) {
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

fn compute_order(ctx: &CheckCtx, call_edges: &HashMap<Symbol, Vec<Symbol>>) -> Vec<Vec<Symbol>> {
    let names: Vec<Symbol> = ctx.func_order.clone();
    let idx_of: HashMap<Symbol, usize> = names.iter().enumerate().map(|(i, &s)| (s, i)).collect();
    let n = names.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (name, callees) in call_edges {
        let Some(&fi) = idx_of.get(name) else {
            continue;
        };
        for c in callees {
            if let Some(&ci) = idx_of.get(c) {
                adj[fi].push(ci);
            }
        }
    }
    let (_, sccs) = tarjan(n, &adj);
    sccs.iter()
        .map(|comp| comp.iter().map(|&i| names[i]).collect())
        .collect()
}

fn tarjan(n: usize, adj: &[Vec<usize>]) -> (Vec<i64>, Vec<Vec<usize>>) {
    let mut index = vec![-1i64; n];
    let mut lowlink = vec![0i64; n];
    let mut on_stack = vec![false; n];
    let mut scc_id = vec![-1i64; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index: i64 = 0;
    let mut next_scc: i64 = 0;
    let mut sccs: Vec<Vec<usize>> = Vec::new();

    for start in 0..n {
        if index[start] != -1 {
            continue;
        }
        let mut work: Vec<(usize, usize)> = vec![(start, 0)];
        index[start] = next_index;
        lowlink[start] = next_index;
        next_index += 1;
        stack.push(start);
        on_stack[start] = true;

        while let Some(&mut (v, ref mut ci)) = work.last_mut() {
            if *ci < adj[v].len() {
                let w = adj[v][*ci];
                *ci += 1;
                if index[w] == -1 {
                    index[w] = next_index;
                    lowlink[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] && index[w] < lowlink[v] {
                    lowlink[v] = index[w];
                }
            } else {
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    if lowlink[v] < lowlink[parent] {
                        lowlink[parent] = lowlink[v];
                    }
                }
                if lowlink[v] == index[v] {
                    let mut comp = Vec::new();
                    loop {
                        let w = stack.pop().expect("pilha do Tarjan não pode esvaziar aqui");
                        on_stack[w] = false;
                        scc_id[w] = next_scc;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    sccs.push(comp);
                    next_scc += 1;
                }
            }
        }
    }
    (scc_id, sccs)
}
