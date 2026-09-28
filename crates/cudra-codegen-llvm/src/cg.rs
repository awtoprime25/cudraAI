use std::collections::HashMap;

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::intrinsics::Intrinsic;
use inkwell::module::{Linkage, Module as LlvmModule};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValue, BasicValueEnum, FloatValue, FunctionValue, IntValue,
    PointerValue, ValueKind,
};
use inkwell::{AddressSpace, FloatPredicate, IntPredicate};

use cudra_tir::{
    CmpOp, Func, FuncId, Intrinsic as TIntrinsic, LocalId, Module, TBinOp, TCmp, TExpr, TExprKind,
    TStmt, TUnOp, Ty,
};

use crate::Options;

/// Codigos de panico do runtime que o codegen tem de emitir (ver
/// `cudra_runtime::panics`).
const PANIC_INT_OVERFLOW: u32 = 1;
const PANIC_NEG_SHIFT: u32 = 3;
const PANIC_RANGE_STEP_ZERO: u32 = 4;
const PANIC_FLOAT_DIV_ZERO: u32 = 8;
const PANIC_INT_FLOORDIV_ZERO: u32 = 9;
const PANIC_INT_MOD_ZERO: u32 = 10;

type Val<'ctx> = Option<BasicValueEnum<'ctx>>;
type R<T> = Result<T, String>;

fn llvm_err<E: std::fmt::Display>(e: E) -> String {
    format!("erro interno do codegen (LLVM): {e}")
}

fn internal(msg: &str) -> String {
    format!("erro interno do codegen: {msg}")
}

/// `int` -> i64, `float` -> double, `bool` -> i1; `None` nao tem valor nem
/// armazenamento.
fn llvm_ty<'ctx>(ctx: &'ctx Context, ty: Ty) -> Option<BasicTypeEnum<'ctx>> {
    Some(match ty {
        Ty::Int => ctx.i64_type().into(),
        Ty::Float => ctx.f64_type().into(),
        Ty::Bool => ctx.bool_type().into(),
        Ty::NoneTy => return None,
    })
}

/// Simbolo LLVM de uma funcao do utilizador. Tudo o que nao seja alfanumerico
/// ASCII e escapado como `_xx` (um por byte), pelo que o mapa de nomes e
/// injetivo; o prefixo `cdr_` afasta-nos de `main`, da libc e do runtime, que
/// usa o prefixo `cudra_`.
fn symbol_name(m: &Module, f: &Func) -> String {
    if f.id == m.main {
        return "main".to_string();
    }
    let mut out = String::with_capacity(f.name.len() + 4);
    out.push_str("cdr_");
    for b in f.name.as_bytes() {
        match b {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' => out.push(*b as char),
            _ => out.push_str(&format!("_{b:02x}")),
        }
    }
    out
}

fn func_type<'ctx>(ctx: &'ctx Context, f: &Func, is_main: bool) -> FunctionType<'ctx> {
    if is_main {
        let i32t = ctx.i32_type();
        let argv = ctx.ptr_type(AddressSpace::default());
        return i32t.fn_type(&[i32t.into(), argv.into()], false);
    }
    let params: Vec<BasicMetadataTypeEnum> = f
        .params
        .iter()
        .filter_map(|p| llvm_ty(ctx, f.locals[p.0 as usize].ty).map(Into::into))
        .collect();
    match llvm_ty(ctx, f.ret) {
        Some(t) => t.fn_type(&params, false),
        None => ctx.void_type().fn_type(&params, false),
    }
}

/// Numero de `for` no corpo de uma funcao, em pre-ordem; cada um reserva um
/// contador escondido em u64 no bloco de entrada, para um ciclo dentro de
/// outro ciclo nao alocar pilha a cada iteracao.
fn count_for_loops(stmts: &[TStmt]) -> usize {
    let mut n = 0;
    for s in stmts {
        match s {
            TStmt::ForRange { body, .. } => n += 1 + count_for_loops(body),
            TStmt::While { body, .. } => n += count_for_loops(body),
            TStmt::If { then, orelse, .. } => n += count_for_loops(then) + count_for_loops(orelse),
            _ => {}
        }
    }
    n
}

pub(crate) fn emit_module<'ctx>(
    ctx: &'ctx Context,
    m: &Module,
    opts: &Options,
) -> Result<LlvmModule<'ctx>, String> {
    let module = ctx.create_module("cudra");
    let funcs: Vec<FunctionValue<'ctx>> = m
        .funcs
        .iter()
        .map(|f| {
            let name = symbol_name(m, f);
            let ty = func_type(ctx, f, f.id == m.main);
            let linkage = if f.id == m.main {
                None
            } else {
                Some(Linkage::Internal)
            };
            module.add_function(&name, ty, linkage)
        })
        .collect();
    let main = *funcs
        .get(m.main.0 as usize)
        .ok_or_else(|| internal("main fora dos limites"))?;
    let mut cg = Cg {
        ctx,
        module,
        builder: ctx.create_builder(),
        release: opts.release,
        funcs,
        decls: HashMap::new(),
        cur: main,
        slots: Vec::new(),
        panic_blocks: HashMap::new(),
        for_slots: Vec::new(),
        next_for_slot: 0,
        loops: Vec::new(),
        is_main: true,
    };
    for f in &m.funcs {
        cg.emit_func(m, f)?;
    }
    Ok(cg.module)
}

struct Cg<'ctx> {
    ctx: &'ctx Context,
    module: LlvmModule<'ctx>,
    builder: Builder<'ctx>,
    release: bool,
    funcs: Vec<FunctionValue<'ctx>>,
    decls: HashMap<String, FunctionValue<'ctx>>,
    cur: FunctionValue<'ctx>,
    slots: Vec<Option<PointerValue<'ctx>>>,
    /// Bloco de panico por codigo, partilhado por todas as verificacoes da
    /// funcao. Sem isto ha um bloco por verificacao (`cudra_panic(code)` e
    /// sempre o mesmo por codigo), e uma funcao de milhares de instrucoes
    /// passa a ter dezenas de milhares de blocos — o pipeline de maquina do
    /// LLVM (optimizador de CFG, alocador de registos) e caro nesse numero.
    panic_blocks: HashMap<u32, BasicBlock<'ctx>>,
    /// Contadores escondidos dos `for`, por ordem de emissao.
    for_slots: Vec<PointerValue<'ctx>>,
    next_for_slot: usize,
    loops: Vec<(BasicBlock<'ctx>, BasicBlock<'ctx>)>,
    is_main: bool,
}

