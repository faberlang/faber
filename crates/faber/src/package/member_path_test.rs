use super::resolve_package_member;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> Result<PathBuf, Box<dyn Error>> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(Box::<dyn Error>::from)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("faber-member-path-{label}-{nonce}"));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn has_arg(diagnostic: &radix::diagnostics::Diagnostic, name: &str, value: &str) -> bool {
    diagnostic
        .args
        .iter()
        .any(|arg| arg.name == name && arg.value == value)
}

#[test]
fn resolve_package_member_rejects_parent_escape() -> Result<(), Box<dyn Error>> {
    let root = temp_dir("parent-escape")?;
    let package_root = root.join("pkg");
    fs::create_dir_all(package_root.join("src"))?;
    let anchor = package_root.join("faber.toml");
    fs::write(&anchor, "")?;

    let diagnostic = resolve_package_member(&package_root, "../outside.fab", &anchor)
        .expect_err("parent escape should be rejected");

    assert!(
        has_arg(&diagnostic, "issue", "package_member_parent_escape"),
        "expected parent escape issue, got {diagnostic:?}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn resolve_package_member_rejects_symlink_escape() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let root = temp_dir("symlink-escape")?;
    let package_root = root.join("pkg");
    let outside = root.join("outside");
    fs::create_dir_all(package_root.join("src"))?;
    fs::create_dir_all(&outside)?;
    fs::write(outside.join("escaped.fab"), "nota \"outside\"\n")?;
    symlink(&outside, package_root.join("src").join("linked"))?;
    let anchor = package_root.join("faber.toml");
    fs::write(&anchor, "")?;

    let diagnostic = resolve_package_member(&package_root, "src/linked/escaped.fab", &anchor)
        .expect_err("symlink escape should be rejected");

    assert!(
        has_arg(&diagnostic, "issue", "package_member_symlink_escape"),
        "expected symlink escape issue, got {diagnostic:?}"
    );
    Ok(())
}
