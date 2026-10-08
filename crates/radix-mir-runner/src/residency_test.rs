//! Tests for device residency: a host access to a device-resident tensor is a
//! sync point that downloads only the range read; a host write marks the
//! device copy stale.

use super::kernel_dispatch::{DeviceToken, ElementRange, KernelDispatcher, ResidencyHook};
use super::*;
use crate::value::TensorValue;
use radix_lexer::Span;
use radix_mir::MirFunctionId;

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