impl<'ctx> Cg<'ctx> {
    // --- contexto corrente -------------------------------------------------

    fn i64(&self, v: i64) -> IntValue<'ctx> {
        self.ctx.i64_type().const_int(v as u64, true)
    }

    fn i1(&self, v: bool) -> IntValue<'ctx> {
        self.ctx.bool_type().const_int(v as u64, false)
    }

    fn block(&self) -> BasicBlock<'ctx> {
        self.builder
            .get_insert_block()
            .expect("o builder está sempre posicionado num bloco")
    }

    fn terminated(&self) -> bool {
        self.block().get_terminator().is_some()
    }

    fn new_block(&mut self, name: &str) -> BasicBlock<'ctx> {
        let f = self.cur;
        self.ctx.append_basic_block(f, name)
    }

    fn goto(&mut self, bb: BasicBlock<'ctx>) -> R<()> {
        self.builder
            .build_unconditional_branch(bb)
            .map_err(llvm_err)?;
        Ok(())
    }

    fn br_if(&mut self, cond: IntValue<'ctx>, t: BasicBlock<'ctx>, f: BasicBlock<'ctx>) -> R<()> {
        self.builder
            .build_conditional_branch(cond, t, f)
            .map_err(llvm_err)?;
        Ok(())
    }

    fn at(&mut self, bb: BasicBlock<'ctx>) {
        self.builder.position_at_end(bb);
    }

    // --- declaracoes -------------------------------------------------------

    fn rt_fn(&mut self, name: &'static str) -> R<FunctionValue<'ctx>> {
        if let Some(f) = self.decls.get(name) {
            return Ok(*f);
        }
        let ctx = self.ctx;
        let i8t = ctx.i8_type();
        let i32t = ctx.i32_type();
        let i64t = ctx.i64_type();
        let f64t = ctx.f64_type();
        let ptr = ctx.ptr_type(AddressSpace::default());
        let void = ctx.void_type();
        let ty = match name {
            "cudra_rt_init" => void.fn_type(&[i32t.into(), ptr.into()], false),
            "cudra_rt_exit" => i32t.fn_type(&[], false),
            "cudra_print_int" => void.fn_type(&[i64t.into()], false),
            "cudra_print_float" => void.fn_type(&[f64t.into()], false),
            "cudra_print_bool" => void.fn_type(&[i8t.into()], false),
            "cudra_print_none" | "cudra_print_sep" | "cudra_print_end" => void.fn_type(&[], false),
            "cudra_arg_int" => i64t.fn_type(&[i64t.into()], false),
            "cudra_arg_float" => f64t.fn_type(&[i64t.into()], false),
            "cudra_ipow" => i64t.fn_type(&[i64t.into(), i64t.into(), i8t.into()], false),
            "cudra_int_truediv" => f64t.fn_type(&[i64t.into(), i64t.into()], false),
            "cudra_float_floordiv" | "cudra_float_mod" | "cudra_fpow" => {
                f64t.fn_type(&[f64t.into(), f64t.into()], false)
            }
            "cudra_cmp_int_float" => i32t.fn_type(&[i64t.into(), f64t.into()], false),
            "cudra_float_to_int" => i64t.fn_type(&[f64t.into()], false),
            "cudra_panic" => void.fn_type(&[i32t.into()], false),
            _ => return Err(internal(&format!("função do runtime desconhecida: {name}"))),
        };
        let fv = self.module.add_function(name, ty, None);
        self.decls.insert(name.to_string(), fv);
        Ok(fv)
    }

    fn call_rt(
        &mut self,
        name: &'static str,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> R<Val<'ctx>> {
        let f = self.rt_fn(name)?;
        let call = self.builder.build_call(f, args, "").map_err(llvm_err)?;
        Ok(match call.try_as_basic_value() {
            ValueKind::Basic(v) => Some(v),
            ValueKind::Instruction(_) => None,
        })
    }

    fn intrinsic_fn(
        &mut self,
        name: &'static str,
        params: &[BasicTypeEnum<'ctx>],
    ) -> R<FunctionValue<'ctx>> {
        if let Some(f) = self.decls.get(name) {
            return Ok(*f);
        }
        let intrin = Intrinsic::find(name)
            .ok_or_else(|| internal(&format!("intrínseco LLVM desconhecido: {name}")))?;
        let fv = intrin
            .get_declaration(&self.module, params)
            .ok_or_else(|| internal(&format!("declaração do intrínseco {name} falhou")))?;
        self.decls.insert(name.to_string(), fv);
        Ok(fv)
    }

    fn overflow_op(
        &mut self,
        name: &'static str,
        a: IntValue<'ctx>,
        b: IntValue<'ctx>,
    ) -> R<(IntValue<'ctx>, IntValue<'ctx>)> {
        // o unico tipo de sobrecarga de llvm.{sadd,ssub,smul}.with.overflow e
        // o do elemento do resultado, nao os dois parametros
        let i64t = self.ctx.i64_type();
        let f = self.intrinsic_fn(name, &[i64t.into()])?;
        let call = self
            .builder
            .build_call(f, &[a.into(), b.into()], "ovf")
            .map_err(llvm_err)?;
        let sv = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_struct_value(),
            ValueKind::Instruction(_) => return Err(internal("intrínseco de overflow sem valor")),
        };
        let value = self
            .builder
            .build_extract_value(sv, 0, "ovf.v")
            .map_err(llvm_err)?
            .into_int_value();
        let flag = self
            .builder
            .build_extract_value(sv, 1, "ovf.f")
            .map_err(llvm_err)?
            .into_int_value();
        Ok((value, flag))
    }

    fn emit_panic(&mut self, code: u32) -> R<()> {
        let c = self.ctx.i32_type().const_int(code as u64, false);
        self.call_rt("cudra_panic", &[c.into()])?;
        self.builder.build_unreachable().map_err(llvm_err)?;
        Ok(())
    }

    /// Se `cond` for verdadeira, chama `cudra_panic(code)`; a execucao
    /// continua no bloco seguinte quando for falsa.
    fn guard(&mut self, cond: IntValue<'ctx>, code: u32) -> R<()> {
        let cont_bb = self.new_block("cont");
        let panic_bb = match self.panic_blocks.get(&code).copied() {
            Some(bb) => bb,
            None => {
                let bb = self.new_block("panic");
                let prev = self.block();
                self.at(bb);
                self.emit_panic(code)?;
                self.at(prev);
                self.panic_blocks.insert(code, bb);
                bb
            }
        };
        self.br_if(cond, panic_bb, cont_bb)?;
        self.at(cont_bb);
        Ok(())
    }

    // --- funcoes -----------------------------------------------------------

    fn emit_func(&mut self, m: &Module, f: &Func) -> R<()> {
        self.cur = self.funcs[f.id.0 as usize];
        self.is_main = f.id == m.main;
        self.slots = vec![None; f.locals.len()];
        self.panic_blocks.clear();
        self.for_slots.clear();
        self.next_for_slot = 0;
        self.loops.clear();

        let entry = self.new_block("entry");
        self.at(entry);
        for (i, l) in f.locals.iter().enumerate() {
            if let Some(t) = llvm_ty(self.ctx, l.ty) {
                let name = format!("l{i}");
                let slot = self.builder.build_alloca(t, &name).map_err(llvm_err)?;
                self.slots[i] = Some(slot);
            }
        }
        for i in 0..count_for_loops(&f.body) {
            let name = format!("k{i}");
            let slot = self
                .builder
                .build_alloca(self.ctx.i64_type(), &name)
                .map_err(llvm_err)?;
            self.for_slots.push(slot);
        }

        if self.is_main {
            let argc = self
                .cur
                .get_nth_param(0)
                .ok_or_else(|| internal("main sem argc"))?;
            let argv = self
                .cur
                .get_nth_param(1)
                .ok_or_else(|| internal("main sem argv"))?;
            let fv = self.rt_fn("cudra_rt_init")?;
            self.builder
                .build_call(fv, &[argc.into(), argv.into()], "")
                .map_err(llvm_err)?;
        } else {
            for (i, p) in f.params.iter().enumerate() {
                let arg = self
                    .cur
                    .get_nth_param(i as u32)
                    .ok_or_else(|| internal("parâmetro em falta"))?;
                if let Some(slot) = self.slots[p.0 as usize] {
                    self.builder.build_store(slot, arg).map_err(llvm_err)?;
                }
            }
        }

        self.emit_stmts(&f.body)?;
        if !self.terminated() {
            if self.is_main {
                let rc = self.call_rt("cudra_rt_exit", &[])?;
                let rc = rc.ok_or_else(|| internal("cudra_rt_exit sem valor"))?;
                self.builder.build_return(Some(&rc)).map_err(llvm_err)?;
            } else if f.ret == Ty::NoneTy {
                self.builder.build_return(None).map_err(llvm_err)?;
            } else {
                // a sema garante que este ponto nao e alcancavel
                self.builder.build_unreachable().map_err(llvm_err)?;
            }
        }
        Ok(())
    }

    // --- instrucoes --------------------------------------------------------

    fn emit_stmts(&mut self, stmts: &[TStmt]) -> R<()> {
        for s in stmts {
            if self.terminated() {
                let bb = self.new_block("dead");
                self.at(bb);
            }
            self.emit_stmt(s)?;
        }
        Ok(())
    }

    fn emit_stmt(&mut self, s: &TStmt) -> R<()> {
        match s {
            TStmt::Store { local, value } => {
                let v = self.expr(value)?;
                if let (Some(slot), Some(v)) = (self.slots[local.0 as usize], v) {
                    self.builder.build_store(slot, v).map_err(llvm_err)?;
                }
                Ok(())
            }
            TStmt::Expr(e) => {
                self.expr(e)?;
                Ok(())
            }
            TStmt::If { cond, then, orelse } => self.emit_if(cond, then, orelse),
            TStmt::While { cond, body } => self.emit_while(cond, body),
            TStmt::ForRange {
                var,
                start,
                stop,
                step,
                body,
            } => self.emit_for(*var, start, stop, step, body),
            TStmt::Return(opt) => self.emit_return(opt.as_ref()),
            TStmt::Break => {
                let (_, end) = self
                    .loops
                    .last()
                    .copied()
                    .ok_or_else(|| internal("break fora de ciclo"))?;
                self.goto(end)
            }
            TStmt::Continue => {
                let (cond, _) = self
                    .loops
                    .last()
                    .copied()
                    .ok_or_else(|| internal("continue fora de ciclo"))?;
                self.goto(cond)
            }
        }
    }

    fn emit_return(&mut self, value: Option<&TExpr>) -> R<()> {
        let v = match value {
            Some(e) => self.expr(e)?,
            None => None,
        };
        if self.is_main {
            // 'return' no topo e rejeitado pela sema; mesmo assim o flush do
            // stdout tem de acontecer antes de sair.
            let rc = self.call_rt("cudra_rt_exit", &[])?;
            let rc = rc.ok_or_else(|| internal("cudra_rt_exit sem valor"))?;
            self.builder.build_return(Some(&rc)).map_err(llvm_err)?;
            return Ok(());
        }
        match v {
            Some(v) => {
                self.builder.build_return(Some(&v)).map_err(llvm_err)?;
            }
            None => {
                self.builder.build_return(None).map_err(llvm_err)?;
            }
        }
        Ok(())
    }

    fn emit_if(&mut self, cond: &TExpr, then: &[TStmt], orelse: &[TStmt]) -> R<()> {
        let raw = self.expr(cond)?;
        let c = self.bool_of(raw)?;
        // sem `else` nao ha um ramo vazio a emitir: o proprio bloco de juncao
        // serve de destino do caminho falso (poupando um bloco por `if`).
        if orelse.is_empty() {
            let then_bb = self.new_block("if.then");
            let end_bb = self.new_block("if.end");
            self.br_if(c, then_bb, end_bb)?;
            self.at(then_bb);
            self.emit_stmts(then)?;
            if !self.terminated() {
                self.goto(end_bb)?;
            }
            self.at(end_bb);
            return Ok(());
        }
        let then_bb = self.new_block("if.then");
        let else_bb = self.new_block("if.else");
        let end_bb = self.new_block("if.end");
        self.br_if(c, then_bb, else_bb)?;
        self.at(then_bb);
        self.emit_stmts(then)?;
        if !self.terminated() {
            self.goto(end_bb)?;
        }
        self.at(else_bb);
        self.emit_stmts(orelse)?;
        if !self.terminated() {
            self.goto(end_bb)?;
        }
        self.at(end_bb);
        Ok(())
    }

    fn emit_while(&mut self, cond: &TExpr, body: &[TStmt]) -> R<()> {
        let cond_bb = self.new_block("while.cond");
        let body_bb = self.new_block("while.body");
        let end_bb = self.new_block("while.end");
        self.goto(cond_bb)?;
        self.at(cond_bb);
        let raw = self.expr(cond)?;
        let c = self.bool_of(raw)?;
        self.br_if(c, body_bb, end_bb)?;
        self.at(body_bb);
        self.loops.push((cond_bb, end_bb));
        self.emit_stmts(body)?;
        self.loops.pop();
        if !self.terminated() {
            self.goto(cond_bb)?;
        }
        self.at(end_bb);
        Ok(())
    }

    fn emit_for(
        &mut self,
        var: LocalId,
        start: &TExpr,
        stop: &TExpr,
        step: &TExpr,
        body: &[TStmt],
    ) -> R<()> {
        // regra 14: start, stop e step avaliados uma vez, por esta ordem
        let raw_start = self.expr(start)?;
        let start_v = self.int_of(raw_start)?;
        let raw_stop = self.expr(stop)?;
        let stop_v = self.int_of(raw_stop)?;
        let raw_step = self.expr(step)?;
        let step_v = self.int_of(raw_step)?;

        let zero = self.i64(0);
        let step_zero = self.icmp(IntPredicate::EQ, step_v, zero, "step.zero")?;
        self.guard(step_zero, PANIC_RANGE_STEP_ZERO)?;

        let count = self.range_count(start_v, stop_v, step_v)?;
        let slot = self
            .for_slots
            .get(self.next_for_slot)
            .copied()
            .ok_or_else(|| internal("contador de for em falta"))?;
        self.next_for_slot += 1;
        self.builder.build_store(slot, zero).map_err(llvm_err)?;

        let cond_bb = self.new_block("for.cond");
        let body_bb = self.new_block("for.body");
        let inc_bb = self.new_block("for.inc");
        let end_bb = self.new_block("for.end");
        self.goto(cond_bb)?;

        self.at(cond_bb);
        let k = self.load_int(slot)?;
        let more = self.icmp(IntPredicate::ULT, k, count, "for.more")?;
        self.br_if(more, body_bb, end_bb)?;

        self.at(body_bb);
        let prod = self
            .builder
            .build_int_mul(k, step_v, "for.prod")
            .map_err(llvm_err)?;
        let v = self
            .builder
            .build_int_add(start_v, prod, "for.v")
            .map_err(llvm_err)?;
        if let Some(var_slot) = self.slots[var.0 as usize] {
            self.builder.build_store(var_slot, v).map_err(llvm_err)?;
        }
        self.loops.push((inc_bb, end_bb));
        self.emit_stmts(body)?;
        self.loops.pop();
        if !self.terminated() {
            self.goto(inc_bb)?;
        }

        self.at(inc_bb);
        let k2 = self.load_int(slot)?;
        let one = self.i64(1);
        let k3 = self
            .builder
            .build_int_add(k2, one, "for.k")
            .map_err(llvm_err)?;
        self.builder.build_store(slot, k3).map_err(llvm_err)?;
        self.goto(cond_bb)?;

        self.at(end_bb);
        Ok(())
    }

    /// Numero de iteracoes em u64 (como i64 sem sinal), sem overflow: regra 14.
    fn range_count(
        &mut self,
        start: IntValue<'ctx>,
        stop: IntValue<'ctx>,
        step: IntValue<'ctx>,
    ) -> R<IntValue<'ctx>> {
        let zero = self.i64(0);
        let one = self.i64(1);

        let pos = self.icmp(IntPredicate::SGT, step, zero, "step.pos")?;
        let neg = self.icmp(IntPredicate::SLT, step, zero, "step.neg")?;
        let up = self.icmp(IntPredicate::SLT, start, stop, "range.up")?;
        let down = self.icmp(IntPredicate::SGT, start, stop, "range.down")?;
        let fwd = self
            .builder
            .build_and(pos, up, "range.fwd")
            .map_err(llvm_err)?;
        let bwd = self
            .builder
            .build_and(neg, down, "range.bwd")
            .map_err(llvm_err)?;

        // divisores sempre != 0 (`udiv` por zero e UB)
        let step_fwd = self.select(pos, step, one, "s.fwd")?;
        let abs_step = self
            .builder
            .build_int_neg(step, "s.abs")
            .map_err(llvm_err)?;
        let step_bwd = self.select(neg, abs_step, one, "s.bwd")?;

        let span_fwd = self
            .builder
            .build_int_sub(stop, start, "d.fwd")
            .map_err(llvm_err)?;
        let span_bwd = self
            .builder
            .build_int_sub(start, stop, "d.bwd")
            .map_err(llvm_err)?;
        let n_fwd = self.count_from(span_fwd, step_fwd)?;
        let n_bwd = self.count_from(span_bwd, step_bwd)?;

        let n = self.select(fwd, n_fwd, zero, "n.fwd")?;
        self.select(bwd, n_bwd, n, "n")
    }

    /// `(span - 1) / step + 1` em aritmetica sem sinal (`span > 0`, `step > 0`).
    fn count_from(&mut self, span: IntValue<'ctx>, step: IntValue<'ctx>) -> R<IntValue<'ctx>> {
        let one = self.i64(1);
        let d = self
            .builder
            .build_int_sub(span, one, "span-1")
            .map_err(llvm_err)?;
        let q = self
            .builder
            .build_int_unsigned_div(d, step, "span/step")
            .map_err(llvm_err)?;
        self.builder.build_int_add(q, one, "n+1").map_err(llvm_err)
    }

    // --- expressoes --------------------------------------------------------

    fn expr(&mut self, e: &TExpr) -> R<Val<'ctx>> {
        match &e.kind {
            TExprKind::ConstInt(v) => Ok(Some(self.i64(*v).into())),
            TExprKind::ConstFloat(v) => Ok(Some(self.ctx.f64_type().const_float(*v).into())),
            TExprKind::ConstBool(v) => Ok(Some(self.i1(*v).into())),
            TExprKind::ConstNone => Ok(None),
            TExprKind::Local(id) => match self.slots[id.0 as usize] {
                None => Ok(None),
                Some(slot) => {
                    let ty = llvm_ty(self.ctx, e.ty).ok_or_else(|| internal("local sem tipo"))?;
                    let v = self.builder.build_load(ty, slot, "ld").map_err(llvm_err)?;
                    Ok(Some(v))
                }
            },
            TExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs),
            TExprKind::Unary { op, operand } => self.unary(*op, operand),
            TExprKind::Compare { first, rest } => Ok(Some(self.compare(first, rest)?.into())),
            TExprKind::And(a, b) => Ok(Some(self.logical(a, b, false)?.into())),
            TExprKind::Or(a, b) => Ok(Some(self.logical(a, b, true)?.into())),
            TExprKind::Cast { value } => self.cast(e.ty, value),
            TExprKind::Call { func, args } => self.call_user(*func, args),
            TExprKind::Intrinsic { which, args } => self.intrinsic(*which, args),
        }
    }

    fn int_of(&self, v: Val<'ctx>) -> R<IntValue<'ctx>> {
        match v {
            Some(BasicValueEnum::IntValue(i)) if i.get_type().get_bit_width() == 64 => Ok(i),
            _ => Err(internal("esperava-se um valor inteiro")),
        }
    }

    fn float_of(&self, v: Val<'ctx>) -> R<FloatValue<'ctx>> {
        match v {
            Some(BasicValueEnum::FloatValue(f)) => Ok(f),
            _ => Err(internal("esperava-se um valor de vírgula flutuante")),
        }
    }

    fn bool_of(&self, v: Val<'ctx>) -> R<IntValue<'ctx>> {
        match v {
            Some(BasicValueEnum::IntValue(i)) if i.get_type().get_bit_width() == 1 => Ok(i),
            _ => Err(internal("esperava-se um valor booleano")),
        }
    }

    fn load_int(&mut self, slot: PointerValue<'ctx>) -> R<IntValue<'ctx>> {
        let v = self
            .builder
            .build_load(self.ctx.i64_type(), slot, "ld.k")
            .map_err(llvm_err)?;
        Ok(v.into_int_value())
    }

    fn select(
        &mut self,
        cond: IntValue<'ctx>,
        t: IntValue<'ctx>,
        f: IntValue<'ctx>,
        name: &str,
    ) -> R<IntValue<'ctx>> {
        self.builder
            .build_select(cond, t, f, name)
            .map(BasicValueEnum::into_int_value)
            .map_err(llvm_err)
    }

    fn icmp(
        &mut self,
        op: IntPredicate,
        a: IntValue<'ctx>,
        b: IntValue<'ctx>,
        n: &str,
    ) -> R<IntValue<'ctx>> {
        self.builder
            .build_int_compare(op, a, b, n)
            .map_err(llvm_err)
    }

    fn fcmp(
        &mut self,
        op: FloatPredicate,
        a: FloatValue<'ctx>,
        b: FloatValue<'ctx>,
        n: &str,
    ) -> R<IntValue<'ctx>> {
        self.builder
            .build_float_compare(op, a, b, n)
            .map_err(llvm_err)
    }

    fn binary(&mut self, op: TBinOp, lhs: &TExpr, rhs: &TExpr) -> R<Val<'ctx>> {
        let l = self.expr(lhs)?;
        let r = self.expr(rhs)?;
        use TBinOp::*;
        match op {
            IAdd | ISub | IMul => {
                let a = self.int_of(l)?;
                let b = self.int_of(r)?;
                let v = if self.release {
                    match op {
                        IAdd => self.builder.build_int_add(a, b, "add"),
                        ISub => self.builder.build_int_sub(a, b, "sub"),
                        _ => self.builder.build_int_mul(a, b, "mul"),
                    }
                    .map_err(llvm_err)?
                } else {
                    let name = match op {
                        IAdd => "llvm.sadd.with.overflow",
                        ISub => "llvm.ssub.with.overflow",
                        _ => "llvm.smul.with.overflow",
                    };
                    let (value, of) = self.overflow_op(name, a, b)?;
                    self.guard(of, PANIC_INT_OVERFLOW)?;
                    value
                };
                Ok(Some(v.into()))
            }
            ITrueDiv => {
                let a = self.int_of(l)?;
                let b = self.int_of(r)?;
                self.call_rt("cudra_int_truediv", &[a.into(), b.into()])
            }
            IFloorDiv | IMod => {
                let a = self.int_of(l)?;
                let b = self.int_of(r)?;
                let v = self.floor_divmod(a, b, op == IMod)?;
                Ok(Some(v.into()))
            }
            IPow => {
                let a = self.int_of(l)?;
                let b = self.int_of(r)?;
                let checked = self
                    .ctx
                    .i8_type()
                    .const_int(u64::from(!self.release), false);
                self.call_rt("cudra_ipow", &[a.into(), b.into(), checked.into()])
            }
            IAnd | IOr | IXor => {
                let a = self.int_of(l)?;
                let b = self.int_of(r)?;
                let v = match op {
                    IAnd => self.builder.build_and(a, b, "and"),
                    IOr => self.builder.build_or(a, b, "or"),
                    _ => self.builder.build_xor(a, b, "xor"),
                }
                .map_err(llvm_err)?;
                Ok(Some(v.into()))
            }
            IShl => {
                let a = self.int_of(l)?;
                let b = self.int_of(r)?;
                let v = self.shift_left(a, b)?;
                Ok(Some(v.into()))
            }
            IShr => {
                let a = self.int_of(l)?;
                let b = self.int_of(r)?;
                let v = self.shift_right(a, b)?;
                Ok(Some(v.into()))
            }
            FAdd | FSub | FMul => {
                let a = self.float_of(l)?;
                let b = self.float_of(r)?;
                let v = match op {
                    FAdd => self.builder.build_float_add(a, b, "fadd"),
                    FSub => self.builder.build_float_sub(a, b, "fsub"),
                    _ => self.builder.build_float_mul(a, b, "fmul"),
                }
                .map_err(llvm_err)?;
                Ok(Some(v.into()))
            }
            FDiv => {
                let a = self.float_of(l)?;
                let b = self.float_of(r)?;
                let zero = self.ctx.f64_type().const_float(0.0);
                let is_zero = self.fcmp(FloatPredicate::OEQ, b, zero, "fdiv.zero")?;
                self.guard(is_zero, PANIC_FLOAT_DIV_ZERO)?;
                let v = self
                    .builder
                    .build_float_div(a, b, "fdiv")
                    .map_err(llvm_err)?;
                Ok(Some(v.into()))
            }
            FFloorDiv => {
                let a = self.float_of(l)?;
                let b = self.float_of(r)?;
                self.call_rt("cudra_float_floordiv", &[a.into(), b.into()])
            }
            FMod => {
                let a = self.float_of(l)?;
                let b = self.float_of(r)?;
                self.call_rt("cudra_float_mod", &[a.into(), b.into()])
            }
            FPow => {
                let a = self.float_of(l)?;
                let b = self.float_of(r)?;
                self.call_rt("cudra_fpow", &[a.into(), b.into()])
            }
        }
    }

    /// `//` e `%` com semantica de floor. O divisor -1 e substituido por 1 (com
    /// o dividendo negado), o que evita o `sdiv`/`srem` com (MIN, -1) — que no
    /// x86 da SIGFPE — sem precisar de ramos para escolher o resultado.
    fn floor_divmod(
        &mut self,
        a: IntValue<'ctx>,
        b: IntValue<'ctx>,
        want_mod: bool,
    ) -> R<IntValue<'ctx>> {
        let zero = self.i64(0);
        let one = self.i64(1);
        let m1 = self.i64(-1);

        let b_zero = self.icmp(IntPredicate::EQ, b, zero, "b.zero")?;
        let code = if want_mod {
            PANIC_INT_MOD_ZERO
        } else {
            PANIC_INT_FLOORDIV_ZERO
        };
        self.guard(b_zero, code)?;

        let is_m1 = self.icmp(IntPredicate::EQ, b, m1, "b.m1")?;
        let neg_a = self.builder.build_int_neg(a, "neg.a").map_err(llvm_err)?;
        if !self.release && !want_mod {
            let (_, of) = self.overflow_op("llvm.ssub.with.overflow", zero, a)?;
            let ovf = self
                .builder
                .build_and(is_m1, of, "m1.ovf")
                .map_err(llvm_err)?;
            self.guard(ovf, PANIC_INT_OVERFLOW)?;
        }
        let a1 = self.select(is_m1, neg_a, a, "a1")?;
        let b1 = self.select(is_m1, one, b, "b1")?;

        let q = self
            .builder
            .build_int_signed_div(a1, b1, "sdiv")
            .map_err(llvm_err)?;
        let r = self
            .builder
            .build_int_signed_rem(a1, b1, "srem")
            .map_err(llvm_err)?;

        let r_ne = self.icmp(IntPredicate::NE, r, zero, "r.ne")?;
        let mixed = self.builder.build_xor(r, b1, "r^b").map_err(llvm_err)?;
        let neg_mixed = self.icmp(IntPredicate::SLT, mixed, zero, "mixed")?;
        let adjust = self
            .builder
            .build_and(r_ne, neg_mixed, "adjust")
            .map_err(llvm_err)?;

        if want_mod {
            let r_plus = self.builder.build_int_add(r, b1, "r+b").map_err(llvm_err)?;
            self.select(adjust, r_plus, r, "mod")
        } else {
            let q_minus = self
                .builder
                .build_int_sub(q, one, "q-1")
                .map_err(llvm_err)?;
            self.select(adjust, q_minus, q, "floordiv")
        }
    }

    /// `<<` (regra 8): contagem negativa -> panic 3; em debug o resultado tem
    /// de caber em i64; em release, `n >= 64` da 0 (para qualquer `x`).
    fn shift_left(&mut self, x: IntValue<'ctx>, n: IntValue<'ctx>) -> R<IntValue<'ctx>> {
        let zero = self.i64(0);
        let neg = self.icmp(IntPredicate::SLT, n, zero, "sh.neg")?;
        self.guard(neg, PANIC_NEG_SHIFT)?;

        let sixty4 = self.i64(64);
        let wide = self.icmp(IntPredicate::SGE, n, sixty4, "sh.wide")?;
        let sixty3 = self.i64(63);
        let shift = self.select(wide, sixty3, n, "sh.amt")?;
        let shifted = self
            .builder
            .build_left_shift(x, shift, "shl")
            .map_err(llvm_err)?;

        if self.release {
            return self.select(wide, zero, shifted, "shl.rel");
        }
        let back = self
            .builder
            .build_right_shift(shifted, shift, true, "shl.back")
            .map_err(llvm_err)?;
        let wraps = self.icmp(IntPredicate::NE, back, x, "shl.wraps")?;
        let x_ne = self.icmp(IntPredicate::NE, x, zero, "shl.xne")?;
        let huge = self
            .builder
            .build_and(wide, x_ne, "shl.huge")
            .map_err(llvm_err)?;
        let ovf = self
            .builder
            .build_or(wraps, huge, "shl.ovf")
            .map_err(llvm_err)?;
        self.guard(ovf, PANIC_INT_OVERFLOW)?;
        Ok(shifted)
    }

    /// `>>` (regra 8): contagem negativa -> panic 3; `n >= 64` da 0 ou -1
    /// conforme o sinal, nos dois modos.
    fn shift_right(&mut self, x: IntValue<'ctx>, n: IntValue<'ctx>) -> R<IntValue<'ctx>> {
        let zero = self.i64(0);
        let neg = self.icmp(IntPredicate::SLT, n, zero, "shr.neg")?;
        self.guard(neg, PANIC_NEG_SHIFT)?;

        let sixty4 = self.i64(64);
        let wide = self.icmp(IntPredicate::SGE, n, sixty4, "shr.wide")?;
        let sixty3 = self.i64(63);
        let shift = self.select(wide, sixty3, n, "shr.amt")?;
        let shifted = self
            .builder
            .build_right_shift(x, shift, true, "ashr")
            .map_err(llvm_err)?;
        let x_neg = self.icmp(IntPredicate::SLT, x, zero, "shr.xneg")?;
        let sat = self.select(x_neg, self.i64(-1), zero, "shr.sat")?;
        self.select(wide, sat, shifted, "shr")
    }

    fn unary(&mut self, op: TUnOp, operand: &TExpr) -> R<Val<'ctx>> {
        let v = self.expr(operand)?;
        Ok(match op {
            TUnOp::Not => {
                let b = self.bool_of(v)?;
                Some(self.builder.build_not(b, "not").map_err(llvm_err)?.into())
            }
            TUnOp::IInvert => {
                let x = self.int_of(v)?;
                Some(
                    self.builder
                        .build_not(x, "invert")
                        .map_err(llvm_err)?
                        .into(),
                )
            }
            TUnOp::FNeg => {
                let f = self.float_of(v)?;
                Some(
                    self.builder
                        .build_float_neg(f, "fneg")
                        .map_err(llvm_err)?
                        .into(),
                )
            }
            TUnOp::INeg => {
                let a = self.int_of(v)?;
                let x = if self.release {
                    self.builder.build_int_neg(a, "neg").map_err(llvm_err)?
                } else {
                    let zero = self.i64(0);
                    let (value, of) = self.overflow_op("llvm.ssub.with.overflow", zero, a)?;
                    self.guard(of, PANIC_INT_OVERFLOW)?;
                    value
                };
                Some(x.into())
            }
        })
    }

    /// `and`/`or` com curto-circuito (ramos + phi: os operandos podem ter
    /// efeitos).
    fn logical(&mut self, a: &TExpr, b: &TExpr, is_or: bool) -> R<IntValue<'ctx>> {
        let raw_a = self.expr(a)?;
        let av = self.bool_of(raw_a)?;
        let first_bb = self.block();
        let rhs_bb = self.new_block(if is_or { "or.rhs" } else { "and.rhs" });
        let end_bb = self.new_block(if is_or { "or.end" } else { "and.end" });
        if is_or {
            self.br_if(av, end_bb, rhs_bb)?;
        } else {
            self.br_if(av, rhs_bb, end_bb)?;
        }
        self.at(rhs_bb);
        let raw_b = self.expr(b)?;
        let bv = self.bool_of(raw_b)?;
        // avaliar `b` pode ter criado blocos (comparacoes em cadeia): quem
        // ramifica para `end_bb` e o bloco onde a expressao acabou, nao o
        // bloco onde comecou.
        let b_bb = self.block();
        self.goto(end_bb)?;
        self.at(end_bb);
        let short = self.i1(is_or);
        let phi = self
            .builder
            .build_phi(self.ctx.bool_type(), "logic")
            .map_err(llvm_err)?;
        phi.add_incoming(&[(&short, first_bb), (&bv, b_bb)]);
        Ok(phi.as_basic_value().into_int_value())
    }

    /// Comparacao em cadeia: cada operando avaliado uma vez, da esquerda para
    /// a direita, com curto-circuito.
    fn compare(&mut self, first: &TExpr, rest: &[(TCmp, TExpr)]) -> R<IntValue<'ctx>> {
        let end_bb = self.new_block("cmp.end");
        let false_v = self.i1(false);
        let true_v = self.i1(true);
        let mut incoming: Vec<(BasicValueEnum<'ctx>, BasicBlock<'ctx>)> = Vec::new();

        let mut prev = self.expr(first)?;
        for (cmp, rhs) in rest {
            let rhs_v = self.expr(rhs)?;
            let cond = self.build_cmp(*cmp, &prev, &rhs_v)?;
            let false_bb = self.block();
            let next_bb = self.new_block("cmp.next");
            self.br_if(cond, next_bb, end_bb)?;
            incoming.push((false_v.into(), false_bb));
            self.at(next_bb);
            prev = rhs_v;
        }
        let last_bb = self.block();
        self.goto(end_bb)?;
        incoming.push((true_v.into(), last_bb));
        self.at(end_bb);
        let phi = self
            .builder
            .build_phi(self.ctx.bool_type(), "cmp")
            .map_err(llvm_err)?;
        let refs: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
            .iter()
            .map(|(v, bb)| (v as &dyn BasicValue<'ctx>, *bb))
            .collect();
        phi.add_incoming(&refs);
        Ok(phi.as_basic_value().into_int_value())
    }

    fn build_cmp(&mut self, cmp: TCmp, l: &Val<'ctx>, r: &Val<'ctx>) -> R<IntValue<'ctx>> {
        match cmp {
            TCmp::I(op) => {
                let a = self.int_of(*l)?;
                let b = self.int_of(*r)?;
                self.icmp(int_pred(op), a, b, "icmp")
            }
            TCmp::F(op) => {
                let a = self.float_of(*l)?;
                let b = self.float_of(*r)?;
                self.fcmp(float_pred(op), a, b, "fcmp")
            }
            TCmp::B(op) => {
                let a = self.bool_of(*l)?;
                let b = self.bool_of(*r)?;
                let pred = if op == CmpOp::Eq {
                    IntPredicate::EQ
                } else {
                    IntPredicate::NE
                };
                self.icmp(pred, a, b, "bcmp")
            }
            TCmp::IntFloat(op) => {
                let i = self.int_of(*l)?;
                let f = self.float_of(*r)?;
                self.cmp_int_float(i, f, op, false)
            }
            TCmp::FloatInt(op) => {
                let i = self.int_of(*r)?;
                let f = self.float_of(*l)?;
                self.cmp_int_float(i, f, op, true)
            }
        }
    }

    /// `cudra_cmp_int_float` da -1/0/1, ou 2 quando o float e nan.
    fn cmp_int_float(
        &mut self,
        i: IntValue<'ctx>,
        f: FloatValue<'ctx>,
        op: CmpOp,
        swapped: bool,
    ) -> R<IntValue<'ctx>> {
        let r = self
            .call_rt("cudra_cmp_int_float", &[i.into(), f.into()])?
            .ok_or_else(|| internal("cudra_cmp_int_float sem valor"))?
            .into_int_value();
        let op = if swapped { mirror(op) } else { op };
        let i32t = self.ctx.i32_type();
        let zero = i32t.const_int(0, false);
        let one = i32t.const_int(1, false);
        let minus_one = i32t.const_int((-1i64) as u64, true);
        match op {
            CmpOp::Eq => self.icmp(IntPredicate::EQ, r, zero, "cmp.eq"),
            CmpOp::Ne => self.icmp(IntPredicate::NE, r, zero, "cmp.ne"),
            CmpOp::Lt => self.icmp(IntPredicate::EQ, r, minus_one, "cmp.lt"),
            CmpOp::Gt => self.icmp(IntPredicate::EQ, r, one, "cmp.gt"),
            CmpOp::Le => {
                let a = self.icmp(IntPredicate::EQ, r, minus_one, "cmp.le1")?;
                let b = self.icmp(IntPredicate::EQ, r, zero, "cmp.le2")?;
                self.builder.build_or(a, b, "cmp.le").map_err(llvm_err)
            }
            CmpOp::Ge => {
                let a = self.icmp(IntPredicate::EQ, r, zero, "cmp.ge1")?;
                let b = self.icmp(IntPredicate::EQ, r, one, "cmp.ge2")?;
                self.builder.build_or(a, b, "cmp.ge").map_err(llvm_err)
            }
        }
    }

    fn cast(&mut self, to: Ty, value: &TExpr) -> R<Val<'ctx>> {
        let v = self.expr(value)?;
        Ok(match (value.ty, to) {
            (Ty::Int, Ty::Float) => {
                let x = self.int_of(v)?;
                Some(
                    self.builder
                        .build_signed_int_to_float(x, self.ctx.f64_type(), "sitofp")
                        .map_err(llvm_err)?
                        .into(),
                )
            }
            (Ty::Float, Ty::Int) => {
                let f = self.float_of(v)?;
                self.call_rt("cudra_float_to_int", &[f.into()])?
            }
            (Ty::Bool, Ty::Int) => {
                let b = self.bool_of(v)?;
                Some(
                    self.builder
                        .build_int_z_extend(b, self.ctx.i64_type(), "zext")
                        .map_err(llvm_err)?
                        .into(),
                )
            }
            (Ty::Bool, Ty::Float) => {
                let b = self.bool_of(v)?;
                Some(
                    self.builder
                        .build_unsigned_int_to_float(b, self.ctx.f64_type(), "uitofp")
                        .map_err(llvm_err)?
                        .into(),
                )
            }
            (Ty::Int, Ty::Bool) => {
                let x = self.int_of(v)?;
                let zero = self.i64(0);
                Some(self.icmp(IntPredicate::NE, x, zero, "int2bool")?.into())
            }
            (Ty::Float, Ty::Bool) => {
                let f = self.float_of(v)?;
                let zero = self.ctx.f64_type().const_float(0.0);
                Some(
                    self.fcmp(FloatPredicate::UNE, f, zero, "float2bool")?
                        .into(),
                )
            }
            _ => return Err(internal("cast inválido na TIR")),
        })
    }

    fn call_user(&mut self, func: FuncId, args: &[TExpr]) -> R<Val<'ctx>> {
        let fv = *self
            .funcs
            .get(func.0 as usize)
            .ok_or_else(|| internal("FuncId fora dos limites"))?;
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.expr(a)?);
        }
        let mut mvals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::with_capacity(vals.len());
        for v in vals.iter().flatten() {
            mvals.push((*v).into());
        }
        let call = self
            .builder
            .build_call(fv, &mvals, "call")
            .map_err(llvm_err)?;
        Ok(match call.try_as_basic_value() {
            ValueKind::Basic(v) => Some(v),
            ValueKind::Instruction(_) => None,
        })
    }

    fn intrinsic(&mut self, which: TIntrinsic, args: &[TExpr]) -> R<Val<'ctx>> {
        match which {
            TIntrinsic::Print => {
                // todos os argumentos avaliados primeiro, como no Python
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push((a.ty, self.expr(a)?));
                }
                for (i, (ty, v)) in vals.into_iter().enumerate() {
                    if i > 0 {
                        self.call_rt("cudra_print_sep", &[])?;
                    }
                    match ty {
                        Ty::Int => {
                            let x = self.int_of(v)?;
                            self.call_rt("cudra_print_int", &[x.into()])?;
                        }
                        Ty::Float => {
                            let f = self.float_of(v)?;
                            self.call_rt("cudra_print_float", &[f.into()])?;
                        }
                        Ty::Bool => {
                            let b = self.bool_of(v)?;
                            let z = self
                                .builder
                                .build_int_z_extend(b, self.ctx.i8_type(), "b2i8")
                                .map_err(llvm_err)?;
                            self.call_rt("cudra_print_bool", &[z.into()])?;
                        }
                        Ty::NoneTy => {
                            self.call_rt("cudra_print_none", &[])?;
                        }
                    }
                }
                self.call_rt("cudra_print_end", &[])?;
                Ok(None)
            }
            TIntrinsic::AbsInt => {
                let raw = self.expr(&args[0])?;
                let x = self.int_of(raw)?;
                let zero = self.i64(0);
                let neg = if self.release {
                    self.builder.build_int_neg(x, "abs.neg").map_err(llvm_err)?
                } else {
                    let (value, of) = self.overflow_op("llvm.ssub.with.overflow", zero, x)?;
                    self.guard(of, PANIC_INT_OVERFLOW)?;
                    value
                };
                let is_neg = self.icmp(IntPredicate::SLT, x, zero, "abs.neg?")?;
                Ok(Some(self.select(is_neg, neg, x, "abs")?.into()))
            }
            TIntrinsic::AbsFloat => {
                let raw = self.expr(&args[0])?;
                let f = self.float_of(raw)?;
                let f64t = self.ctx.f64_type();
                let fv = self.intrinsic_fn("llvm.fabs", &[f64t.into()])?;
                let call = self
                    .builder
                    .build_call(fv, &[f.into()], "fabs")
                    .map_err(llvm_err)?;
                Ok(match call.try_as_basic_value() {
                    ValueKind::Basic(v) => Some(v),
                    ValueKind::Instruction(_) => None,
                })
            }
            TIntrinsic::ArgInt => {
                let raw = self.expr(&args[0])?;
                let i = self.int_of(raw)?;
                self.call_rt("cudra_arg_int", &[i.into()])
            }
            TIntrinsic::ArgFloat => {
                let raw = self.expr(&args[0])?;
                let i = self.int_of(raw)?;
                self.call_rt("cudra_arg_float", &[i.into()])
            }
        }
    }
}

fn int_pred(op: CmpOp) -> IntPredicate {
    match op {
        CmpOp::Eq => IntPredicate::EQ,
        CmpOp::Ne => IntPredicate::NE,
        CmpOp::Lt => IntPredicate::SLT,
        CmpOp::Le => IntPredicate::SLE,
        CmpOp::Gt => IntPredicate::SGT,
        CmpOp::Ge => IntPredicate::SGE,
    }
}

fn float_pred(op: CmpOp) -> FloatPredicate {
    match op {
        CmpOp::Eq => FloatPredicate::OEQ,
        CmpOp::Ne => FloatPredicate::UNE,
        CmpOp::Lt => FloatPredicate::OLT,
        CmpOp::Le => FloatPredicate::OLE,
        CmpOp::Gt => FloatPredicate::OGT,
        CmpOp::Ge => FloatPredicate::OGE,
    }
}

/// Espelho do operador para a comparacao `float <op> int` (o helper compara
/// `int` com `float`).
fn mirror(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Ge => CmpOp::Le,
        other => other,
    }
}
