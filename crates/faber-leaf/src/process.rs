//! Blocking process leaf. Serves Latin `processus:*` routes.
//!
//! `spawn` / `Subprocess` stay N4b. `processus:exi` stays a runner/product
//! builtin (`Host::exit`) so `faber run` and cargo tests do not share a
//! `std::process::exit` path. `write_env` is not restored.

#[cfg(test)]
#[path = "process_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use std::process::{Command, Stdio};
use std::sync::Mutex;

static PROGRAM_ARGV: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Install the Faber program argv for this execution (excludes runtime/script).
pub fn install_argv(args: Vec<String>) {
    *lock_argv() = Some(args);
}

/// Drop a previous execution's argv so the next program does not inherit it.
pub fn reset_argv() {
    *lock_argv() = None;
}

fn lock_argv() -> std::sync::MutexGuard<'static, Option<Vec<String>>> {
    match PROGRAM_ARGV.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn program_argv() -> Vec<String> {
    lock_argv()
        .clone()
        .unwrap_or_else(|| std::env::args().skip(1).collect())
}

/// Whether `route` belongs to this leaf.
#[must_use]
pub fn is_route(route: &str) -> bool {
    serve(route, &[]).is_some()
}

/// Answer a `processus:*` route. `None` means the route is not this leaf.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "processus:genera" => spawn(args),
        "processus:dimitte" => detach(args),
        "processus:captura" => capture(args),
        "processus:exsequi" | "processus:exsequetur" => exec(args),
        "processus:lege" => read_env(args),
        "processus:sedes" => cwd(args),
        "processus:muta" => chdir(args),
        "processus:identitas" => pid(args),
        "processus:argumenta" => argv(args),
        _ => return None,
    };
    Some(result)
}

fn expect_str<'a>(
    route: &'static str,
    args: &'a [Arg],
    index: usize,
    name: &str,
) -> Result<&'a str, LeafError> {
    match args.get(index) {
        Some(Arg::Str(value)) => Ok(value),
        Some(Arg::Nil) => Err(LeafError::invalid_argument(
            route,
            format!("{name} must be textus"),
        )),
        Some(_) => Err(LeafError::invalid_argument(
            route,
            format!("{name} must be textus"),
        )),
        None => Err(LeafError::invalid_argument(
            route,
            format!("missing {name}"),
        )),
    }
}

/// Collect the child argv from an opener that flattened a `lista<textus>`
/// into one positional `Str` per element, while still accepting a single
/// `Arg::Strs` when the list arrived intact.
fn expect_argv(route: &'static str, args: &[Arg]) -> Result<Vec<String>, LeafError> {
    match args {
        [Arg::Strs(value)] => Ok(value.clone()),
        [] => Err(LeafError::invalid_argument(route, "missing args")),
        other => other
            .iter()
            .map(|arg| match arg {
                Arg::Str(value) => Ok(value.clone()),
                _ => Err(LeafError::invalid_argument(
                    route,
                    "args must be lista<textus>",
                )),
            })
            .collect(),
    }
}

fn empty_ok(route: &'static str, args: &[Arg]) -> Result<(), LeafError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(LeafError::invalid_argument(route, "unexpected arguments"))
    }
}

fn spawn(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:genera";
    let argv = expect_argv(ROUTE, args)?;
    if argv.is_empty() || argv[0].is_empty() {
        return Err(LeafError::invalid_argument(
            ROUTE,
            "empty argv / empty command",
        ));
    }
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command
        .spawn()
        .map(|child| Reply::Int(i64::from(child.id())))
        .map_err(|err| LeafError::from_io(ROUTE, &argv[0], &err))
}

fn detach(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:dimitte";
    let argv = expect_argv(ROUTE, args)?;
    if argv.is_empty() || argv[0].is_empty() {
        return Err(LeafError::invalid_argument(
            ROUTE,
            "empty argv / empty command",
        ));
    }
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command
        .spawn()
        .map(|child| Reply::Int(i64::from(child.id())))
        .map_err(|err| LeafError::from_io(ROUTE, &argv[0], &err))
}

fn capture(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:captura";
    let argv = expect_argv(ROUTE, args)?;
    if argv.is_empty() || argv[0].is_empty() {
        return Err(LeafError::invalid_argument(
            ROUTE,
            "empty argv / empty command",
        ));
    }
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|err| LeafError::from_io(ROUTE, &argv[0], &err))?;
    Ok(Reply::Map(vec![
        (
            "status".to_owned(),
            Reply::Int(i64::from(output.status.code().unwrap_or(-1))),
        ),
        (
            "stdout".to_owned(),
            Reply::Str(String::from_utf8_lossy(&output.stdout).into_owned()),
        ),
        (
            "stderr".to_owned(),
            Reply::Str(String::from_utf8_lossy(&output.stderr).into_owned()),
        ),
    ]))
}

fn exec(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:exsequi";
    let command = expect_str(ROUTE, args, 0, "command")?;
    if command.is_empty() {
        return Err(LeafError::invalid_argument(ROUTE, "empty command"));
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|err| LeafError::from_io(ROUTE, command, &err))?;
    if !output.status.success() {
        return Err(LeafError::from_io(
            ROUTE,
            command,
            &std::io::Error::other(format!(
                "exit non-zero ({}): {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        ));
    }
    Ok(Reply::Str(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

fn read_env(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:lege";
    let name = expect_str(ROUTE, args, 0, "name")?;
    match std::env::var(name) {
        Ok(value) => Ok(Reply::Str(value)),
        Err(std::env::VarError::NotPresent) => Err(LeafError::unset(
            ROUTE,
            format!("environment variable `{name}` is not set"),
        )),
        Err(err) => Err(LeafError::invalid_argument(ROUTE, err.to_string())),
    }
}

fn cwd(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:sedes";
    empty_ok(ROUTE, args)?;
    std::env::current_dir()
        .map(|path| Reply::Str(path.to_string_lossy().into_owned()))
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))
}

fn chdir(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:muta";
    let path = expect_str(ROUTE, args, 0, "path")?;
    std::env::set_current_dir(path)
        .map(|()| Reply::Unit)
        .map_err(|err| LeafError::from_io(ROUTE, path, &err))
}

fn pid(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:identitas";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Int(i64::from(std::process::id())))
}

fn argv(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:argumenta";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Strs(program_argv()))
}
