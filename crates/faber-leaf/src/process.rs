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
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

static PROGRAM_ARGV: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Live spawned children keyed by pid, so `exspecta` / `neca` can reach them.
static CHILDREN: Mutex<Option<HashMap<i64, Child>>> = Mutex::new(None);

fn with_children<T>(f: impl FnOnce(&mut HashMap<i64, Child>) -> T) -> T {
    let mut guard = match CHILDREN.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(guard.get_or_insert_with(HashMap::new))
}

/// Drop every tracked child. Test isolation only.
#[cfg(test)]
fn reset_children() {
    with_children(HashMap::clear);
}

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

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &[
    "processus:genera",
    "processus:dimitte",
    "processus:captura",
    "processus:exsequi",
    "processus:exsequetur",
    "processus:lege",
    "processus:sedes",
    "processus:muta",
    "processus:identitas",
    "processus:argumenta",
    "processus:exspecta",
    "processus:neca",
];

/// Whether `route` belongs to this leaf. Pure name lookup; spawns nothing.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
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
        "processus:exspecta" => wait(args),
        "processus:neca" => kill(args),
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

fn expect_pid(route: &'static str, args: &[Arg]) -> Result<i64, LeafError> {
    match args {
        [Arg::Int(value)] => Ok(*value),
        [] => Err(LeafError::invalid_argument(route, "missing pid")),
        _ => Err(LeafError::invalid_argument(route, "pid must be int")),
    }
}

/// `processus:exspecta` — block until the tracked child exits; return its
/// exit code (-1 when killed by a signal) and stop tracking it.
fn wait(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:exspecta";
    let pid = expect_pid(ROUTE, args)?;
    let mut child = with_children(|map| map.remove(&pid))
        .ok_or_else(|| LeafError::invalid_argument(ROUTE, "unknown pid"))?;
    let status = child
        .wait()
        .map_err(|err| LeafError::from_io(ROUTE, "wait", &err))?;
    Ok(Reply::Int(i64::from(status.code().unwrap_or(-1))))
}

/// `processus:neca` — kill the tracked child. Killing a child that already
/// exited is not an error; an unknown pid is.
fn kill(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "processus:neca";
    let pid = expect_pid(ROUTE, args)?;
    let known = with_children(|map| match map.get_mut(&pid) {
        Some(child) => {
            // WHY: an already-exited child reports InvalidInput from kill();
            // the child is being discarded either way, so it is not actionable.
            drop(child.kill());
            true
        }
        None => false,
    });
    if !known {
        return Err(LeafError::invalid_argument(ROUTE, "unknown pid"));
    }
    Ok(Reply::Unit)
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
    let child = command
        .spawn()
        .map_err(|err| LeafError::from_io(ROUTE, &argv[0], &err))?;
    let pid = i64::from(child.id());
    with_children(|map| map.insert(pid, child));
    Ok(Reply::Int(pid))
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
    let mut child = command
        .spawn()
        .map_err(|err| LeafError::from_io(ROUTE, &argv[0], &err))?;
    let pid = i64::from(child.id());
    // Reap in the background so a detached child never becomes a zombie.
    std::thread::spawn(move || {
        // WHY: reaping is best-effort in a detached thread; a wait error has
        // no caller to report to.
        drop(child.wait());
    });
    Ok(Reply::Int(pid))
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
