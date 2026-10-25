//! Tests for device residency: a host access to a device-resident tensor is a
//! sync point that downloads only the range read; a host write marks the
//! device copy stale.

use super::kernel_dispatch::{DeviceToken, ElementRange, KernelDispatcher, ResidencyHook};
use super::*;
use crate::host::BufferHost;
use crate::value::{FieldOrder, TensorValue};
use radix_lexer::{Interner, Span};
use radix_mir::MirFunctionId;
use radix_types::TypeTable;

#[derive(Default)]
struct Stub {
    downloads: Vec<(DeviceToken, ElementRange)>,
    host_writes: Vec<DeviceToken>,
    uploads: usize,
}

impl KernelDispatcher for Stub {
    fn dispatch(
        &mut self,
        _function: MirFunctionId,
        _args: &mut [Value],
        _span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        Ok(None)
    }

    fn make_host_current(
        &mut self,
        token: DeviceToken,
        range: ElementRange,
    ) -> Result<(), RunnerError> {
        self.downloads.push((token, range));
        Ok(())
    }

    fn host_wrote(&mut self, token: DeviceToken) {
        self.host_writes.push(token);
    }
}

fn resident_tensor(host_current: bool) -> (TensorValue, Rc<RefCell<Stub>>) {
    let stub = Rc::new(RefCell::new(Stub::default()));
    let hook: ResidencyHook = stub.clone();
    let tensor = TensorValue::try_contiguous((0..4).map(Value::Int).collect(), vec![4])
        .expect("contiguous tensor");
    tensor.mark_device_resident(DeviceToken(7), hook, host_current);
    (tensor, stub)
}

/// The test's stand-in for a device use: re-upload when the device copy is stale.
fn device_use(tensor: &TensorValue, stub: &Rc<RefCell<Stub>>) {
    let (token, current) = tensor.device_residency().expect("resident");
    if !current {
        stub.borrow_mut().uploads += 1;
        let hook: ResidencyHook = stub.clone();
        tensor.mark_device_resident(token, hook, true);
    }
}

#[test]
fn residency_host_read_downloads_only_the_range_once() {
    let (tensor, stub) = resident_tensor(false);
    assert_eq!(tensor.value_at(2), Some(Value::Int(2)));
    assert_eq!(stub.borrow().downloads, vec![(DeviceToken(7), 2..3)]);
    // The same range again does not call the dispatcher.
    assert_eq!(tensor.value_at(2), Some(Value::Int(2)));
    assert_eq!(stub.borrow().downloads.len(), 1);
    // A different element downloads its own range only.
    assert_eq!(tensor.value_at(0), Some(Value::Int(0)));
    assert_eq!(stub.borrow().downloads.len(), 2);
    assert_eq!(stub.borrow().downloads[1].1, 0..1);
    // A view shares the storage's residency.
    let view = tensor.view(vec![2], vec![1], 2).expect("view");
    assert_eq!(view.value_at(0), Some(Value::Int(2)));
    assert_eq!(stub.borrow().downloads.len(), 2);
    assert_eq!(tensor.logical_values().map(|v| v.len()), Some(4));
    assert_eq!(stub.borrow().downloads.len(), 3);
    assert_eq!(stub.borrow().downloads[2].1, 0..4);
}

#[test]
fn residency_host_write_marks_the_device_copy_stale() {
    let (tensor, stub) = resident_tensor(false);
    tensor.set_value_at(1, Value::Int(99)).expect("write");
    assert_eq!(stub.borrow().host_writes, vec![DeviceToken(7)]);
    assert_eq!(tensor.device_residency(), Some((DeviceToken(7), false)));
    // The write first made the rest of the host copy current.
    assert_eq!(stub.borrow().downloads, vec![(DeviceToken(7), 0..4)]);
    // A further write while stale does not notify again.
    tensor.set_value_at(2, Value::Int(5)).expect("write");
    assert_eq!(stub.borrow().host_writes.len(), 1);
    // The next device use re-uploads once and the device copy is current.
    device_use(&tensor, &stub);
    assert_eq!(stub.borrow().uploads, 1);
    assert_eq!(tensor.device_residency(), Some((DeviceToken(7), true)));
    // Reads after a write see the host value without further downloads.
    assert_eq!(tensor.value_at(1), Some(Value::Int(99)));
    assert_eq!(stub.borrow().downloads.len(), 1);
}

