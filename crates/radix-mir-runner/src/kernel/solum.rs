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
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::UNIX_EPOCH;

pub(crate) fn dispatch(verb: &str, args: &[Value], span: Span) -> Result<Value, RunnerError> {
    match verb {
        "lege" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.lege provider arity"));
            };
            let path = expect_str(via, span, "solum.lege path")?;
            read_text(&path, span).map(Value::Str)
        }
        "hauri" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.hauri provider arity"));
            };
            let path = expect_str(via, span, "solum.hauri path")?;
            read_bytes(&path, span).map(Value::Octeti)
        }
        "partem" => {
            let [via, initium, longitudo] = args else {
                return Err(RunnerError::internal(span, "solum.partem provider arity"));
            };
            let path = expect_str(via, span, "solum.partem path")?;
            let Value::Int(initium) = initium else {
                return Err(RunnerError::internal(
                    span,
                    "solum.partem initium expects numerus",
                ));
            };
            let Value::Int(longitudo) = longitudo else {
                return Err(RunnerError::internal(
                    span,
                    "solum.partem longitudo expects numerus",
                ));
            };
            read_byte_range(&path, *initium, *longitudo, span).map(Value::Octeti)
        }
        "carpe" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.carpe provider arity"));
            };
            let path = expect_str(via, span, "solum.carpe path")?;
            read_text(&path, span)
                .map(|content| strings_to_value_lista(content.lines().map(str::to_owned).collect()))
        }
        "scribe" => {
            let [via, data] = args else {
                return Err(RunnerError::internal(span, "solum.scribe provider arity"));
            };
            let path = expect_str(via, span, "solum.scribe path")?;
            let data = expect_str(data, span, "solum.scribe data")?;
            fs::write(&path, data).map_err(|err| {
                RunnerError::internal(span, format!("solum.scribe failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "funde" => {
            let [via, data] = args else {
                return Err(RunnerError::internal(span, "solum.funde provider arity"));
            };
            let path = expect_str(via, span, "solum.funde path")?;
            let Value::Octeti(bytes) = data else {
                return Err(RunnerError::internal(
                    span,
                    "solum.funde data expects octeti",
                ));
            };
            fs::write(&path, bytes).map_err(|err| {
                RunnerError::internal(span, format!("solum.funde failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "appone" => {
            let [via, data] = args else {
                return Err(RunnerError::internal(span, "solum.appone provider arity"));
            };
            let path = expect_str(via, span, "solum.appone path")?;
            let data = expect_str(data, span, "solum.appone data")?;
            append_text(&path, &data, span)?;
            Ok(Value::Unit)
        }
        "exstat" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.exstat provider arity"));
            };
            Ok(Value::Bool(
                Path::new(&expect_str(via, span, "solum.exstat path")?).exists(),
            ))
        }
        "directoriumne" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.directoriumne provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.directoriumne path")?;
            Ok(Value::Bool(
                fs::metadata(path)
                    .map(|meta| meta.is_dir())
                    .unwrap_or(false),
            ))
        }
        "regularene" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.regularene provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.regularene path")?;
            Ok(Value::Bool(
                fs::metadata(path)
                    .map(|meta| meta.is_file())
                    .unwrap_or(false),
            ))
        }
        "vinculumne" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.vinculumne provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.vinculumne path")?;
            Ok(Value::Bool(
                fs::symlink_metadata(path)
                    .map(|meta| meta.file_type().is_symlink())
                    .unwrap_or(false),
            ))
        }
        "mensura" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.mensura provider arity"));
            };
            let path = expect_str(via, span, "solum.mensura path")?;
            let size = fs::metadata(path)
                .map(|meta| meta.len() as i64)
                .map_err(|err| {
                    RunnerError::internal(span, format!("solum.mensura failed: {err}"))
                })?;
            Ok(Value::Int(size))
        }
        "modus" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.modus provider arity"));
            };
            let path = expect_str(via, span, "solum.modus path")?;
            let mode = fs::symlink_metadata(path)
                .map(|meta| (meta.mode() & 0o7777) as i64)
                .map_err(|err| {
                    RunnerError::internal(span, format!("solum.modus failed: {err}"))
                })?;
            Ok(Value::Int(mode))
        }
        "enumera" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.enumera provider arity"));
            };
            let path = expect_str(via, span, "solum.enumera path")?;
            list_dir(&path, span).map(strings_to_value_lista)
        }
        "describe" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.describe provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.describe path")?;
            describe(&path, span)
        }
        "sequere" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.sequere provider arity"));
            };
            let path = expect_str(via, span, "solum.sequere path")?;
            read_link(&path, span).map(Value::Str)
        }
        "vincula" => {
            let [fons, finis] = args else {
                return Err(RunnerError::internal(span, "solum.vincula provider arity"));
            };
            let source = expect_str(fons, span, "solum.vincula source")?;
            let destination = expect_str(finis, span, "solum.vincula destination")?;
            symlink(source, destination).map_err(|err| {
                RunnerError::internal(span, format!("solum.vincula failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "dele" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.dele provider arity"));
            };
            let path = expect_str(via, span, "solum.dele path")?;
            remove_file(&path, span)?;
            Ok(Value::Unit)
        }
        "exscribe" => {
            let [origo, finis] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.exscribe provider arity",
                ));
            };
            let source = expect_str(origo, span, "solum.exscribe source")?;
            let destination = expect_str(finis, span, "solum.exscribe destination")?;
            copy_file(&source, &destination, span)?;
            Ok(Value::Unit)
        }
        "renomina" => {
            let [origo, finis] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.renomina provider arity",
                ));
            };
            let source = expect_str(origo, span, "solum.renomina source")?;
            let destination = expect_str(finis, span, "solum.renomina destination")?;
            rename_path(&source, &destination, span)?;
            Ok(Value::Unit)
        }
        "tange" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.tange provider arity"));
            };
            let path = expect_str(via, span, "solum.tange path")?;
            touch_file(&path, span)?;
            Ok(Value::Unit)
        }
        "modum" => {
            let [via, modus] = args else {
                return Err(RunnerError::internal(span, "solum.modum provider arity"));
            };
            let path = expect_str(via, span, "solum.modum path")?;
            let mode = expect_mode(modus, span)?;
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|err| {
                RunnerError::internal(span, format!("solum.modum failed: {err}"))
            })?;
            Ok(Value::Unit)
        }
        "crea" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.crea provider arity"));
            };
            let path = expect_str(via, span, "solum.crea path")?;
            create_dir_all(&path, span)?;
            Ok(Value::Unit)
        }
        "amputa" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.amputa provider arity"));
            };
            let path = expect_str(via, span, "solum.amputa path")?;
            remove_dir_all(&path, span)?;
            Ok(Value::Unit)
        }
        "iunge" => {
            let [partes] = args else {
                return Err(RunnerError::internal(span, "solum.iunge provider arity"));
            };
            let path_parts = value_lista_to_strings(partes, span)?;
            let mut path = PathBuf::new();
            for part in path_parts {
                path.push(part);
            }
            Ok(Value::Str(path.to_string_lossy().into_owned()))
        }
        "parens" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.parens provider arity"));
            };
            let path = expect_str(via, span, "solum.parens path")?;
            Ok(Value::Str(
                Path::new(&path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "nomen" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.nomen provider arity"));
            };
            let path = expect_str(via, span, "solum.nomen path")?;
            Ok(Value::Str(
                Path::new(&path)
                    .file_name()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "suffixum" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.suffixum provider arity",
                ));
            };
            let path = expect_str(via, span, "solum.suffixum path")?;
            Ok(Value::Str(
                Path::new(&path)
                    .extension()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ))
        }
        "absolve" => {
            let [via] = args else {
                return Err(RunnerError::internal(span, "solum.absolve provider arity"));
            };
            let path = expect_str(via, span, "solum.absolve path")?;
            Ok(Value::Str(
                fs::canonicalize(&path)
                    .unwrap_or_else(|_| PathBuf::from(path))
                    .to_string_lossy()
                    .into_owned(),
            ))
        }
        "domus" => {
            let [] = args else {
                return Err(RunnerError::internal(span, "solum.domus provider arity"));
            };
            Ok(Value::Str(
                std::env::var("HOME").unwrap_or_else(|_| "/".to_owned()),
            ))
        }
        "temporarium" => {
            let [] = args else {
                return Err(RunnerError::internal(
                    span,
                    "solum.temporarium provider arity",
                ));
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
        .map_err(|err| RunnerError::internal(span, format!("solum.lege failed: {err}")))
}

fn read_bytes(path: &str, span: Span) -> Result<Vec<u8>, RunnerError> {
    fs::read(path).map_err(|err| RunnerError::internal(span, format!("solum.hauri failed: {err}")))
}

fn read_byte_range(
    path: &str,
    start: i64,
    length: i64,
    span: Span,
) -> Result<Vec<u8>, RunnerError> {
    const MAX_RANGE_READ_BYTES: usize = 64 * 1024 * 1024;
    let start = u64::try_from(start)
        .map_err(|_| RunnerError::internal(span, "solum.partem initium must be non-negative"))?;
    let length = usize::try_from(length)
        .map_err(|_| RunnerError::internal(span, "solum.partem longitudo must be non-negative"))?;
    if length > MAX_RANGE_READ_BYTES {
        return Err(RunnerError::internal(
            span,
            format!("solum.partem longitudo must be at most {MAX_RANGE_READ_BYTES} bytes"),
        ));
    }
    let mut file = File::open(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.partem open failed: {err}")))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|err| RunnerError::internal(span, format!("solum.partem seek failed: {err}")))?;
    let mut bytes = Vec::with_capacity(length);
    file.take(length as u64)
        .read_to_end(&mut bytes)
        .map_err(|err| RunnerError::internal(span, format!("solum.partem read failed: {err}")))?;
    Ok(bytes)
}

#[cfg(test)]
mod range_tests {
    use super::read_byte_range;
    use radix_lexer::Span;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path(suffix: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should follow the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "radix-solum-partem-{}-{nonce}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn range_read_returns_exact_bytes_and_a_short_read_at_eof() {
        let path = unique_path("artifact");
        fs::write(&path, b"abcdefgh").expect("write range fixture");

        let path_text = path.to_string_lossy();
        let first = read_byte_range(&path_text, 2, 3, Span::new(0, 0)).expect("first range");
        let second = read_byte_range(&path_text, 6, 4, Span::new(0, 0)).expect("second range");
        assert_eq!(first, b"cde");
        assert_eq!(second, b"gh");

        fs::remove_file(path).expect("remove range fixture");
    }
}

fn expect_mode(value: &Value, span: Span) -> Result<u32, RunnerError> {
    match value {
        Value::Int(mode) => u32::try_from(*mode).map_err(|_| {
            RunnerError::internal(span, "solum.modum mode expects non-negative numerus")
        }),
        _ => Err(RunnerError::internal(
            span,
            "solum.modum mode expects non-negative numerus",
        )),
    }
}

fn append_text(path: &str, data: &str, span: Span) -> Result<(), RunnerError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.appone open failed: {err}")))?;
    file.write_all(data.as_bytes())
        .map_err(|err| RunnerError::internal(span, format!("solum.appone write failed: {err}")))
}

fn read_link(path: &str, span: Span) -> Result<String, RunnerError> {
    fs::read_link(path)
        .map(|target| target.to_string_lossy().into_owned())
        .map_err(|err| RunnerError::internal(span, format!("solum.sequere failed: {err}")))
}

fn remove_file(path: &str, span: Span) -> Result<(), RunnerError> {
    fs::remove_file(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.dele failed: {err}")))
}

fn copy_file(source: &str, destination: &str, span: Span) -> Result<(), RunnerError> {
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|err| RunnerError::internal(span, format!("solum.exscribe failed: {err}")))
}

fn rename_path(source: &str, destination: &str, span: Span) -> Result<(), RunnerError> {
    fs::rename(source, destination)
        .map_err(|err| RunnerError::internal(span, format!("solum.renomina failed: {err}")))
}

fn touch_file(path: &str, span: Span) -> Result<(), RunnerError> {
    if Path::new(path).exists() {
        OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|file| file.sync_all())
            .map_err(|err| RunnerError::internal(span, format!("solum.tange failed: {err}")))?;
    } else {
        fs::write(path, "")
            .map_err(|err| RunnerError::internal(span, format!("solum.tange failed: {err}")))?;
    }
    Ok(())
}

