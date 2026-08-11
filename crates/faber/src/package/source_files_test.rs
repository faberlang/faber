use super::{is_package_source_path, is_proba_source_path, package_source_files};
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "faber-source-files-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

#[test]
fn package_source_files_production_skips_proba() {
    let root = temp_root("prod");
    fs::write(root.join("lib.fab"), "functio id() → numerus { redde 1 }\n").unwrap();
    fs::write(
        root.join("lib.proba"),
        "probandum \"lib\" { proba \"ok\" { adfirma 1 ≡ 1 } }\n",
    )
    .unwrap();

    let files = package_source_files(&root, false).expect("discover");
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("lib.fab"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn package_source_files_test_includes_proba() {
    let root = temp_root("test");
    fs::write(root.join("lib.fab"), "functio id() → numerus { redde 1 }\n").unwrap();
    fs::write(
        root.join("lib.proba"),
        "probandum \"lib\" { proba \"ok\" { adfirma 1 ≡ 1 } }\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(
        root.join("nested/extra.proba"),
        "probandum \"nested\" { proba \"ok\" { adfirma 2 ≡ 2 } }\n",
    )
    .unwrap();

    let files = package_source_files(&root, true).expect("discover");
    let names: Vec<_> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"lib.fab".to_owned()));
    assert!(names.contains(&"lib.proba".to_owned()));
    assert!(names.contains(&"extra.proba".to_owned()));
    assert_eq!(files.len(), 3);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn is_package_source_path_respects_proba_flag() {
    let fab = PathBuf::from("x.fab");
    let proba = PathBuf::from("x.proba");
    let rs = PathBuf::from("x.rs");
    assert!(is_package_source_path(&fab, false));
    assert!(is_package_source_path(&fab, true));
    assert!(!is_package_source_path(&proba, false));
    assert!(is_package_source_path(&proba, true));
    assert!(!is_package_source_path(&rs, true));
    assert!(is_proba_source_path(&proba));
    assert!(!is_proba_source_path(&fab));
}
