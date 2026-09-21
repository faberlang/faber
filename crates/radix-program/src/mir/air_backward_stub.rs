//! No-op AIR backward bundle when the `device` feature is off.

use radix_module::driver::AnalyzedModule;
use radix_module::mir::LoweredMirUnit;

pub(super) type AirBundle = ();

pub(super) fn prepare(_analysis: &mut AnalyzedModule) -> Result<Option<AirBundle>, String> {
    Ok(None)
}

pub(super) fn apply(
    lowered: LoweredMirUnit<'_>,
    _bundle: AirBundle,
    _analysis: &AnalyzedModule,
    _no_fuse: bool,
) -> Result<LoweredMirUnit<'_>, String> {
    Ok(lowered)
}