fn create_dir_all(path: &str, span: Span) -> Result<(), RunnerError> {
    fs::create_dir_all(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.crea failed: {err}")))
}

fn remove_dir_all(path: &str, span: Span) -> Result<(), RunnerError> {
    fs::remove_dir_all(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.amputa failed: {err}")))
}

fn list_dir(path: &str, span: Span) -> Result<Vec<String>, RunnerError> {
    let entries = fs::read_dir(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.enumera failed: {err}")))?;
    entries
        .map(|entry| {
            entry
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .map_err(|err| {
                    RunnerError::internal(span, format!("solum.enumera entry failed: {err}"))
                })
        })
        .collect()
}

fn describe(path: &str, span: Span) -> Result<Value, RunnerError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|err| RunnerError::internal(span, format!("solum.describe failed: {err}")))?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let mut fields = FxHashMap::default();
    fields.insert(
        MapKey::Str("modus".to_owned()),
        Value::Int(meta.mode() as i64),
    );
    fields.insert(
        MapKey::Str("nexus".to_owned()),
        Value::Int(meta.nlink() as i64),
    );
    fields.insert(
        MapKey::Str("possessor".to_owned()),
        Value::Int(meta.uid() as i64),
    );
    fields.insert(
        MapKey::Str("grex".to_owned()),
        Value::Int(meta.gid() as i64),
    );
    fields.insert(
        MapKey::Str("mensura".to_owned()),
        Value::Int(meta.len() as i64),
    );
    fields.insert(MapKey::Str("modificatum".to_owned()), Value::Int(modified));
    fields.insert(
        MapKey::Str("directoriumne".to_owned()),
        Value::Bool(meta.is_dir()),
    );
    fields.insert(
        MapKey::Str("vinculumne".to_owned()),
        Value::Bool(meta.file_type().is_symlink()),
    );
    Ok(Value::Map(Rc::new(RefCell::new(fields))))
}
