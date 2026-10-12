//! Blocking fs leaf. Serves Latin `solum:*` routes. No open-file handle.
//!
//! `stat` / `FileStatus` and `map_read` / `MappedRegion` are named in the
//! contract and not implemented here. Runner-only `digest_file` is not
//! restored (R1).

#[cfg(test)]
#[path = "fs_test.rs"]
mod tests;

use crate::error::FsError;
use crate::value::{Arg, Reply};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

/// Largest single `solum:partem` allocation (512 MiB). Implementation ceiling,
/// not a language constant. Same bound the retired runner kernel used.
pub const MAX_RANGE_READ_BYTES: usize = 512 * 1024 * 1024;

/// Route names this leaf serves. Must match the `serve` match arms below.
const ROUTES: &[&str] = &[
    "solum:lege",
    "solum:hauri",
    "solum:hauriet",
    "solum:partem",
    "solum:inveni",
    "solum:carpe",
    "solum:carpiet",
    "solum:scribe",
    "solum:scribet",
    "solum:funde",
    "solum:appone",
    "solum:apponet",
    "solum:exstat",
    "solum:exstabit",
    "solum:directoriumne",
    "solum:regularene",
    "solum:legibilene",
    "solum:vinculumne",
    "solum:mensura",
    "solum:modus",
    "solum:status",
    "solum:status_async",
    "solum:sequere",
    "solum:sequetur",
    "solum:vincula",
    "solum:dele",
    "solum:delet",
    "solum:copy",
    "solum:copy_async",
    "solum:renomina",
    "solum:renominabit",
    "solum:tange",
    "solum:tanget",
    "solum:modum",
    "solum:crea",
    "solum:creabit",
    "solum:enumera",
    "solum:enumerabit",
    "solum:amputa",
    "solum:amputabit",
    "solum:iunge",
    "solum:parens",
    "solum:nomen",
    "solum:suffixum",
    "solum:absolve",
    "solum:domus",
    "solum:temporarium",
];

/// Whether `route` belongs to this leaf. Pure name lookup; touches no file.
#[must_use]
pub fn is_route(route: &str) -> bool {
    ROUTES.contains(&route)
}

/// Answer a `solum:*` route. `None` means the route is not this leaf.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, FsError>> {
    let result = match route {
        "solum:lege" => read_text(args),
        "solum:hauri" | "solum:hauriet" => read_bytes(args),
        "solum:partem" => read_range(args),
        "solum:inveni" => find(args),
        "solum:carpe" | "solum:carpiet" => read_lines(args),
        "solum:scribe" | "solum:scribet" => write_text(args),
        "solum:funde" => write_bytes(args),
        "solum:appone" | "solum:apponet" => append_text(args),
        "solum:exstat" | "solum:exstabit" => exists(args),
        "solum:directoriumne" => is_directory(args),
        "solum:regularene" => is_regular(args),
        "solum:legibilene" => is_readable(args),
        "solum:vinculumne" => is_symlink(args),
        "solum:mensura" => byte_length(args),
        "solum:modus" => mode(args),
        "solum:status" | "solum:status_async" => file_status(args),
        "solum:sequere" | "solum:sequetur" => read_link(args),
        "solum:vincula" => create_link(args),
        "solum:dele" | "solum:delet" => delete_file(args),
        "solum:copy" | "solum:copy_async" => copy_file(args),
        "solum:renomina" | "solum:renominabit" => rename_path(args),
        "solum:tange" | "solum:tanget" => touch(args),
        "solum:modum" => set_mode(args),
        "solum:crea" | "solum:creabit" => create_dir(args),
        "solum:enumera" | "solum:enumerabit" => list_dir(args),
        "solum:amputa" | "solum:amputabit" => remove_tree(args),
        "solum:iunge" => join(args),
        "solum:parens" => parent(args),
        "solum:nomen" => file_name(args),
        "solum:suffixum" => extension(args),
        "solum:absolve" => resolve(args),
        "solum:domus" => home(args),
        "solum:temporarium" => temp_dir(args),
        _ => return None,
    };
    Some(result)
}

fn expect_str<'a>(
    route: &'static str,
    args: &'a [Arg],
    index: usize,
    name: &str,
) -> Result<&'a str, FsError> {
    match args.get(index) {
        Some(Arg::Str(value)) => Ok(value),
        Some(_) => Err(FsError::invalid_argument(
            route,
            format!("{name} must be textus"),
        )),
        None => Err(FsError::invalid_argument(route, format!("missing {name}"))),
    }
}

