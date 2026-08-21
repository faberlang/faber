use super::*;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct EnvRestore {
    home: Option<std::ffi::OsString>,
}

impl EnvRestore {
    fn capture() -> Self {
        Self {
            home: std::env::var_os(FABER_LIBRARY_HOME_ENV),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env(FABER_LIBRARY_HOME_ENV, self.home.take());
    }
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

#[test]
fn workspace_library_probe_finds_sibling_norma_when_home_unset() {
    let _guard = env_guard();
    let _env = EnvRestore::capture();
    std::env::remove_var(FABER_LIBRARY_HOME_ENV);

    let home = default_library_home();
    assert!(home.is_some());
    assert!(home.unwrap().join("norma/src/solum.fab").is_file());
}

#[test]
fn explicit_library_home_wins_over_workspace_probe() {
    let _guard = env_guard();
    let _env = EnvRestore::capture();
    let explicit = PathBuf::from("/tmp/faber-explicit-library-home-test");
    std::env::set_var(FABER_LIBRARY_HOME_ENV, &explicit);

    assert_eq!(default_library_home(), Some(explicit));
}