#[test]
fn residency_uploaded_tensor_reads_without_download() {
    let (tensor, stub) = resident_tensor(true);
    assert_eq!(tensor.value_at(3), Some(Value::Int(3)));
    assert!(stub.borrow().downloads.is_empty());
}

#[test]
fn residency_absent_marker_is_unchanged_behavior() {
    let tensor = TensorValue::try_contiguous(vec![Value::Int(1)], vec![1]).expect("tensor");
    assert_eq!(tensor.device_residency(), None);
    assert_eq!(tensor.value_at(0), Some(Value::Int(1)));
    tensor.set_value_at(0, Value::Int(2)).expect("write");
    assert_eq!(tensor.value_at(0), Some(Value::Int(2)));
}

/// A dispatcher whose downloads fail with a distinct issue code.
struct FailingDownload;

impl KernelDispatcher for FailingDownload {
    fn dispatch(
        &mut self,
        _function: MirFunctionId,
        _args: &mut [Value],
        _span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        Ok(None)
    }

    fn make_host_current(
        &mut self,
        _token: DeviceToken,
        _range: ElementRange,
    ) -> Result<(), RunnerError> {
        Err(RunnerError::internal_with_issue(
            Span::default(),
            "device fault",
            "stub_device_fault",
        ))
    }
}

fn faulting_tensor() -> TensorValue {
    let hook: ResidencyHook = Rc::new(RefCell::new(FailingDownload));
    let tensor = TensorValue::try_contiguous((0..4).map(Value::Int).collect(), vec![4])
        .expect("contiguous tensor");
    tensor.mark_device_resident(DeviceToken(7), hook, false);
    tensor
}

#[test]
fn residency_fault_surfaces_from_a_whole_tensor_print() {
    let interner = Interner::new();
    let empty = handroll::FxHashMap::default();
    let order = FieldOrder::new(&empty, &empty);
    let value = Value::Tensor(Rc::new(RefCell::new(faulting_tensor())));
    let error = value
        .display_typed(&interner, order, Span::default(), None)
        .expect_err("the failed download must stop the print");
    assert_eq!(error.issue, "stub_device_fault");
}

#[test]
fn residency_fault_surfaces_from_an_element_read() {
    let types = TypeTable::new();
    let interner = Interner::new();
    let program = MirProgram::new();
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let cell = Rc::new(RefCell::new(faulting_tensor()));
    let error = Runner::<BufferHost>::collection_index(
        Value::Tensor(cell.clone()),
        &Value::Int(2),
        Span::default(),
    )
    .expect_err("the failed download must stop the element read");
    assert_eq!(error.issue, "stub_device_fault");
    // The fallible logical read (optional-access path) reports it too.
    let cell = Rc::new(RefCell::new(faulting_tensor()));
    let error = runner
        .tensor_read_at(&cell, 1, Span::default())
        .expect_err("the failed download is an error, not a missing element");
    assert_eq!(error.issue, "stub_device_fault");
}

#[test]
fn residency_invalid_storage_without_a_stored_fault_keeps_the_layout_diagnostic() {
    let interner = Interner::new();
    let empty = handroll::FxHashMap::default();
    let order = FieldOrder::new(&empty, &empty);
    // Backing shorter than the shape: the logical read fails, no device fault.
    let mut tensor = TensorValue::try_contiguous(vec![Value::Int(1)], vec![1]).expect("tensor");
    tensor.shape = vec![4];
    let value = Value::Tensor(Rc::new(RefCell::new(tensor)));
    let error = value
        .display_typed(&interner, order, Span::default(), None)
        .expect_err("a short backing is invalid storage");
    assert_eq!(error.issue, "runner_tensor_view_has_invalid_storage_layout");
}