fn expect_int(route: &'static str, args: &[Arg], index: usize, name: &str) -> Result<i64, FsError> {
    match args.get(index) {
        Some(Arg::Int(value)) => Ok(*value),
        Some(_) => Err(FsError::invalid_argument(
            route,
            format!("{name} must be numerus"),
        )),
        None => Err(FsError::invalid_argument(route, format!("missing {name}"))),
    }
}

fn expect_bytes<'a>(
    route: &'static str,
    args: &'a [Arg],
    index: usize,
) -> Result<&'a [u8], FsError> {
    match args.get(index) {
        Some(Arg::Bytes(value)) => Ok(value),
        Some(_) => Err(FsError::invalid_argument(route, "data must be octeti")),
        None => Err(FsError::invalid_argument(route, "missing data")),
    }
}

fn join_parts(route: &'static str, args: &[Arg]) -> Result<Vec<String>, FsError> {
    match args {
        [Arg::Strs(value)] => Ok(value.clone()),
        [] => Err(FsError::invalid_argument(route, "missing parts")),
        other => other
            .iter()
            .map(|arg| match arg {
                Arg::Str(value) => Ok(value.clone()),
                _ => Err(FsError::invalid_argument(
                    route,
                    "parts must be lista<textus>",
                )),
            })
            .collect(),
    }
}

fn empty_ok(route: &'static str, args: &[Arg]) -> Result<(), FsError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(FsError::invalid_argument(route, "unexpected arguments"))
    }
}

