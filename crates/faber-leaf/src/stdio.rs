//! Blocking stdio leaf. Serves Latin `consolum:*` routes. No console handle.
//!
//! `print` is a language statement, not this leaf. EOF on `read_line` is still
//! an empty string (locked with the N2 acceptance proba); Norma distinguishes
//! EOF from a blank line by calling the non-consuming `consolum:desinit`
//! probe first.

#[cfg(test)]
#[path = "stdio_test.rs"]
mod tests;

use crate::error::LeafError;
use crate::value::{Arg, Reply};
use std::io::{self, BufRead, IsTerminal, Read, Write};

/// Hosts-provider bound reused: `consolum:hauri` rejects reads larger than 1 MiB.
pub const MAX_STDIN_READ_BYTES: i64 = 1024 * 1024;

/// Route names this leaf serves. Must match the `serve` match arms below.
pub(crate) const ROUTES: &[&str] = &[
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
    "consolum:desinit",
    "consolum:metire",
];

/// Whether `route` belongs to this leaf. Pure name lookup; runs no handler.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer a `consolum:*` route. `None` means the route is not this leaf.
///
/// Async aliases (`hauriet`, `leget`, `scribet`, `dicet`, `monet`, `videbit`)
/// run the sync handler and report the canonical route in errors.
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
        "consolum:desinit" => stdin_ended(args),
        "consolum:metire" => tty_size(args),
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

/// Collect bytes from an opener that flattened an `octeti` value into one
/// positional integer per byte, while still accepting a single `Arg::Bytes`.
fn expect_byte_list(route: &'static str, args: &[Arg]) -> Result<Vec<u8>, LeafError> {
    match args {
        [Arg::Bytes(value)] => Ok(value.clone()),
        [] => Err(LeafError::invalid_argument(route, "missing data")),
        other => other
            .iter()
            .map(|arg| match arg {
                Arg::Int(value) => u8::try_from(*value)
                    .map_err(|_| LeafError::invalid_argument(route, "byte must be 0..=255")),
                _ => Err(LeafError::invalid_argument(route, "data must be octeti")),
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

fn write_stdout(route: &'static str, bytes: &[u8]) -> Result<Reply, LeafError> {
    let mut out = io::stdout();
    out.write_all(bytes)
        .and_then(|()| out.flush())
        .map(|()| Reply::Unit)
        .map_err(|err| LeafError::from_io(route, "", &err))
}

/// Write `text` plus one newline to stderr.
fn write_stderr_line(route: &'static str, text: &str) -> Result<Reply, LeafError> {
    let mut err = io::stderr();
    err.write_all(text.as_bytes())
        .and_then(|()| err.write_all(b"\n"))
        .and_then(|()| err.flush())
        .map(|()| Reply::Unit)
        .map_err(|io_err| LeafError::from_io(route, "", &io_err))
}

/// One `read` from stdin, so a short answer is possible; EOF and a zero-length
/// request both return empty `bytes`.
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
    let mut stdin = io::stdin().lock();
    read_line_from(&mut stdin)
        .map(Reply::Str)
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))
}

/// Read one line from `reader`, stripping one trailing `\n` and then one
/// trailing `\r`. At EOF this returns an empty string.
fn read_line_from<R: BufRead>(reader: &mut R) -> io::Result<String> {
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read > 0 && line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(line)
}

/// Non-consuming end-of-input probe: blocks until a byte arrives or the
/// stream closes, then reports whether stdin has no more bytes. `fill_buf`
/// consumes nothing, so a following `read_line` still sees the pending input.
fn stdin_ended(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:desinit";
    empty_ok(ROUTE, args)?;
    let mut stdin = io::stdin().lock();
    input_ended(&mut stdin)
        .map(Reply::Bool)
        .map_err(|err| LeafError::from_io(ROUTE, "", &err))
}

fn input_ended<R: BufRead>(reader: &mut R) -> io::Result<bool> {
    Ok(reader.fill_buf()?.is_empty())
}

fn write_bytes(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:funde";
    let data = expect_byte_list(ROUTE, args)?;
    write_stdout(ROUTE, &data)
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
    write_stderr_line(ROUTE, msg)
}

fn debug_line(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:vide";
    let msg = expect_str(ROUTE, args, 0, "msg")?;
    write_stderr_line(ROUTE, msg)
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

/// Terminal size of stdout as `[columns, rows]`. Zeroes when stdout is not a
/// terminal or the host cannot answer, so a monitor can read a degenerate size
/// instead of handling an error. This route never errors on a tty-less host.
fn tty_size(args: &[Arg]) -> Result<Reply, LeafError> {
    const ROUTE: &str = "consolum:metire";
    empty_ok(ROUTE, args)?;
    let (columns, rows) = terminal_size();
    Ok(Reply::List(vec![Reply::Int(columns), Reply::Int(rows)]))
}

#[cfg(unix)]
fn terminal_size() -> (i64, i64) {
    let mut size = libc::winsize {
        ws_col: 0,
        ws_row: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `size` is a valid, writable `winsize` for the whole call, and
    // `TIOCGWINSZ` on stdout (fd 1) writes nothing else. A failing ioctl
    // leaves the zeroed struct untouched, which is the documented degenerate
    // answer for a non-terminal stream.
    let status = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &raw mut size) };
    if status != 0 {
        return (0, 0);
    }
    (i64::from(size.ws_col), i64::from(size.ws_row))
}

#[cfg(not(unix))]
fn terminal_size() -> (i64, i64) {
    (0, 0)
}
