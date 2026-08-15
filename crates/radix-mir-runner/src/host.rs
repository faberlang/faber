#![allow(clippy::needless_lifetimes)]

use super::{kernel, RunnerError, Value};
use radix_lexer::Span;
use radix_mir::KernelModule;
use radix_mir::{MirDiagnosticKind, MirProvider};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::io::{self, BufRead, Write};

/// Host seam for diagnostics and capability expansion.
///
/// The trait is the sole process-capability surface the runner reaches
/// through. Default implementations fail-closed so that unsupported
/// operations reject explicitly. Hosts that expose a capability opt in by
/// overriding.
pub trait Host {
    #[allow(clippy::missing_errors_doc)]
    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), RunnerError>;
    #[allow(clippy::missing_errors_doc)]
    fn read_line(&mut self) -> Result<Option<String>, RunnerError>;
    fn abort(&mut self, reason: &str) -> !;
    #[allow(clippy::missing_errors_doc)]
    fn provider(&mut self, provider: &MirProvider) -> Result<Value, RunnerError>;
    #[allow(clippy::missing_errors_doc)]
    fn kernel_call(
        &mut self,
        _module: KernelModule,
        _verb: &str,
        _args: &[Value],
        _span: Span,
    ) -> Result<Value, RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            "host kernel_call",
        ))
    }
    fn exit(&mut self, code: i32) -> !;

    /// Write text to stdout **without** appending a newline (`consolum.dic`).
    ///
    /// Default hosts reject the surface so callers fail closed until a real
    /// console-capable host opts in. `StdioHost` writes immediately;
    /// `BufferHost` accumulates into `pending_stdout` until a line scribe
    /// flushes it.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying write fails.
    fn write_stdout_raw(&mut self, _text: &str) -> Result<(), RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            "host write_stdout_raw (consolum.dic)",
        ))
    }

    /// Script arguments forwarded after `faber run script.fab -- …`.
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose process arguments.
    fn argumenta(&self) -> Result<&[String], RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host argumenta"))
    }

    /// Read an environment variable (`processus.lege`). Hosts that do not
    /// expose process env return `Err`. `Ok(None)` means the variable is not
    /// set; `Ok(Some(v))` returns the value.
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose environment access.
    fn env_get(&self, _name: &str) -> Result<Option<String>, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host env_get"))
    }

    /// Write an environment variable (`processus.scribe`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose environment mutation.
    fn env_set(&mut self, _name: &str, _value: &str) -> Result<(), RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host env_set"))
    }

    /// Current working directory (`processus.sedes`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose a filesystem.
    fn cwd(&self) -> Result<String, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host cwd"))
    }

    /// Change the current working directory (`processus.muta`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose a filesystem.
    fn set_cwd(&mut self, _path: &str) -> Result<(), RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host set_cwd"))
    }

    /// Process id (`identitas`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose a real process id.
    fn pid(&self) -> Result<i64, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host pid"))
    }
}

/// Test/embedder host: buffers diagnostics and isolates process state.
///
/// The buffer is the only process-like state this host holds. Env reads/writes
/// hit an in-memory map and the cwd is a virtual string, so driving a script
/// through `BufferHost` never touches the real process environment.
#[derive(Debug, Default)]
pub struct BufferHost {
    pub stdout_lines: Vec<String>,
    pub stderr_lines: Vec<String>,
    /// Incomplete stdout from `write_stdout_raw` not yet closed by a newline
    /// scribe (`nota` / `consolum.scribe`).
    pub pending_stdout: String,
    pub stdin_lines: VecDeque<String>,
    pub argumenta: Vec<String>,
    pub env: FxHashMap<String, String>,
    pub cwd: String,
    /// Last `abort` reason observed before the host panicked (test seam).
    pub last_abort: Option<String>,
}

impl BufferHost {
    #[must_use]
    pub fn with_argumenta(argumenta: Vec<String>) -> Self {
        Self {
            argumenta,
            ..Self::default()
        }
    }

    /// Seed an environment variable for `processus.lege` assertions.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Seed the virtual working directory returned by `processus.sedes`.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = cwd.into();
        self
    }

    #[must_use]
    pub fn with_stdin_lines(mut self, lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.stdin_lines = lines.into_iter().map(Into::into).collect();
        self
    }
}

