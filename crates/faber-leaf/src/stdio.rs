//! Blocking stdio leaf. Serves Latin `consolum:*` routes. No console handle.
//!
//! `print` is a language statement, not this leaf. EOF on `read_line` is an
//! empty string (locked with the N2 acceptance proba).

#[cfg(test)]
#[path = "stdio_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use std::io::{self, BufRead, IsTerminal, Read, Write};

/// Hosts-provider bound reused: `consolum:hauri` rejects reads larger than 1 MiB.
pub const MAX_STDIN_READ_BYTES: i64 = 1024 * 1024;

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &[
    "consolum:hauri",
    "consolum:hauriet",
    "consolum:lege",
    "consolum:leget",
    "consolum:funde",
    "consolum:scribe",
    "consolum:scribet",
    "consolum:dic",
    "consolum:dicet",
    "consolum:mone",
    "consolum:monet",
    "consolum:vide",
    "consolum:videbit",
    "consolum:audit",
    "consolum:loquitur",
    "consolum:admonet",
];

/// Whether `route` belongs to this leaf. Pure name lookup; runs no handler.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer a `consolum:*` route. `None` means the route is not this leaf.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    let result = match route {
        "consolum:hauri" | "consolum:hauriet" => read_bytes(args),
        "consolum:lege" | "consolum:leget" => read_line(args),
        "consolum:funde" => write_bytes(args),
        "consolum:scribe" | "consolum:scribet" => write_line(args),
        "consolum:dic" | "consolum:dicet" => write_partial(args),
        "consolum:mone" | "consolum:monet" => warn_line(args),
        "consolum:vide" | "consolum:videbit" => debug_line(args),
        "consolum:audit" => stdin_is_tty(args),
        "consolum:loquitur" => stdout_is_tty(args),
        "consolum:admonet" => stderr_is_tty(args),
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

fn expect_int(
    route: &'static str,
    args: &[Arg],
    index: usize,
    name: &str,
) -> Result<i64, LeafError> {
    match args.get(index) {
        Some(Arg::Int(value)) => Ok(*value),
        Some(_) => Err(LeafError::invalid_argument(
            route,
            format!("{name} must be numerus"),
        )),
        None => Err(LeafError::invalid_argument(
            route,
            format!("missing {name}"),
        )),
    }
}

fn expect_bytes<'a>(
    route: &'static str,
    args: &'a [Arg],
    index: usize,
) -> Result<&'a [u8], LeafError> {
    match args.get(index) {
        Some(Arg::Bytes(value)) => Ok(value),
        Some(_) => Err(LeafError::invalid_argument(route, "data must be octeti")),
        None => Err(LeafError::invalid_argument(route, "missing data")),
    }
}

fn empty_ok(route: &'static str, args: &[Arg]) -> Result<(), LeafError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(LeafError::invalid_argument(route, "unexpected arguments"))
    }
}

fn write_stdout(route: &'static str, bytes: &[u8]) -> Result<Reply, LeafError> {
    let mut out = io::stdout();
    out.write_all(bytes)
        .and_then(|()| out.flush())
        .map(|()| Reply::Unit)
        .map_err(|err| LeafError::from_io(route, "", &err))
}

fn write_stderr(route: &'static str, text: &str) -> Result<Reply, LeafError> {
    let mut err = io::stderr();
    err.write_all(text.as_bytes())
        .and_then(|()| err.write_all(b"\n"))
        .and_then(|()| err.flush())
        .map(|()| Reply::Unit)
        .map_err(|io_err| LeafError::from_io(route, "", &io_err))
}

fn read_bytes(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:hauri";
    let length = expect_int(ROUTE, args, 0, "byte_length")?;
    if length < 0 {
        return Err(LeafError::invalid_argument(
            ROUTE,
            "byte_length must be non-negative",
        ));
    }
    if length > MAX_STDIN_READ_BYTES {
        return Err(LeafError::invalid_argument(
            ROUTE,
            format!("byte_length must be at most {MAX_STDIN_READ_BYTES} bytes"),
        ));
    }
    let length = usize::try_from(length)
        .map_err(|_| LeafError::invalid_argument(ROUTE, "byte_length is too large"))?;
    let mut buf = vec![0_u8; length];
    let read = io::stdin()
        .lock()
        .read(&mut buf)
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))?;
    buf.truncate(read);
    Ok(Reply::Bytes(buf))
}

fn read_line(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:lege";
    empty_ok(ROUTE, args)?;
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) => Ok(Reply::Str(String::new())),
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(Reply::Str(line))
        }
        Err(err) => Err(LeafError::from_io(ROUTE, "", &err)),
    }
}

fn write_bytes(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:funde";
    let data = expect_bytes(ROUTE, args, 0)?;
    write_stdout(ROUTE, data)
}

fn write_line(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:scribe";
    let msg = expect_str(ROUTE, args, 0, "msg")?;
    write_stdout(ROUTE, format!("{msg}\n").as_bytes())
}

fn write_partial(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:dic";
    let msg = expect_str(ROUTE, args, 0, "msg")?;
    write_stdout(ROUTE, msg.as_bytes())
}

fn warn_line(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:mone";
    let msg = expect_str(ROUTE, args, 0, "msg")?;
    write_stderr(ROUTE, msg)
}

fn debug_line(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:vide";
    let msg = expect_str(ROUTE, args, 0, "msg")?;
    write_stderr(ROUTE, msg)
}

fn stdin_is_tty(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:audit";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Bool(io::stdin().is_terminal()))
}

fn stdout_is_tty(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:loquitur";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Bool(io::stdout().is_terminal()))
}

fn stderr_is_tty(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:admonet";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Bool(io::stderr().is_terminal()))
}
