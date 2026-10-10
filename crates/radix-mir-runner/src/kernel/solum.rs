#![allow(
    clippy::cast_lossless,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::map_unwrap_or,
    clippy::unused_self
)]
// The solum kernel surfaces host file metadata and durations as `Value::Int`;
// wrap/truncation only occur beyond the i64 host range.
#![expect(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    reason = "file metadata and durations surface as Value::Int numerus"
)]

use super::super::value::MapKey;
use super::super::{RunnerError, Value};
use super::args::{expect_str, strings_to_value_lista, value_lista_to_strings};
use radix_lexer::Span;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::UNIX_EPOCH;

pub(crate) fn dispatch(verb: &str, args: &[Value], span: Span) -> Result<Value, RunnerError> {
    match verb {
        "read_file" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.read_file provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.read_file path")?;
            read_text(&path, span).map(Value::Str)
        }
        "read_bytes" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.read_bytes provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.read_bytes path")?;
            read_bytes(&path, span).map(Value::Octeti)
        }
        "digest_file" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.digest_file provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.digest_file path")?;
            digest_file(&path, span).map(Value::Str)
        }
        "read_range" => {
            let [via, initium, longitudo] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.read_range provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.read_range path")?;
            let Value::Int(initium) = initium else {
                return Err(RunnerError::internal(
                    span,
                    "solum.read_range initium expects numerus",
                ));
            };
            let Value::Int(longitudo) = longitudo else {
                return Err(RunnerError::internal(
                    span,
                    "solum.read_range longitudo expects numerus",
                ));
            };
            read_byte_range(&path, *initium, *longitudo, span).map(Value::Octeti)
        }
        "read_lines" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.read_lines provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.read_lines path")?;
            read_text(&path, span)
                .map(|content| strings_to_value_lista(content.lines().map(str::to_owned).collect()))
        }
        "write_file" => {
            let [via, data] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.write_file provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.write_file path")?;
            let data = expect_str(data, span, "solum.write_file data")?;
            fs::write(&path, data).map_err(|err| {
                RunnerError::internal(span, format!("solum.write_file failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "write_bytes" => {
            let [via, data] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.write_bytes provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.write_bytes path")?;
            let Some(bytes) = octeti_bytes(data) else {
                return Err(RunnerError::internal(
                    span,
                    "solum.write_bytes data expects octeti",
                ));
            };
            fs::write(&path, bytes).map_err(|err| {
                RunnerError::internal(span, format!("solum.write_bytes failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "append" => {
            let [via, data] = args else {
                return Err(RunnerError::internal(span, "solum.append provider arity"));
            };
            let path = expect_str(via, span, "solum.append path")?;
            let data = expect_str(data, span, "solum.append data")?;
            append_text(&path, &data, span)?;
            Ok(Value::Unit)
        }
        "exists" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.exists provider arity"));
            };
            Ok(Value::Bool(
                Path::new(&expect_str(via, span, "solum.exists path")?).exists(),
            ))
        }
        "is_directory" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.is_directory provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.is_directory path")?;
            Ok(Value::Bool(
                fs::metadata(path)
                    .map(|meta| meta.is_dir())
                    .unwrap_or(false),
            ))
        }
        "is_regular" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.is_regular provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.is_regular path")?;
            Ok(Value::Bool(
                fs::metadata(path)
                    .map(|meta| meta.is_file())
                    .unwrap_or(false),
            ))
        }
        "is_symlink" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.is_symlink provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.is_symlink path")?;
            Ok(Value::Bool(
                fs::symlink_metadata(path)
                    .map(|meta| meta.file_type().is_symlink())
                    .unwrap_or(false),
            ))
        }
        "byte_length" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.byte_length provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.byte_length path")?;
            let size = fs::metadata(path)
                .map(|meta| meta.len() as i64)
                .map_err(|err| {
                    RunnerError::internal(span, format!("solum.byte_length failed: {err}"))
                })?;
            Ok(Value::Int(size))
        }
        "mode" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.mode provider arity"));
            };
            let path = expect_str(via, span, "solum.mode path")?;
            let mode = fs::symlink_metadata(path)
                .map(|meta| (meta.mode() & 0o7777) as i64)
                .map_err(|err| RunnerError::internal(span, format!("solum.mode failed: {err}")))?;
            Ok(Value::Int(mode))
        }
        "list_dir" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.list_dir provider arity"));
            };
            let path = expect_str(via, span, "solum.list_dir path")?;
            list_dir(&path, span).map(strings_to_value_lista)
        }
        "stat" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.stat provider arity"));
            };
            let path = expect_str(via, span, "solum.stat path")?;
            stat(&path, span)
        }
        "read_link" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.read_link provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.read_link path")?;
            read_link(&path, span).map(Value::Str)
        }
        "create_link" => {
            let [fons, finis] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.create_link provider arity",
                ));
            };
            let source = expect_str(fons, span, "solum.create_link source")?;
            let destination = expect_str(finis, span, "solum.create_link destination")?;
            symlink(source, destination).map_err(|err| {
                RunnerError::internal(span, format!("solum.create_link failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "delete" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.delete provider arity"));
            };
            let path = expect_str(via, span, "solum.delete path")?;
            remove_file(&path, span)?;
            Ok(Value::Unit)
        }
        "copy" => {
            let [origo, finis] = args else {
                return Err(RunnerError::internal(span, "solum.copy provider arity"));
            };
            let source = expect_str(origo, span, "solum.copy source")?;
            let destination = expect_str(finis, span, "solum.copy destination")?;
            copy_file(&source, &destination, span)?;
            Ok(Value::Unit)
        }
        "rename" => {
            let [origo, finis] = args else {
                return Err(RunnerError::internal(span, "solum.rename provider arity"));
            };
            let source = expect_str(origo, span, "solum.rename source")?;
            let destination = expect_str(finis, span, "solum.rename destination")?;
            rename_path(&source, &destination, span)?;
            Ok(Value::Unit)
        }
        "touch" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.touch provider arity"));
            };
            let path = expect_str(via, span, "solum.touch path")?;
            touch_file(&path, span)?;
            Ok(Value::Unit)
        }
        "set_mode" => {
            let [via, modus] = args else {
                return Err(RunnerError::internal(span, "solum.set_mode provider arity"));
            };
            let path = expect_str(via, span, "solum.set_mode path")?;
            let mode = expect_mode(modus, span)?;
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|err| {
                RunnerError::internal(span, format!("solum.set_mode failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "create_dir" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.create_dir provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.create_dir path")?;
            create_dir_all(&path, span)?;
            Ok(Value::Unit)
        }
        "remove_tree" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.remove_tree provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.remove_tree path")?;
            remove_dir_all(&path, span)?;
            Ok(Value::Unit)
        }
        "join" => {
            let [partes] = args else {
                return Err(RunnerError::internal(span, "solum.join provider arity"));
            };
            let path_parts = value_lista_to_strings(partes, span)?;
            let mut path = PathBuf::new();
            for part in path_parts {
                path.push(part);
            }
            Ok(Value::Str(path.to_string_lossy().into_owned()))
        }
        "parent" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.parent provider arity"));
            };
            let path = expect_str(via, span, "solum.parent path")?;
            Ok(Value::Str(
                Path::new(&path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "file_name" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.file_name provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.file_name path")?;
            Ok(Value::Str(
                Path::new(&path)
                    .file_name()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "extension" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.extension provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.extension path")?;
            Ok(Value::Str(
                Path::new(&path)
                    .extension()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "resolve" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.resolve provider arity"));
            };
            let path = expect_str(via, span, "solum.resolve path")?;
            Ok(Value::Str(
                fs::canonicalize(&path)
                    .unwrap_or_else(|_| PathBuf::from(path))
                    .to_string_lossy()
                    .into_owned(),
            ))
        }
        "home" => {
            let [] = args else {
                return Err(RunnerError::internal(span, "solum.home provider arity"));
            };
            Ok(Value::Str(
                std::env::var("HOME").unwrap_or_else(|_| "/".to_owned()),
            ))
        }
        "temp_dir" => {
            let [] = args else {
                return Err(RunnerError::internal(span, "solum.temp_dir provider arity"));
            };
            Ok(Value::Str(
                std::env::temp_dir().to_string_lossy().into_owned(),
            ))
        }
        other => Err(RunnerError::unsupported(
            span,
            &format!("kernel solum verb `{other}`"),
        )),
    }
}

fn read_text(path: &str, span: Span) -> Result<String, RunnerError> {
    fs::read_to_string(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.read_file failed: {err}")))
}

fn read_bytes(path: &str, span: Span) -> Result<Vec<u8>, RunnerError> {
    fs::read(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.read_bytes failed: {err}")))
}

/// Streaming SHA-256 of a file as 64 lowercase hex digits (never loads the
/// file whole). Copy of the retired `hosts` `solum:digestio` semantics.
fn digest_file(path: &str, span: Span) -> Result<String, RunnerError> {
    let mut file = File::open(path).map_err(|err| {
        RunnerError::internal(span, format!("solum.digest_file open failed: {err}"))
    })?;
    let mut hasher = handroll::Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).map_err(|err| {
            RunnerError::internal(span, format!("solum.digest_file read failed: {err}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.finalize_hex())
}

/// Largest single `read_range` request, in bytes (512 MiB).
///
/// WHY: the A1 weight loader reads each tensor's byte range with ONE call, and
/// the largest `SmolLM2` F32 tensor, `token_embd.weight`, is 49152 x 960 x 4 =
/// 188,743,680 bytes, which the former 64 MiB ceiling refused. This is an
/// implementation ceiling, not a language constant (numeric-model.md: resource
/// bounds are implementation ceilings); it bounds one read's allocation. The
/// same constant serves the raw `solum:partem` route, which dispatches here.
pub(crate) const MAX_RANGE_READ_BYTES: usize = 512 * 1024 * 1024;

fn read_byte_range(
    path: &str,
    start: i64,
    length: i64,
    span: Span,
) -> Result<Vec<u8>, RunnerError> {
    let start = u64::try_from(start).map_err(|_| {
        RunnerError::internal(span, "solum.read_range initium must be non-negative")
    })?;
    let length = usize::try_from(length).map_err(|_| {
        RunnerError::internal(span, "solum.read_range longitudo must be non-negative")
    })?;
    if length > MAX_RANGE_READ_BYTES {
        return Err(RunnerError::internal(
            span,
            format!("solum.read_range longitudo must be at most {MAX_RANGE_READ_BYTES} bytes"),
        ));
    }
    let mut file = File::open(path).map_err(|err| {
        RunnerError::internal(span, format!("solum.read_range open failed: {err}"))
    })?;
    // A regular file is clamped to its remaining bytes so a short read at EOF
    // allocates only what exists; other files keep the requested length.
    let wanted = match file.metadata() {
        Ok(meta) if meta.is_file() => {
            length.min(usize::try_from(meta.len().saturating_sub(start)).unwrap_or(usize::MAX))
        }
        _ => length,
    };
    file.seek(SeekFrom::Start(start)).map_err(|err| {
        RunnerError::internal(span, format!("solum.read_range seek failed: {err}"))
    })?;
    // One allocation, filled in place by the kernel read: the bytes are copied
    // once (file to this Vec) and then moved into `Value::Octeti`.
    let mut bytes = vec![0_u8; wanted];
    let mut filled = 0;
    while filled < wanted {
        let read = file.read(&mut bytes[filled..]).map_err(|err| {
            RunnerError::internal(span, format!("solum.read_range read failed: {err}"))
        })?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    bytes.truncate(filled);
    Ok(bytes)
}

fn expect_mode(value: &Value, span: Span) -> Result<u32, RunnerError> {
    match value {
        Value::Int(mode) => u32::try_from(*mode).map_err(|_| {
            RunnerError::internal(span, "solum.set_mode mode expects non-negative numerus")
        }),
        _ => Err(RunnerError::internal(
            span,
            "solum.set_mode mode expects non-negative numerus",
        )),
    }
}

fn append_text(path: &str, data: &str, span: Span) -> Result<(), RunnerError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.append open failed: {err}")))?;
    file.write_all(data.as_bytes())
        .map_err(|err| RunnerError::internal(span, format!("solum.append write failed: {err}")))
}

fn read_link(path: &str, span: Span) -> Result<String, RunnerError> {
    fs::read_link(path)
        .map(|target| target.to_string_lossy().into_owned())
        .map_err(|err| RunnerError::internal(span, format!("solum.read_link failed: {err}")))
}

fn remove_file(path: &str, span: Span) -> Result<(), RunnerError> {
    fs::remove_file(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.delete failed: {err}")))
}

fn copy_file(source: &str, destination: &str, span: Span) -> Result<(), RunnerError> {
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|err| RunnerError::internal(span, format!("solum.copy failed: {err}")))
}

fn rename_path(source: &str, destination: &str, span: Span) -> Result<(), RunnerError> {
    fs::rename(source, destination)
        .map_err(|err| RunnerError::internal(span, format!("solum.rename failed: {err}")))
}

fn touch_file(path: &str, span: Span) -> Result<(), RunnerError> {
    if Path::new(path).exists() {
        OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|file| file.sync_all())
            .map_err(|err| RunnerError::internal(span, format!("solum.touch failed: {err}")))?;
    } else {
        fs::write(path, "")
            .map_err(|err| RunnerError::internal(span, format!("solum.touch failed: {err}")))?;
    }
    Ok(())
}

fn create_dir_all(path: &str, span: Span) -> Result<(), RunnerError> {
    fs::create_dir_all(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.create_dir failed: {err}")))
}

fn remove_dir_all(path: &str, span: Span) -> Result<(), RunnerError> {
    fs::remove_dir_all(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.remove_tree failed: {err}")))
}

fn list_dir(path: &str, span: Span) -> Result<Vec<String>, RunnerError> {
    let entries = fs::read_dir(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.list_dir failed: {err}")))?;
    entries
        .map(|entry| {
            entry
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .map_err(|err| {
                    RunnerError::internal(span, format!("solum.list_dir entry failed: {err}"))
                })
        })
        .collect()
}

fn stat(path: &str, span: Span) -> Result<Value, RunnerError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.stat failed: {err}")))?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let mut fields = FxHashMap::default();
    fields.insert(
        MapKey::Str("mode".to_owned()),
        Value::Int(meta.mode() as i64),
    );
    fields.insert(
        MapKey::Str("link_count".to_owned()),
        Value::Int(meta.nlink() as i64),
    );
    fields.insert(
        MapKey::Str("owner".to_owned()),
        Value::Int(meta.uid() as i64),
    );
    fields.insert(
        MapKey::Str("group".to_owned()),
        Value::Int(meta.gid() as i64),
    );
    fields.insert(
        MapKey::Str("byte_length".to_owned()),
        Value::Int(meta.len() as i64),
    );
    fields.insert(MapKey::Str("modified".to_owned()), Value::Int(modified));
    fields.insert(
        MapKey::Str("is_directory".to_owned()),
        Value::Bool(meta.is_dir()),
    );
    fields.insert(
        MapKey::Str("is_symlink".to_owned()),
        Value::Bool(meta.file_type().is_symlink()),
    );
    Ok(Value::Map(Rc::new(RefCell::new(fields))))
}

/// The bytes of an `octeti` argument: the byte value itself, or the
/// `lista<b8>` array a bit-pattern list arrives as (every element one
/// unsigned pattern below 256).
pub(super) fn octeti_bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Octeti(bytes) => Some(bytes.clone()),
        Value::Array(items) => items
            .borrow()
            .iter()
            .map(|item| match item {
                Value::UInt(v) => u8::try_from(*v).ok(),
                Value::Int(v) => u8::try_from(*v).ok(),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

#[cfg(test)]
#[path = "solum_test.rs"]
mod tests;
