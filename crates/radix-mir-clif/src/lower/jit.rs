//! The two module kinds a program is lowered into: a relocatable object for the
//! system linker, and an in-memory JIT image for `clif-jit`.
//!
//! Both run the same [`lower_module`](super::lower_module); only the module the
//! functions are defined in, the ISA flags and the entry shims differ.

use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable, Flags};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
use handroll::FxHashSet;

use super::{Decline, ProgramInput, cranelift_error, lower_module};

/// How the program's entry is reached once the module is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryMode {
    /// A linked executable: the object also exports the C `main`.
    Process,
    /// An in-memory image: the host calls `faber_clif_incipit` itself.
    InProcess,
}

/// The host ISA with the flags both routes share. An object is position
/// independent; a JIT image is mapped at its final address and is not.
fn host_isa(position_independent: bool) -> Result<std::sync::Arc<dyn TargetIsa>, Decline> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").map_err(cranelift_error)?;
    flags
        .set(
            "is_pic",
            if position_independent {
                "true"
            } else {
                "false"
            },
        )
        .map_err(cranelift_error)?;
    if !position_independent {
        flags
            .set("use_colocated_libcalls", "false")
            .map_err(cranelift_error)?;
    }
    cranelift_native::builder_with_options(false)
        .map_err(cranelift_error)?
        .finish(Flags::new(flags))
        .map_err(cranelift_error)
}

/// Lower `input` to relocatable object bytes for the host triple.
///
/// # Errors
///
/// Returns the [`Decline`] naming the first shape the lowering cannot emit.
pub fn lower_program(input: &ProgramInput<'_>) -> Result<Vec<u8>, Decline> {
    let isa = host_isa(true)?;
    let builder = ObjectBuilder::new(isa, "radix-mir-clif", default_libcall_names())
        .map_err(cranelift_error)?;
    let mut module = ObjectModule::new(builder);
    lower_module(&mut module, input, EntryMode::Process)?;
    module.finish().emit().map_err(cranelift_error)
}

/// A program compiled into this process's memory.
///
/// The code lives as long as the image; dropping it leaves the mapped code in
/// place (the JIT memory is not freed), so a pointer taken from
/// [`JitProgram::incipit`] stays callable, but the contract is to keep the
/// image alive for as long as the program may run.
pub struct JitProgram {
    module: Box<JITModule>,
    incipit: FuncId,
}

impl JitProgram {
    /// The program's `faber_clif_incipit`: installs the output words, runs the
    /// entry and returns the process exit status the entry hands back.
    #[must_use]
    pub fn incipit(&self) -> unsafe extern "C" fn(*const std::ffi::c_void) -> i32 {
        let code = self.module.get_finalized_function(self.incipit);
        // SAFETY: `define_entry_shims` defines `faber_clif_incipit` as
        // `(arena pointer) -> i32` with the host's C calling convention.
        unsafe {
            std::mem::transmute::<*const u8, unsafe extern "C" fn(*const std::ffi::c_void) -> i32>(
                code,
            )
        }
    }
}

/// Libc and libm routines Cranelift may call for an operation the target has
/// no instruction for. The explicit table replaces the dynamic-symbol lookup
/// the JIT would otherwise fall back on, so an image resolves exactly what is
/// listed and nothing else.
fn libcall_symbols() -> Vec<(&'static str, *const u8)> {
    use std::ffi::c_void;
    unsafe extern "C" {
        fn memcpy(dest: *mut c_void, src: *const c_void, count: usize) -> *mut c_void;
        fn memmove(dest: *mut c_void, src: *const c_void, count: usize) -> *mut c_void;
        fn memset(dest: *mut c_void, byte: i32, count: usize) -> *mut c_void;
        fn memcmp(left: *const c_void, right: *const c_void, count: usize) -> i32;
        fn ceil(value: f64) -> f64;
        fn ceilf(value: f32) -> f32;
        fn floor(value: f64) -> f64;
        fn floorf(value: f32) -> f32;
        fn trunc(value: f64) -> f64;
        fn truncf(value: f32) -> f32;
        fn nearbyint(value: f64) -> f64;
        fn nearbyintf(value: f32) -> f32;
        fn fma(a: f64, b: f64, c: f64) -> f64;
        fn fmaf(a: f32, b: f32, c: f32) -> f32;
    }
    vec![
        ("memcpy", memcpy as *const u8),
        ("memmove", memmove as *const u8),
        ("memset", memset as *const u8),
        ("memcmp", memcmp as *const u8),
        ("ceil", ceil as *const u8),
        ("ceilf", ceilf as *const u8),
        ("floor", floor as *const u8),
        ("floorf", floorf as *const u8),
        ("trunc", trunc as *const u8),
        ("truncf", truncf as *const u8),
        ("nearbyint", nearbyint as *const u8),
        ("nearbyintf", nearbyintf as *const u8),
        ("fma", fma as *const u8),
        ("fmaf", fmaf as *const u8),
    ]
}

/// Lower `input` into this process's memory.
///
/// `runtime` is the explicit table of the runtime's `extern "C"` entry points:
/// every imported runtime symbol must be in it, and a symbol it lacks ends the
/// compile with a named [`Decline`] instead of a failed lookup at run time.
///
/// # Errors
///
/// Returns the [`Decline`] naming the first shape the lowering cannot emit, or
/// the runtime symbol the table does not provide.
pub fn lower_program_jit(
    input: &ProgramInput<'_>,
    runtime: &[(&str, *const u8)],
) -> Result<JitProgram, Decline> {
    let isa = host_isa(false)?;
    let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
    builder.symbols(libcall_symbols());
    builder.symbols(runtime.iter().copied());
    let mut module = JITModule::new(builder);
    let incipit = lower_module(&mut module, input, EntryMode::InProcess)?;
    let provided: FxHashSet<&str> = runtime
        .iter()
        .map(|(name, _)| *name)
        .chain(libcall_symbols().into_iter().map(|(name, _)| name))
        .collect();
    for (_, declaration) in module.declarations().get_functions() {
        if declaration.linkage == Linkage::Import
            && let Some(name) = declaration.name.as_deref()
            && !provided.contains(name)
        {
            return Err(Decline::new(format!(
                "runtime symbol {name} is not provided"
            )));
        }
    }
    module.finalize_definitions().map_err(cranelift_error)?;
    Ok(JitProgram {
        module: Box::new(module),
        incipit,
    })
}