fn read_text(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:lege";
    let path = expect_str(ROUTE, args, 0, "path")?;
    refuse_directory(ROUTE, path)?;
    fs::read_to_string(path)
        .map(Reply::Str)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn read_bytes(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:hauri";
    let path = expect_str(ROUTE, args, 0, "path")?;
    refuse_directory(ROUTE, path)?;
    if args.len() > 1 {
        let max = expect_int(ROUTE, args, 1, "max")?;
        let max = u64::try_from(max)
            .map_err(|_| FsError::invalid_argument(ROUTE, "max must be non-negative"))?;
        let size = fs::metadata(path)
            .map(|meta| meta.len())
            .map_err(|err| FsError::from_io(ROUTE, path, &err))?;
        if size > max {
            return Err(FsError::invalid_argument(
                ROUTE,
                format!("file is {size} bytes, exceeding the {max}-byte ceiling"),
            ));
        }
    }
    fs::read(path)
        .map(Reply::Bytes)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn read_range(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:partem";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let start = expect_int(ROUTE, args, 1, "offset")?;
    let length = expect_int(ROUTE, args, 2, "length")?;
    let start = u64::try_from(start)
        .map_err(|_| FsError::invalid_argument(ROUTE, "offset must be non-negative"))?;
    let length = usize::try_from(length)
        .map_err(|_| FsError::invalid_argument(ROUTE, "length must be non-negative"))?;
    if length > MAX_RANGE_READ_BYTES {
        return Err(FsError::invalid_argument(
            ROUTE,
            format!("length must be at most {MAX_RANGE_READ_BYTES} bytes"),
        ));
    }
    let mut file = File::open(path).map_err(|err| FsError::from_io(ROUTE, path, &err))?;
    let wanted = match file.metadata() {
        Ok(meta) if meta.is_file() => {
            length.min(usize::try_from(meta.len().saturating_sub(start)).unwrap_or(usize::MAX))
        }
        Ok(meta) if meta.is_dir() => return Err(FsError::is_a_directory(ROUTE, path)),
        _ => length,
    };
    file.seek(SeekFrom::Start(start))
        .map_err(|err| FsError::from_io(ROUTE, path, &err))?;
    let mut bytes = vec![0_u8; wanted];
    let mut filled = 0;
    while filled < wanted {
        let read = file
            .read(&mut bytes[filled..])
            .map_err(|err| FsError::from_io(ROUTE, path, &err))?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    bytes.truncate(filled);
    Ok(Reply::Bytes(bytes))
}

fn find(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:inveni";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let pattern = expect_str(ROUTE, args, 1, "pattern")?;
    let offset = expect_int(ROUTE, args, 2, "offset")?;
    let length = expect_int(ROUTE, args, 3, "length")?;
    let bytes = match read_range(&[
        Arg::Str(path.to_owned()),
        Arg::Int(offset),
        Arg::Int(length),
    ])? {
        Reply::Bytes(bytes) => bytes,
        _ => return Err(FsError::invalid_argument(ROUTE, "range read failed")),
    };
    let needle = pattern.as_bytes();
    if needle.is_empty() {
        return Ok(Reply::Int(offset));
    }
    let found = bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .and_then(|pos| offset.checked_add(i64::try_from(pos).ok()?));
    Ok(Reply::Int(found.unwrap_or(-1)))
}

fn read_lines(args: &[Arg]) -> Result<Reply, FsError> {
    match read_text(args)? {
        Reply::Str(text) => Ok(Reply::Strs(text.lines().map(str::to_owned).collect())),
        other => Ok(other),
    }
}

fn write_text(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:scribe";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let data = expect_str(ROUTE, args, 1, "data")?;
    fs::write(path, data)
        .map(|()| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn write_bytes(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:funde";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let data = expect_bytes(ROUTE, args, 1)?;
    fs::write(path, data)
        .map(|()| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn append_text(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:appone";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let data = expect_str(ROUTE, args, 1, "data")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))?;
    file.write_all(data.as_bytes())
        .map(|()| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn exists(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:exstat";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Bool(Path::new(path).exists()))
}

fn is_directory(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:directoriumne";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Bool(
        fs::metadata(path)
            .map(|meta| meta.is_dir())
            .unwrap_or(false),
    ))
}

fn is_regular(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:regularene";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Bool(
        fs::metadata(path)
            .map(|meta| meta.is_file())
            .unwrap_or(false),
    ))
}

fn is_readable(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:legibilene";
    let path = expect_str(ROUTE, args, 0, "path")?;
    if !Path::new(path)
        .metadata()
        .map(|meta| meta.is_file())
        .unwrap_or(false)
    {
        return Ok(Reply::Bool(false));
    }
    Ok(Reply::Bool(File::open(path).is_ok()))
}

fn is_symlink(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:vinculumne";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Bool(
        fs::symlink_metadata(path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false),
    ))
}

fn file_status(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:status";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let meta = fs::symlink_metadata(path).map_err(|err| FsError::from_io(ROUTE, path, &err))?;
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0);
    let (mode, link_count, owner, group) = unix_status_fields(&meta);
    Ok(Reply::Map(vec![
        ("mode".to_owned(), Reply::Int(mode)),
        ("link_count".to_owned(), Reply::Int(link_count)),
        ("owner".to_owned(), Reply::Int(owner)),
        ("group".to_owned(), Reply::Int(group)),
        (
            "byte_length".to_owned(),
            Reply::Int(i64::try_from(meta.len()).unwrap_or(i64::MAX)),
        ),
        ("modified".to_owned(), Reply::Int(modified_ms)),
        ("is_directory".to_owned(), Reply::Bool(meta.is_dir())),
        (
            "is_symlink".to_owned(),
            Reply::Bool(meta.file_type().is_symlink()),
        ),
    ]))
}

#[cfg(unix)]
fn unix_status_fields(meta: &fs::Metadata) -> (i64, i64, i64, i64) {
    (
        i64::from(meta.mode() & 0o7777),
        i64::try_from(meta.nlink()).unwrap_or(i64::MAX),
        i64::from(meta.uid()),
        i64::from(meta.gid()),
    )
}

#[cfg(not(unix))]
fn unix_status_fields(_meta: &fs::Metadata) -> (i64, i64, i64, i64) {
    (0, 1, 0, 0)
}

fn byte_length(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:mensura";
    let path = expect_str(ROUTE, args, 0, "path")?;
    fs::metadata(path)
        .map(|meta| Reply::Int(i64::try_from(meta.len()).unwrap_or(i64::MAX)))
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn mode(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:modus";
    let path = expect_str(ROUTE, args, 0, "path")?;
    #[cfg(unix)]
    {
        fs::metadata(path)
            .map(|meta| Reply::Int(i64::from(meta.mode() & 0o7777)))
            .map_err(|err| FsError::from_io(ROUTE, path, &err))
    }
    #[cfg(not(unix))]
    {
        Err(FsError::invalid_argument(
            ROUTE,
            format!("mode is unix-only ({path})"),
        ))
    }
}

fn read_link(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:sequere";
    let path = expect_str(ROUTE, args, 0, "path")?;
    fs::read_link(path)
        .map(|target| Reply::Str(target.to_string_lossy().into_owned()))
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn create_link(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:vincula";
    let source = expect_str(ROUTE, args, 0, "source")?;
    let destination = expect_str(ROUTE, args, 1, "destination")?;
    #[cfg(unix)]
    {
        symlink(source, destination)
            .map(|()| Reply::Unit)
            .map_err(|err| FsError::from_io(ROUTE, destination, &err))
    }
    #[cfg(not(unix))]
    {
        Err(FsError::invalid_argument(
            ROUTE,
            format!("create_link is unix-only ({source} -> {destination})"),
        ))
    }
}

fn delete_file(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:dele";
    let path = expect_str(ROUTE, args, 0, "path")?;
    if Path::new(path)
        .symlink_metadata()
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
    {
        return Err(FsError::is_a_directory(ROUTE, path));
    }
    fs::remove_file(path)
        .map(|()| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn copy_file(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:copy";
    let source = expect_str(ROUTE, args, 0, "source")?;
    let destination = expect_str(ROUTE, args, 1, "destination")?;
    fs::copy(source, destination)
        .map(|_| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, source, &err))
}

fn rename_path(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:renomina";
    let source = expect_str(ROUTE, args, 0, "source")?;
    let destination = expect_str(ROUTE, args, 1, "destination")?;
    fs::rename(source, destination)
        .map(|()| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, source, &err))
}

fn touch(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:tange";
    let path = expect_str(ROUTE, args, 0, "path")?;
    if Path::new(path).exists() {
        File::open(path)
            .and_then(|file| file.set_modified(std::time::SystemTime::now()))
            .map(|()| Reply::Unit)
            .map_err(|err| FsError::from_io(ROUTE, path, &err))
    } else {
        fs::write(path, "")
            .map(|()| Reply::Unit)
            .map_err(|err| FsError::from_io(ROUTE, path, &err))
    }
}

fn set_mode(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:modum";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let mode = expect_int(ROUTE, args, 1, "mode")?;
    let mode = u32::try_from(mode)
        .map_err(|_| FsError::invalid_argument(ROUTE, "mode must be non-negative"))?;
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map(|()| Reply::Unit)
            .map_err(|err| FsError::from_io(ROUTE, path, &err))
    }
    #[cfg(not(unix))]
    {
        Err(FsError::invalid_argument(
            ROUTE,
            format!("set_mode is unix-only ({path}, {mode})"),
        ))
    }
}

fn create_dir(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:crea";
    let path = expect_str(ROUTE, args, 0, "path")?;
    fs::create_dir_all(path)
        .map(|()| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn list_dir(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:enumera";
    let path = expect_str(ROUTE, args, 0, "path")?;
    let entries = fs::read_dir(path).map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            FsError::from_io(ROUTE, path, &err)
        } else if Path::new(path)
            .metadata()
            .map(|meta| !meta.is_dir())
            .unwrap_or(false)
        {
            FsError::not_a_directory(ROUTE, path)
        } else {
            FsError::from_io(ROUTE, path, &err)
        }
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| FsError::from_io(ROUTE, path, &err))?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(Reply::Strs(names))
}

fn remove_tree(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:amputa";
    let path = expect_str(ROUTE, args, 0, "path")?;
    fs::remove_dir_all(path)
        .map(|()| Reply::Unit)
        .map_err(|err| FsError::from_io(ROUTE, path, &err))
}

fn join(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:iunge";
    let parts = join_parts(ROUTE, args)?;
    let mut path = PathBuf::new();
    for part in parts {
        path.push(part);
    }
    Ok(Reply::Str(path.to_string_lossy().into_owned()))
}

fn parent(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:parens";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Str(
        Path::new(path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ))
}

fn file_name(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:nomen";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Str(
        Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    ))
}

fn extension(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:suffixum";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Str(
        Path::new(path)
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default(),
    ))
}

fn resolve(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:absolve";
    let path = expect_str(ROUTE, args, 0, "path")?;
    Ok(Reply::Str(
        fs::canonicalize(path)
            .unwrap_or_else(|_| fallback_absolute(Path::new(path)))
            .to_string_lossy()
            .into_owned(),
    ))
}

/// `resolve` fallback when canonicalization fails (e.g. the path does not
/// exist yet): keep an absolute path as-is, otherwise join it onto the
/// current working directory as a best effort.
fn fallback_absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
}

fn home(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:domus";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Str(
        std::env::var("HOME").unwrap_or_else(|_| "/".to_owned()),
    ))
}

fn temp_dir(args: &[Arg]) -> Result<Reply, FsError> {
    const ROUTE: &str = "solum:temporarium";
    empty_ok(ROUTE, args)?;
    Ok(Reply::Str(
        std::env::temp_dir().to_string_lossy().into_owned(),
    ))
}

fn refuse_directory(route: &'static str, path: &str) -> Result<(), FsError> {
    if Path::new(path)
        .metadata()
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
    {
        Err(FsError::is_a_directory(route, path))
    } else {
        Ok(())
    }
}
