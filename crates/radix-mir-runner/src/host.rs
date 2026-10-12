#![allow(clippy::needless_lifetimes)]

use super::{RunnerError, Value};
use radix_lexer::{Interner, Span};
use radix_mir::{MirDiagnosticKind, MirProvider, MirProviderKind};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::io::{self, BufRead, IsTerminal, Read, Write};

/// Render a provider route without exposing interner allocation numbers.
///
/// Provider MIR stores interned symbols, but those numeric ids are an
/// implementation detail and change with source load order. The runner owns
/// the interner, so resolve the route at the boundary where failures become
/// user-visible.
#[must_use]
pub(crate) fn provider_identity(provider: &MirProvider, interner: &Interner) -> String {
    let mut route = provider
        .module
        .iter()
        .map(|symbol| interner.resolve(*symbol).to_owned())
        .collect::<Vec<_>>();
    route.push(interner.resolve(provider.name).to_owned());
    let route = route.join("::");
    match provider.kind {
        MirProviderKind::Package => format!("package:{route}"),
        MirProviderKind::Kernel(module) => format!("kernel:{}:{route}", module.name()),
    }
}

impl RunnerError {
    /// Replace the legacy numeric provider diagnostic with its stable route.
    pub(crate) fn with_provider_identity(mut self, identity: &str) -> Self {
        if self.code == super::RUNNER_CODE_UNSUPPORTED
            && self
                .args
                .iter()
                .any(|(key, value)| key == "feature" && value == "provider")
        {
            // WHY: identity derives from the structured code + args, not from
            // matching prose; the route lands in both the issue and the args.
            self.message = format!("{}: provider {identity}", super::RUNNER_UNSUPPORTED_PREFIX);
            self.issue = format!("{}_{}", self.issue, super::runner_slug(identity));
            self.args.push(("identity".to_owned(), identity.to_owned()));
        }
        self
    }
}

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
    fn exit(&mut self, code: i32) -> !;

    /// Tier-3 conversation answer for a frame route (A4/U9): `Some(Ok(items))`
    /// answers content items then `done`, `Some(Err(data))` an `error`
    /// terminal. `None` means the host does not serve the route and the
    /// router fails closed. `StdioHost` and `BufferHost` serve the raw
    /// process-capability routes in `routes.rs`.
    fn sermo_route(
        &mut self,
        _route: &str,
        _opener: &Value,
        _span: Span,
    ) -> Option<Result<Vec<Value>, Value>> {
        None
    }

    /// Write text to stdout **without** appending a newline (`consolum.write_partial`).
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
            "host write_stdout_raw (consolum.write_partial)",
        ))
    }

    /// Write bytes to stdout **without** appending a newline (`consolum.write_bytes`).
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying write fails, or when the host does
    /// not expose a console.
    fn write_stdout_bytes(&mut self, _bytes: &[u8]) -> Result<(), RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            "host write_stdout_bytes (consolum.write_bytes)",
        ))
    }

    /// Read up to `len` bytes from stdin (`consolum.read_bytes`).
    ///
    /// Short reads (including EOF) return the bytes obtained. `len == 0`
    /// returns an empty buffer.
    ///
    /// # Errors
    ///
    /// Returns an error when the underlying read fails, or when the host does
    /// not expose stdin.
    fn read_bytes(&mut self, _len: usize) -> Result<Vec<u8>, RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            "host read_bytes (consolum.read_bytes)",
        ))
    }

    /// Whether stdin is a TTY (`consolum.stdin_is_tty`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose TTY probes.
    fn stdin_is_tty(&self) -> Result<bool, RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            "host stdin_is_tty (consolum.stdin_is_tty)",
        ))
    }

    /// Whether stdout is a TTY (`consolum.stdout_is_tty`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose TTY probes.
    fn stdout_is_tty(&self) -> Result<bool, RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            "host stdout_is_tty (consolum.stdout_is_tty)",
        ))
    }

    /// Whether stderr is a TTY (`consolum.stderr_is_tty`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose TTY probes.
    fn stderr_is_tty(&self) -> Result<bool, RunnerError> {
        Err(RunnerError::unsupported(
            Span::default(),
            "host stderr_is_tty (consolum.stderr_is_tty)",
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

    /// Read an environment variable (`processus.read_env`). Hosts that do not
    /// expose process env return `Err`. `Ok(None)` means the variable is not
    /// set; `Ok(Some(v))` returns the value.
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose environment access.
    fn env_get(&self, _name: &str) -> Result<Option<String>, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host env_get"))
    }

    /// Write an environment variable (`processus.write_env`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose environment mutation.
    fn env_set(&mut self, _name: &str, _value: &str) -> Result<(), RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host env_set"))
    }

    /// Current working directory (`processus.cwd`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose a filesystem.
    fn cwd(&self) -> Result<String, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host cwd"))
    }

    /// Change the current working directory (`processus.chdir`).
    ///
    /// # Errors
    ///
    /// Returns an error when the host does not expose a filesystem.
    fn set_cwd(&mut self, _path: &str) -> Result<(), RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "host set_cwd"))
    }

    /// Process id (`processus.pid`).
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
    /// line writer (`nota` / `consolum.write_line`).
    pub pending_stdout: String,
    pub stdin_lines: VecDeque<String>,
    /// Byte stdin for `consolum.read_bytes` (independent of `stdin_lines`).
    pub stdin_bytes: VecDeque<u8>,
    pub argumenta: Vec<String>,
    pub env: FxHashMap<String, String>,
    pub cwd: String,
    /// Last `abort` reason observed before the host panicked (test seam).
    pub last_abort: Option<String>,
}