impl Host for BufferHost {
    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), RunnerError> {
        if kind == MirDiagnosticKind::Mone {
            self.stderr_lines.push(text.to_owned());
        } else {
            let mut line = std::mem::take(&mut self.pending_stdout);
            line.push_str(text);
            self.stdout_lines.push(line);
        }
        Ok(())
    }

    fn write_stdout_raw(&mut self, text: &str) -> Result<(), RunnerError> {
        self.pending_stdout.push_str(text);
        Ok(())
    }

    fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        Ok(self.stdin_lines.pop_front())
    }

    fn abort(&mut self, reason: &str) -> ! {
        self.last_abort = Some(reason.to_owned());
        panic!("runner abort: {reason}");
    }

    fn provider(&mut self, provider: &MirProvider) -> Result<Value, RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            &format!("provider sym#{}", provider.name.0),
        ))
    }

    fn exit(&mut self, code: i32) -> ! {
        panic!("runner exit: {code}");
    }

    fn kernel_call(
        &mut self,
        module: KernelModule,
        verb: &str,
        args: &[Value],
        span: Span,
    ) -> Result<Value, RunnerError> {
        kernel::dispatch_kernel(self, module, verb, args, span)
    }

    fn argumenta(&self) -> Result<&[String], RunnerError> {
        Ok(&self.argumenta)
    }

    fn env_get(&self, name: &str) -> Result<Option<String>, RunnerError> {
        Ok(self.env.get(name).cloned())
    }

    fn env_set(&mut self, name: &str, value: &str) -> Result<(), RunnerError> {
        self.env.insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn cwd(&self) -> Result<String, RunnerError> {
        Ok(self.cwd.clone())
    }

    fn set_cwd(&mut self, path: &str) -> Result<(), RunnerError> {
        self.cwd = path.to_string();
        Ok(())
    }

    fn pid(&self) -> Result<i64, RunnerError> {
        Ok(0)
    }
}

/// CLI host: stdout/stderr diagnostics, real stdin, and real process state.
///
/// Env reads/writes and cwd mutation reach the real process through this host,
/// which is the whole point of running Faber as a script.
#[derive(Debug, Default)]
pub struct StdioHost {
    pub argumenta: Vec<String>,
}

impl StdioHost {
    #[must_use]
    pub fn with_argumenta(argumenta: Vec<String>) -> Self {
        Self { argumenta }
    }
}

impl Host for StdioHost {
    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), RunnerError> {
        let target: &mut dyn Write = match kind {
            MirDiagnosticKind::Mone => &mut io::stderr(),
            _ => &mut io::stdout(),
        };
        writeln!(target, "{text}")
            .map_err(|err| RunnerError::internal(Span::default(), err.to_string()))
    }

    fn write_stdout_raw(&mut self, text: &str) -> Result<(), RunnerError> {
        let mut out = io::stdout();
        write!(out, "{text}")
            .map_err(|err| RunnerError::internal(Span::default(), err.to_string()))?;
        out.flush()
            .map_err(|err| RunnerError::internal(Span::default(), err.to_string()))
    }

    fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        let mut line = String::new();
        let read = io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|err| RunnerError::internal(Span::default(), err.to_string()))?;
        if read == 0 {
            return Ok(None);
        }
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        Ok(Some(line))
    }

    fn abort(&mut self, reason: &str) -> ! {
        let _ = writeln!(io::stderr(), "abort: {reason}");
        std::process::exit(1);
    }

    fn provider(&mut self, provider: &MirProvider) -> Result<Value, RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            &format!("provider sym#{}", provider.name.0),
        ))
    }

    fn exit(&mut self, code: i32) -> ! {
        std::process::exit(code);
    }

    fn kernel_call(
        &mut self,
        module: KernelModule,
        verb: &str,
        args: &[Value],
        span: Span,
    ) -> Result<Value, RunnerError> {
        kernel::dispatch_kernel(self, module, verb, args, span)
    }

    fn argumenta(&self) -> Result<&[String], RunnerError> {
        Ok(&self.argumenta)
    }

    fn env_get(&self, name: &str) -> Result<Option<String>, RunnerError> {
        Ok(std::env::var(name).ok())
    }

    fn env_set(&mut self, name: &str, value: &str) -> Result<(), RunnerError> {
        // SAFETY: scripts are explicit process orchestration tools, and
        // `StdioHost` is only constructed for `faber run` / `-c` / `repl`. The
        // env mutation is the user's intent, not ambient test pollution.
        // Edition 2024 promotes set_var to unsafe; suppress the lint until then.
        #[allow(unused_unsafe)]
        unsafe {
            std::env::set_var(name, value);
        }
        Ok(())
    }

    fn cwd(&self) -> Result<String, RunnerError> {
        Ok(std::env::current_dir().map_or_else(
            |_| ".".to_owned(),
            |path| path.to_string_lossy().into_owned(),
        ))
    }

    fn set_cwd(&mut self, path: &str) -> Result<(), RunnerError> {
        std::env::set_current_dir(path).map_err(|err| {
            RunnerError::internal(Span::default(), format!("processus.muta failed: {err}"))
        })
    }

    fn pid(&self) -> Result<i64, RunnerError> {
        Ok(i64::from(std::process::id()))
    }
}
