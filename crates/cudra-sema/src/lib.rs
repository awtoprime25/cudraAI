use std::collections::HashMap;

use cudra_ast as ast;
use cudra_diagnostics::Diagnostic;
use cudra_span::{Interner, Symbol};

mod definite;
mod infer;
mod lower;
mod resolve;

/// Tipo semântico usado durante a verificação. `Error` absorve-se em
/// cascata: uma vez emitido um diagnóstico para uma subexpressão, o resto da
/// árvore que a envolve não gera mais erros por causa dela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum STy {
    Int,
    Float,
    Bool,
    NoneTy,
    Error,
}

impl STy {
    fn to_tir(self) -> cudra_tir::Ty {
        match self {
            STy::Int => cudra_tir::Ty::Int,
            STy::Float => cudra_tir::Ty::Float,
            STy::Bool => cudra_tir::Ty::Bool,
            STy::NoneTy | STy::Error => cudra_tir::Ty::NoneTy,
        }
    }

    fn name(self) -> &'static str {
        match self {
            STy::Int => "int",
            STy::Float => "float",
            STy::Bool => "bool",
            STy::NoneTy => "None",
            STy::Error => "<erro>",
        }
    }
}

pub(crate) struct FuncEntry {
    pub id: cudra_tir::FuncId,
    pub params: Vec<(Symbol, STy)>,
    pub ret: STy,
    pub ret_annotated: bool,
    pub def_pos: usize,
    pub span: cudra_span::Span,
    pub name_span: cudra_span::Span,
    /// posição a partir da qual a função (e tudo o que chama,
    /// transitivamente) está pronta; usada só para chamadas no topo.
    pub ready_pos: usize,
    pub locals_hint: std::collections::HashSet<Symbol>,
}

pub(crate) struct Imports {
    /// "arg_int"/"arg_float" -> posição do import mais cedo.
    pub names: HashMap<Symbol, usize>,
    /// posição do `import cudra` mais cedo, se algum.
    pub cudra_module_pos: Option<usize>,
}

pub(crate) struct CheckCtx<'a> {
    pub interner: &'a Interner,
    pub functions: HashMap<Symbol, FuncEntry>,
    pub func_order: Vec<Symbol>,
    pub imports: Imports,
    pub top_vars: std::collections::HashSet<Symbol>,
    pub diags: Vec<Diagnostic>,
}

impl<'a> CheckCtx<'a> {
    fn err(&mut self, d: Diagnostic) {
        self.diags.push(d);
    }

    fn sym(&self, s: Symbol) -> &str {
        self.interner.resolve(s)
    }
}

pub fn check(
    module: &ast::Module,
    interner: &Interner,
) -> Result<cudra_tir::Module, Vec<Diagnostic>> {
    let mut ctx = CheckCtx {
        interner,
        functions: HashMap::new(),
        func_order: Vec::new(),
        imports: Imports {
            names: HashMap::new(),
            cudra_module_pos: None,
        },
        top_vars: std::collections::HashSet::new(),
        diags: Vec::new(),
    };

    let plan = resolve::resolve(&mut ctx, module);
    let tir_module = lower::lower_module(&mut ctx, module, &plan);

    if ctx.diags.is_empty() {
        match tir_module {
            Some(m) => Ok(m),
            None => Ok(empty_module()),
        }
    } else {
        ctx.diags.sort_by_key(|d| {
            d.labels
                .iter()
                .find(|l| l.primary)
                .or_else(|| d.labels.first())
                .map(|l| (l.span.file.0, l.span.lo))
                .unwrap_or((0, 0))
        });
        Err(ctx.diags)
    }
}

fn empty_module() -> cudra_tir::Module {
    cudra_tir::Module {
        funcs: vec![cudra_tir::Func {
            id: cudra_tir::FuncId(0),
            name: "main".to_string(),
            params: vec![],
            locals: vec![],
            ret: cudra_tir::Ty::NoneTy,
            body: vec![],
        }],
        main: cudra_tir::FuncId(0),
    }
}
