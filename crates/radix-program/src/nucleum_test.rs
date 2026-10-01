use super::*;
use crate::analyze::{AnalysisSeams, analyze_package};
use crate::library::LibraryResolver;
use crate::test_support::{accept_manifest, test_locale_pack};
use radix_module::driver::Config;
use std::fs;
use std::path::{Path, PathBuf};

#[allow(clippy::unnecessary_wraps)]
fn empty_resolver_for_package(
    _config: &Config,
    _package_root: &Path,
) -> Result<LibraryResolver, Vec<Diagnostic>> {
    Ok(LibraryResolver::with_optional_home(None))
}

fn test_seams() -> AnalysisSeams {
    AnalysisSeams {
        locale_loader: test_locale_pack,
        manifest_validator: accept_manifest,
        resolver_for_package: empty_resolver_for_package,
    }
}

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "radix-program-nucleum-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

fn write_nucleum_package(dir: &Path) {
    fs::write(
        dir.join("main.fab"),
        r"
@ public
@ kernel fragment
fn helper(f32 x) → f32 {
    return x
}

@ public
@ kernel
fn k(f32 x) → f32 {
    return helper(x)
}
",
    )
    .expect("source");
}

#[test]
fn analyzed_package_omits_fragments_from_target_symbols() {
    let dir = temp_root("symbols");
    write_nucleum_package(&dir);
    let program = analyze_package(&Config::default(), &dir.join("main.fab"), &test_seams())
        .expect("analyze nucleum package");
    let metadata = metadata_for_program(&program).expect("nucleum metadata");
    assert_eq!(
        metadata.dispatch_and_reflection_names(),
        vec!["k".to_owned()]
    );
    assert_eq!(
        metadata.omitted_target_symbol_names(),
        vec!["helper".to_owned()]
    );
    let symbols = target_symbol_exports(&program);
    assert!(symbols.contains(&"k".to_owned()), "{symbols:?}");
    assert!(!symbols.contains(&"helper".to_owned()), "{symbols:?}");
}

#[test]
fn signature_only_fragment_package_fails_at_analyze() {
    let dir = temp_root("sigonly");
    fs::write(
        dir.join("main.fab"),
        "@ public\n@ kernel fragment\nfn helper(f32 x) → f32\n",
    )
    .expect("source");
    let err = analyze_package(&Config::default(), &dir.join("main.fab"), &test_seams());
    assert!(err.is_err(), "signature-only fragment must fail closed");
}
