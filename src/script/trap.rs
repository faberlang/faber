//! Exit/abort traps so interpreted Faber can return an [`ExitCode`] without
//! terminating the embedder process.

use radix::mir::{Host, MirDiagnosticKind, MirProvider, StepperError, Value};
use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::ExitCode;

/// Control-flow trap raised when interpreted Faber calls `exi` or aborts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostTrap {
    Exit(i32),
    Abort(String),
}

/// Wraps a [`Host`] so script runners can return an [`ExitCode`] instead of
/// terminating the process.
pub(crate) struct TrapHost<'a> {
    pub inner: &'a mut dyn Host,
}

impl Host for TrapHost<'_> {
    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), StepperError> {
        self.inner.scribe(kind, text)
    }

    fn read_line(&mut self) -> Result<Option<String>, StepperError> {
        self.inner.read_line()
    }

    fn abort(&mut self, reason: &str) -> ! {
        panic_trap(HostTrap::Abort(reason.to_owned()));
    }

    fn provider(&mut self, provider: &MirProvider) -> Result<Value, StepperError> {
        self.inner.provider(provider)
    }

    fn exit(&mut self, code: i32) -> ! {
        panic_trap(HostTrap::Exit(code));
    }

    fn argumenta(&self) -> &[String] {
        self.inner.argumenta()
    }

    fn env_get(&self, name: &str) -> Option<String> {
        self.inner.env_get(name)
    }

    fn env_set(&mut self, name: &str, value: &str) {
        self.inner.env_set(name, value);
    }

    fn cwd(&self) -> String {
        self.inner.cwd()
    }

    fn set_cwd(&mut self, path: &str) -> Result<(), StepperError> {
        self.inner.set_cwd(path)
    }

    fn pid(&self) -> i64 {
        self.inner.pid()
    }
}

fn panic_trap(trap: HostTrap) -> ! {
    std::panic::resume_unwind(Box::new(trap));
}

pub(crate) fn exit_code_from_i32(code: i32) -> ExitCode {
    // WHY: Unix wait status uses the low eight bits of the exit code.
    ExitCode::from((code & 0xff) as u8)
}

/// Inverse of [`exit_code_from_i32`] for [`std::process::exit`].
pub fn raw_exit_code(code: ExitCode) -> i32 {
    if code == ExitCode::SUCCESS {
        return 0;
    }
    if code == ExitCode::FAILURE {
        return 1;
    }
    for value in 2..=255 {
        if code == ExitCode::from(value) {
            return value as i32;
        }
    }
    1
}

fn trap_from_payload(payload: &(dyn Any + Send)) -> Option<HostTrap> {
    payload.downcast_ref::<HostTrap>().cloned()
}

/// Run interpreted Faber until completion, explicit exit, or abort.
///
/// Returns a foreign panic payload when the closure panics for a reason other
/// than [`HostTrap`].
pub(crate) fn run_trapped<F>(run: AssertUnwindSafe<F>) -> Result<ExitCode, Box<dyn Any + Send>>
where
    F: FnOnce(),
{
    match catch_unwind(run) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(payload) => {
            if let Some(trap) = trap_from_payload(payload.as_ref()) {
                match trap {
                    HostTrap::Exit(code) => Ok(exit_code_from_i32(code)),
                    HostTrap::Abort(_) => Ok(ExitCode::FAILURE),
                }
            } else {
                Err(payload)
            }
        }
    }
}
