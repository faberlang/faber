//! No-op AIR backward bundle when the `device` feature is off.

use radix_mir_lower::lower::LoweredMirUnit;
use radix_module::driver::AnalyzedModule;

pub(super) type AirBundle = ();

pub(super) fn prepare(_analysis: &mut AnalyzedModule) -> Result<Option<AirBundle>, String> {
    Ok(None)
}

pub(super) fn apply<'a>(
    lowered: LoweredMirUnit<'a>,
    _bundle: AirBundle,
    _analysis: &AnalyzedModule,
    _no_fuse: bool,
) -> Result<LoweredMirUnit<'a>, String> {
    Ok(lowered)
}
