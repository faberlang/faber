//! Residual kernel dispatch fails closed after N2 deleted the modules.

use super::kernel::dispatch_kernel;
use super::{BufferHost, Value};
use radix_lexer::Span;
use radix_mir::KernelModule;

#[test]
fn leftover_json_kernel_fails_closed() {
    let mut host = BufferHost::default();
    let error = dispatch_kernel(
        &mut host,
        KernelModule::Json,
        "parse",
        &[Value::Str("{}".to_owned())],
        Span::default(),
    )
    .expect_err("json kernel is deleted");
    assert!(error.message.contains("deleted"));
}

#[test]
fn leftover_toml_kernel_fails_closed() {
    let mut host = BufferHost::default();
    let error = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "parse",
        &[Value::Str("x = 1".to_owned())],
        Span::default(),
    )
    .expect_err("toml kernel is deleted");
    assert!(error.message.contains("deleted"));
}