impl BufferHost {
    #[must_use]
    pub fn with_argumenta(argumenta: Vec<String>) -> Self {
        faber_leaf::process::install_argv(argumenta.clone());
        Self {
            argumenta,
            ..Self::default()
        }
    }

    /// Seed an environment variable for `processus.read_env` assertions.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Seed the virtual working directory returned by `processus.cwd`.
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

    #[must_use]
    pub fn with_stdin_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin_bytes = bytes.into().into();
        self
    }
}

impl Host for BufferHost {
    fn sermo_route(
        &mut self,
        route: &str,
        opener: &Value,
        span: Span,
    ) -> Option<Result<Vec<Value>, Value>> {
        super::routes::serve_route(self, route, opener, span)
    }

    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), RunnerError> {
        if matches!(kind, MirDiagnosticKind::Mone | MirDiagnosticKind::Vide) {
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

    fn write_stdout_bytes(&mut self, bytes: &[u8]) -> Result<(), RunnerError> {
        self.pending_stdout
            .push_str(&String::from_utf8_lossy(bytes));
        Ok(())
    }

    fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        Ok(self.stdin_lines.pop_front())
    }

    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>, RunnerError> {
        let n = len.min(self.stdin_bytes.len());
        Ok(self.stdin_bytes.drain(..n).collect())
    }

    fn stdin_is_tty(&self) -> Result<bool, RunnerError> {
        Ok(false)
    }

    fn stdout_is_tty(&self) -> Result<bool, RunnerError> {
        Ok(false)
    }

    fn stderr_is_tty(&self) -> Result<bool, RunnerError> {
        Ok(false)
    }

    fn abort(&mut self, reason: &str) -> ! {
        self.last_abort = Some(reason.to_owned());
        panic!("runner abort: {reason}");
    }

    fn provider(&mut self, _provider: &MirProvider) -> Result<Value, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "provider"))
    }

    fn exit(&mut self, code: i32) -> ! {
        panic!("runner exit: {code}");
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
        faber_leaf::process::install_argv(argumenta.clone());
        Self { argumenta }
    }
}

impl Host for StdioHost {
    fn sermo_route(
        &mut self,
        route: &str,
        opener: &Value,
        span: Span,
    ) -> Option<Result<Vec<Value>, Value>> {
        super::routes::serve_route(self, route, opener, span)
    }

    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), RunnerError> {
        let target: &mut dyn Write = match kind {
            MirDiagnosticKind::Mone | MirDiagnosticKind::Vide => &mut io::stderr(),
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

    fn write_stdout_bytes(&mut self, bytes: &[u8]) -> Result<(), RunnerError> {
        let mut out = io::stdout();
        out.write_all(bytes)
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

    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>, RunnerError> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let mut buffer = vec![0_u8; len];
        let read = io::stdin()
            .lock()
            .read(&mut buffer)
            .map_err(|err| RunnerError::internal(Span::default(), err.to_string()))?;
        buffer.truncate(read);
        Ok(buffer)
    }

    fn stdin_is_tty(&self) -> Result<bool, RunnerError> {
        Ok(io::stdin().is_terminal())
    }

    fn stdout_is_tty(&self) -> Result<bool, RunnerError> {
        Ok(io::stdout().is_terminal())
    }

    fn stderr_is_tty(&self) -> Result<bool, RunnerError> {
        Ok(io::stderr().is_terminal())
    }

    fn abort(&mut self, reason: &str) -> ! {
        let _ = writeln!(io::stderr(), "abort: {reason}");
        std::process::exit(1);
    }

    fn provider(&mut self, _provider: &MirProvider) -> Result<Value, RunnerError> {
        Err(RunnerError::unsupported(Span::default(), "provider"))
    }

    fn exit(&mut self, code: i32) -> ! {
        std::process::exit(code);
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
            RunnerError::internal(Span::default(), format!("processus.chdir failed: {err}"))
        })
    }

    fn pid(&self) -> Result<i64, RunnerError> {
        Ok(i64::from(std::process::id()))
    }
}
