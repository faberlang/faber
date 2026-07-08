use crate::cli::InstallArgs;
use crate::library::FABER_LIBRARY_HOME_ENV;
use std::path::PathBuf;
use std::process::Command;

pub(super) fn cmd_install(args: InstallArgs) {
    if !valid_library_name(&args.library) {
        eprintln!(
            "error: invalid library name `{}`; use ASCII letters, numbers, underscore, or hyphen",
            args.library
        );
        std::process::exit(1);
    }

    let Some(home) = std::env::var_os(FABER_LIBRARY_HOME_ENV).map(PathBuf::from) else {
        eprintln!("error: FABER_LIBRARY_HOME is required for `faber install`");
        std::process::exit(1);
    };

    if let Err(err) = std::fs::create_dir_all(&home) {
        eprintln!(
            "error: failed to create FABER_LIBRARY_HOME at {}: {err}",
            home.display()
        );
        std::process::exit(1);
    }

    let target = home.join(&args.library);
    let status = if target.exists() {
        Command::new("git")
            .arg("-C")
            .arg(&target)
            .arg("pull")
            .arg("--ff-only")
            .status()
    } else {
        Command::new("git")
            .arg("clone")
            .arg(format!("https://github.com/faberlang/{}.git", args.library))
            .arg(&target)
            .status()
    };

    match status {
        Ok(status) if status.success() => {
            println!("installed {} at {}", args.library, target.display());
        }
        Ok(status) => {
            eprintln!("error: git exited with status {status}");
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(err) => {
            eprintln!("error: failed to run git for `faber install`: {err}");
            std::process::exit(1);
        }
    }
}

fn valid_library_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}
