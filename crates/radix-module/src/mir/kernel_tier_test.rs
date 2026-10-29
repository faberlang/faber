//! The kernel tier reaches the MIR device metadata the emitters read, and the
//! safe switch lowers it to the default tier (operator ruling 2026-10-28).

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{KernelLowering, MirLoweringOptions, lower_analyzed_unit};
use crate::program::analyze_prepared_program_name;
use radix_mir::device::KernelTier;

const SOURCE: &str = r#"+++
locale = "en"
+++
@ kernel
@ public
fn plain(f32 x) → f32 { return x + 1.0 }

@ kernel relaxed
@ public
fn loose(f32 x) → f32 { return x + 1.0 }

@ kernel unsafe
@ public
fn raw(f32 x) → f32 { return x + 1.0 }

main {}
"#;

/// The tier carried by each kernel's device metadata row, in source order.
fn lowered_tiers(config: Config) -> Vec<KernelTier> {
    let unit = analyze_prepared_program_name(&Session::new(config), "tiers.fab", SOURCE)
        .unwrap_or_else(|errors| panic!("source must analyze: {errors:?}"));
    let lowered = lower_analyzed_unit(
        unit.application_view(),
        MirLoweringOptions { kernel_lowering: KernelLowering::Device, ..Default::default() },
    )
    .expect("kernels lower");
    lowered
        .validated
        .program()
        .functions
        .iter()
        .filter_map(|function| lowered.device_function_metadata.get(&function.id))
        .map(|metadata| metadata.tier)
        .collect()
}

#[test]
fn the_declared_tier_reaches_the_mir_kernel_metadata() {
    let config = Config::default().with_target(Target::HirTypeScript);
    assert_eq!(
        lowered_tiers(config),
        [KernelTier::Default, KernelTier::Relaxed, KernelTier::Unsafe]
    );
}

#[test]
fn the_safe_switch_lowers_every_kernel_to_the_default_tier() {
    let config = Config::default()
        .with_target(Target::HirTypeScript)
        .with_safe_kernels();
    assert_eq!(lowered_tiers(config), [KernelTier::Default; 3]);
}
