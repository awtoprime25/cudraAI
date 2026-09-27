//! Codegen LLVM da TIR: um modulo LLVM por programa, com o triple do host,
//! ligado ao runtime do cudraAI pela ABI C. As garantias que este codegen
//! pode assumir estao em `cudra_tir::verify` — a TIR que chega aqui ja foi
//! verificada estruturalmente.

use std::sync::OnceLock;

use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;

use cudra_tir::Module;

mod cg;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// `true` = build `--release`: passa o pipeline `default<O3>` e desliga as
    /// verificacoes de overflow (o wrapping passa a ser definido).
    pub release: bool,
}

/// IR textual do programa (ja otimizada quando `opts.release`).
pub fn emit_llvm_ir(m: &Module, opts: &Options) -> Result<String, String> {
    let ctx = Context::create();
    let (module, _tm) = compile(&ctx, m, opts)?;
    Ok(module.print_to_string().to_string())
}

/// Codigo objeto do programa (relocavel, PIC).
pub fn emit_object(m: &Module, opts: &Options) -> Result<Vec<u8>, String> {
    let ctx = Context::create();
    let (module, tm) = compile(&ctx, m, opts)?;
    let buffer = tm
        .write_to_memory_buffer(&module, FileType::Object)
        .map_err(|e| format!("erro ao gerar o objeto: {e}"))?;
    Ok(buffer.as_slice().to_vec())
}

fn compile<'ctx>(
    ctx: &'ctx Context,
    m: &Module,
    opts: &Options,
) -> Result<(LlvmModule<'ctx>, TargetMachine), String> {
    let tm = target_machine(opts.release)?;
    let module = cg::emit_module(ctx, m, opts)?;
    let triple = TargetMachine::get_default_triple();
    module.set_triple(&triple);
    module.set_data_layout(&tm.get_target_data().get_data_layout());
    if let Err(e) = module.verify() {
        return Err(format!("{e}\n\n{}", module.print_to_string()));
    }
    if opts.release {
        module
            .run_passes("default<O3>", &tm, PassBuilderOptions::create())
            .map_err(|e| format!("erro nas otimizações: {e}"))?;
    }
    Ok((module, tm))
}

fn init_targets() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let _ = Target::initialize_native(&InitializationConfig::default());
    });
}

fn target_machine(release: bool) -> Result<TargetMachine, String> {
    init_targets();
    let triple = TargetMachine::get_default_triple();
    let target =
        Target::from_triple(&triple).map_err(|e| format!("alvo nativo indisponível: {e}"))?;
    let host_features = TargetMachine::get_host_cpu_features();
    let features = host_features.to_string_lossy();
    // PIC porque o `cc` do Ubuntu liga PIE por omissão.
    target
        .create_target_machine(
            &triple,
            &TargetMachine::get_host_cpu_name().to_string_lossy(),
            &features,
            if release {
                OptimizationLevel::Aggressive
            } else {
                OptimizationLevel::None
            },
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| "não foi possível criar a máquina de destino".to_string())
}
