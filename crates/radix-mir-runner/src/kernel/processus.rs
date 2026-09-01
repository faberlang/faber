#![allow(clippy::cast_lossless)]

use super::super::value::MapKey;
use super::super::{Host, RunnerError, Value};
use super::args::{expect_str, value_lista_to_strings};
use radix_lexer::Span;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::rc::Rc;

#[expect(
    clippy::cast_possible_truncation,
    reason = "numerus exit code converts to the host's i32 exit channel; wrap on out-of-range codes is the runner's wrap-on-convert behavior"
)]
pub(crate) fn dispatch<H: Host + ?Sized>(
    host: &mut H,
    verb: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, RunnerError> {
    match verb {
        "argv" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(span, "argv provider arity"));
            }
            let values = host
                .argumenta()?
                .iter()
                .map(|arg| Value::Str(arg.clone()))
                .collect::<Vec<_>>();
            Ok(Value::Array(Rc::new(RefCell::new(values))))
        }
        "pid" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(span, "pid provider arity"));
            }
            Ok(Value::Int(host.pid()?))
        }
        "exit_process" => {
            let [code] = args else {
                return Err(RunnerError::internal(span, "exit_process provider arity"));
            };
            let code = match code {
                Value::Int(v) => *v as i32,
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "exit_process code is not numerus",
                    ));
                }
            };
            host.exit(code);
        }
        "read_env" => {
            let [nomen] = args else {
                return Err(RunnerError::internal(
                    span,
                    "processus.read_env provider arity",
                ));
            };
            let nomen = expect_str(nomen, span, "processus.read_env name")?;
            match host.env_get(&nomen)? {
                Some(valor) => Ok(Value::Str(valor)),
                None => Err(RunnerError::internal_with_issue(
                    span,
                    format!("processus.read_env: environment variable `{nomen}` is not set"),
                    "runner_processus_read_env_env_missing",
                )),
            }
        }
        "write_env" => {
            let [nomen, valor] = args else {
                return Err(RunnerError::internal(
                    span,
                    "processus.write_env provider arity",
                ));
            };
            let nomen = expect_str(nomen, span, "processus.write_env name")?;
            let valor = expect_str(valor, span, "processus.write_env value")?;
            host.env_set(&nomen, &valor)?;
            Ok(Value::Unit)
        }
        "cwd" => {
            if !args.is_empty() {
                return Err(RunnerError::internal(span, "processus.cwd provider arity"));
            }
            Ok(Value::Str(host.cwd()?))
        }
        "chdir" => {
            let [via] = args else {
                return Err(RunnerError::internal(
                    span,
                    "processus.chdir provider arity",
                ));
            };
            let via = expect_str(via, span, "processus.chdir path")?;
            host.set_cwd(&via)?;
            Ok(Value::Unit)
        }
        "exec" => {
            let [imperium] = args else {
                return Err(RunnerError::internal(span, "processus.exec provider arity"));
            };
            let imperium = expect_str(imperium, span, "processus.exec command")?;
            execute_shell(&imperium, span).map(Value::Str)
        }
        "spawn" => {
            let [argumenta, directorium, ambitus] = args else {
                return Err(RunnerError::internal(
                    span,
                    "processus.spawn provider arity",
                ));
            };
            let command_args = value_lista_to_strings(argumenta, span)?;
            if command_args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "processus.spawn requires argumenta",
                ));
            }
            let directorium = match directorium {
                Value::Nil | Value::Unit => None,
                other => Some(expect_str(other, span, "processus.spawn directorium")?),
            };
            let env = match ambitus {
                Value::Nil | Value::Unit => None,
                Value::Map(map) => {
                    let mut out = HashMap::new();
                    for (key, value) in map.borrow().iter() {
                        let super::super::value::MapKey::Str(name) = key else {
                            return Err(RunnerError::internal(
                                span,
                                "processus.spawn ambitus keys must be textus",
                            ));
                        };
                        out.insert(
                            name.clone(),
                            expect_str(value, span, "processus.spawn ambitus value")?,
                        );
                    }
                    Some(out)
                }
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "processus.spawn ambitus expects tabula",
                    ));
                }
            };
            spawn_attached(&command_args, directorium.as_deref(), env.as_ref(), span)
        }
        "captura" => {
            let [argumenta] = args else {
                return Err(RunnerError::internal(
                    span,
                    "processus.captura provider arity",
                ));
            };
            let command_args = value_lista_to_strings(argumenta, span)?;
            if command_args.is_empty() {
                return Err(RunnerError::internal(
                    span,
                    "processus.captura requires argumenta",
                ));
            }
            capture_process(&command_args, span)
        }
        other => Err(RunnerError::unsupported(
            span,
            &format!("kernel processus verb `{other}`"),
        )),
    }
}

fn execute_shell(command: &str, span: Span) -> Result<String, RunnerError> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|err| RunnerError::internal(span, format!("processus.exec failed: {err}")))?;
    // F-085: propagate nonzero exit status so callers cannot silently ignore
    // script failures. stdout is still returned on success.
    if !output.status.success() {
        return Err(RunnerError::internal(
            span,
            format!(
                "processus.exec exit non-zero ({}): {}",
                output.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn spawn_attached(
    argv: &[String],
    cwd: Option<&str>,
    env: Option<&HashMap<String, String>>,
    span: Span,
) -> Result<Value, RunnerError> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = env {
        command.envs(env);
    }
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    let mut child = command
        .spawn()
        .map_err(|err| RunnerError::internal(span, format!("processus.spawn failed: {err}")))?;
    let pid = child.id() as i64;
    let status = child.wait().map_err(|err| {
        RunnerError::internal(span, format!("processus.spawn wait failed: {err}"))
    })?;
    let mut fields = FxHashMap::default();
    fields.insert(MapKey::Str("pid".to_owned()), Value::Int(pid));
    fields.insert(
        MapKey::Str("status".to_owned()),
        Value::Int(status.code().unwrap_or(-1) as i64),
    );
    Ok(Value::Map(Rc::new(RefCell::new(fields))))
}

fn capture_process(argv: &[String], span: Span) -> Result<Value, RunnerError> {
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|err| RunnerError::internal(span, format!("processus.captura failed: {err}")))?;
    let mut fields = FxHashMap::default();
    fields.insert(
        MapKey::Str("status".to_owned()),
        Value::Int(output.status.code().unwrap_or(-1) as i64),
    );
    fields.insert(
        MapKey::Str("stdout".to_owned()),
        Value::Str(String::from_utf8_lossy(&output.stdout).into_owned()),
    );
    fields.insert(
        MapKey::Str("stderr".to_owned()),
        Value::Str(String::from_utf8_lossy(&output.stderr).into_owned()),
    );
    Ok(Value::Map(Rc::new(RefCell::new(fields))))
}
