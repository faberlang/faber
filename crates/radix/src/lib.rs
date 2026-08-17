//! Thin complete compiler façade over [`radix_module`] and [`radix_program`].
//!
//! Library-only: no binary, no clap, no process exit. The developer `radix`
//! binary and `tool` stay in `radix-module` until RTR4.
//!
//! Admitted compiler entries are re-exported **by name** under [`module`],
//! [`program`], and [`services`]. This crate does not re-export `tool`.

/// Fail-closed module analysis.
pub mod module {
    pub use radix_module::driver::{
        analyze_source, analyze_source_with_module_import_contract, AnalyzedModule, Config,
        ModuleImportContract, Session,
    };
}

/// Typed program-layer compilation and prepared-module analysis.
///
/// `prepare_*` / `analyze_prepared_program_*` are re-exported from
/// `radix_module::tool` until RTR4 moves them to `radix-program`.
pub mod program {
    pub use radix_module::tool::{
        analyze_prepared_program_name, analyze_prepared_program_path, prepare_program_module,
        prepare_program_module_for_name, PreparedProgramModule,
    };
    pub use radix_module::{CompileResult, Compiler};
}

/// Program-layer helpers re-exported from `radix_module::tool` until RTR4.
pub mod services {
    pub use radix_module::tool::{
        analyze_prepared_program_name, analyze_prepared_program_path, prepare_program_module,
        prepare_program_module_for_name, PreparedProgramModule,
    };
}

pub use radix_module::{CompileResult, Compiler, Config, Diagnostic, Session, Target};

pub use radix_module::diagnostics;
pub use radix_module::hir;
pub use radix_module::lexer;
#[cfg(feature = "mir-core")]
pub use radix_module::mir;
