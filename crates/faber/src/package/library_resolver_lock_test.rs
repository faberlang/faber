//! G1-U2 (S-D3 `8b5c1906`): the resolver's locked map carries only
//! manifest-declared lock rows plus the producer-sanctioned platform-default
//! providers. Absolute store paths are lock contract (`faber-lock.md`
//! fields 4/8/9/10), so the declared-name filter is the whole fix — any
//! other undeclared row must never become importable.

use std::fs;

use radix::module::driver::Config;
use radix::program::library::LibraryResolver;

use super::library_resolver_for_package;

/// App declaring only `goodpkg`, with a lock that also carries an undeclared
/// `evilpkg` row whose interface root holds a resolvable module, and an
/// undeclared platform-default `norma` row.
fn lock_fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("create temp root");
    let app = root.path().join("app");
    let store = root.path().join("store");
    let good_interfaces = store.join("goodpkg/interfaces");
    let evil_interfaces = store.join("evilpkg/interfaces");
    let norma_interfaces = store.join("norma/interfaces");
    fs::create_dir_all(app.join("src")).expect("create app source directory");
    fs::create_dir_all(store.join("goodpkg")).expect("create good package root");
    fs::create_dir_all(&good_interfaces).expect("create good interfaces");
    fs::create_dir_all(&evil_interfaces).expect("create evil interfaces");
    fs::create_dir_all(&norma_interfaces).expect("create norma interfaces");
    fs::write(good_interfaces.join("module.fab"), "func good() → int { return 1 }")
        .expect("write good interface");
    fs::write(evil_interfaces.join("module.fab"), "func evil() → int { return 1 }")
        .expect("write evil interface");
    fs::write(norma_interfaces.join("module.fab"), "func norma_fn() → int { return 1 }")
        .expect("write norma interface");

    fs::write(
        app.join("faber.toml"),
        r#"[package]
name = "app"

[library]
provider = "selfp"

[dependencies]
goodpkg = "1.0.0"
"#,
    )
    .expect("write app manifest");

    let row = |name: &str, interfaces: &std::path::Path| {
        format!(
            r#"[[package]]
name = "{name}"
version = "1.0.0"
source = "store"
package_root = "{}"
kind = "lib"
target_language = "faber"
target_triple = "none"
target_manifest = "{}"
interface_root = "{}"
artifact = ""
crate = "{name}"
rustc = ""
"#,
            store.join(name).display(),
            store.join(name).join("cista.toml").display(),
            interfaces.display(),
        )
    };
    fs::write(
        app.join("faber.lock"),
        format!(
            "{}{}{}",
            row("goodpkg", &good_interfaces),
            row("evilpkg", &evil_interfaces),
            row("norma", &norma_interfaces)
        ),
    )
    .expect("write lock");

    fs::write(app.join("src/module.fab"), "func entry() → int { return 0 }")
        .expect("write self module");
    root
}

fn resolver_for(root: &tempfile::TempDir) -> LibraryResolver {
    let config = Config {
        library_home: Some(root.path().join("empty-library-home")),
        ..Config::default()
    };
    library_resolver_for_package(&config, &root.path().join("app")).expect("resolver builds")
}

#[test]
fn undeclared_lock_entry_never_enters_the_resolver() {
    let root = lock_fixture();
    let resolver = resolver_for(&root);
    // The evil interface file exists and would resolve if the undeclared row
    // reached the locked map; resolution must fail instead of opening it.
    assert!(
        resolver.resolve("evilpkg:module").is_err(),
        "an import backed only by an undeclared lock row must fail resolution"
    );
}

#[test]
fn declared_entries_and_provider_self_entry_still_resolve() {
    let root = lock_fixture();
    let resolver = resolver_for(&root);

    let good = resolver
        .resolve("goodpkg:module")
        .expect("declared entry resolves")
        .expect("declared entry is Some");
    assert_eq!(good.package, "goodpkg");
    assert_eq!(
        good.interface_path,
        root.path().join("store/goodpkg/interfaces/module.fab")
    );

    let self_module = resolver
        .resolve("selfp:module")
        .expect("provider self-entry resolves")
        .expect("provider self-entry is Some");
    assert_eq!(
        self_module.interface_path,
        root.path().join("app/src/module.fab")
    );
}

#[test]
fn platform_default_norma_row_resolves_without_declaration() {
    let root = lock_fixture();
    let resolver = resolver_for(&root);
    // Producer-sanctioned exception: cista rewrites the norma lock row
    // without a `[dependencies]` entry (cista install.rs
    // `PLATFORM_DEFAULT_PACKAGES`), and the pinned platform-default test
    // (`package_test.rs` locked-norma-platform-default) requires it to win
    // over library home.
    let norma = resolver
        .resolve("norma:module")
        .expect("platform-default row resolves")
        .expect("platform-default row is Some");
    assert_eq!(
        norma.interface_path,
        root.path().join("store/norma/interfaces/module.fab")
    );
}
