//! `output ⇇ value` on a Metal kernel against the MIR runner (the oracle for
//! the operator): the device lowers the copy to the output-buffer store the
//! recipes already write, and the values equal the host copy-into result.
//! Requires a Metal device (macOS).

#![cfg(target_os = "macos")]

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::test_support::compile_metal_text_output;
use crate::mir::{BufferHost, run_source};
use metal::{CompileOptions, Device, MTLResourceOptions, MTLSize};

/// Run a host `incipit` through the MIR runner and parse its one printed
/// tensor line (`[1.0, 2.0, ...]`).
fn runner_values(source: &str) -> Vec<f32> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    run_source(&session, "copy-into-device.fab", source, &mut host)
        .unwrap_or_else(|error| panic!("{source}: {error:?}"));
    let [line] = host.stdout_lines.as_slice() else {
        panic!("expected one printed line, got {:?}", host.stdout_lines);
    };
    line.trim_matches(|c| c == '[' || c == ']')
        .split(", ")
        .map(|value| value.parse::<f32>().expect("runner value parses"))
        .collect()
}

/// Compile the kernel to Metal, bind `inputs` then a zeroed output of
/// `output_len` floats, dispatch one thread per output element, and read the
/// output back.
fn device_values(kernel_source: &str, inputs: &[&[f32]], output_len: usize) -> Vec<f32> {
    let output = compile_metal_text_output(kernel_source).expect("the ⇇ kernel emits Metal");
    let device = Device::system_default().expect("a Metal device");
    let library = device
        .new_library_with_source(&output.code, &CompileOptions::new())
        .expect("the emitted Metal compiles on device");
    // ca2499dc-3.1: the compiler names the entry `k<function id>`; take it
    // from the emitted reflection instead of the author's `k` (decoration only).
    let entry = output
        .reflection
        .as_ref()
        .and_then(|reflection| reflection.kernels.first())
        .map(|kernel| kernel.entry_name.as_str())
        .expect("the Metal reflection names the kernel entry");
    let function = library
        .get_function(entry, None)
        .expect("the reflected kernel entry");
    let pipeline = device
        .new_compute_pipeline_state_with_function(&function)
        .expect("pipeline builds");

    let buffers: Vec<_> = inputs
        .iter()
        .map(|values| {
            device.new_buffer_with_data(
                values.as_ptr().cast(),
                std::mem::size_of_val(*values) as u64,
                MTLResourceOptions::StorageModeShared,
            )
        })
        .collect();
    let out = device.new_buffer((output_len * 4) as u64, MTLResourceOptions::StorageModeShared);
    let queue = device.new_command_queue();
    let command_buffer = queue.new_command_buffer();
    let encoder = command_buffer.new_compute_command_encoder();
    encoder.set_compute_pipeline_state(&pipeline);
    for (index, buffer) in buffers.iter().enumerate() {
        encoder.set_buffer(index as u64, Some(buffer), 0);
    }
    encoder.set_buffer(buffers.len() as u64, Some(&out), 0);
    encoder.dispatch_threads(MTLSize::new(output_len as u64, 1, 1), MTLSize::new(1, 1, 1));
    encoder.end_encoding();
    command_buffer.commit();
    command_buffer.wait_until_completed();
    unsafe { std::slice::from_raw_parts(out.contents().cast::<f32>(), output_len) }.to_vec()
}

fn kernel(signature_and_body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n@ kernel\n@ public\n{signature_and_body}\n")
}

const A: [f32; 12] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0];
const B: [f32; 12] = [0.5, -1.0, 2.5, -3.0, 4.5, -5.0, 6.5, -7.0, 8.5, -9.0, 10.5, -11.0];

fn host_tensor(name: &str, values: &[f32], shape: &str) -> String {
    let literal = values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "sit lista<f32> {name}_data ← [{literal}]\nsit tensor<f32, {shape}> {name} ← {name}_data ↦ tensor<f32, {shape}>\n"
    )
}

/// A view source (`input.transpose()`) written through into the output buffer.
#[test]
fn a_transpose_view_copied_into_the_output_matches_the_runner() {
    let device = device_values(
        &kernel("fn k(tf32[3, 4] input, mut tf32[4, 3] output, u32 id) → void {\n    output ⇇ input.transpose()\n}"),
        &[&A],
        12,
    );
    let runner = runner_values(&format!(
        "incipit {{\n{}varia tensor<f32, [4, 3]> t ← tf32[4, 3] {{}}\nt ⇇ a.transpone()\nnota t.planata()\n}}",
        host_tensor("a", &A, "[3, 4]")
    ));
    assert_eq!(device, runner);
    assert_eq!(device[1], 5.0, "the transpose actually moved values");
}

/// A plain buffer source becomes an elementwise copy body.
#[test]
fn a_plain_buffer_copied_into_the_output_matches_the_runner() {
    let device = device_values(
        &kernel("fn k(tf32[3, 4] input, mut tf32[3, 4] output, u32 id) → void {\n    output ⇇ input\n}"),
        &[&A],
        12,
    );
    let runner = runner_values(&format!(
        "incipit {{\n{}varia tensor<f32, [3, 4]> b ← tf32[3, 4] {{}}\nb ⇇ a\nnota b.planata()\n}}",
        host_tensor("a", &A, "[3, 4]")
    ));
    assert_eq!(device, runner);
    assert_eq!(device, A.to_vec());
}

/// An elementwise result fuses into the output store.
#[test]
fn an_elementwise_result_copied_into_the_output_matches_the_runner() {
    let device = device_values(
        &kernel(
            "fn k(tf32[3, 4] input, tf32[3, 4] right, mut tf32[3, 4] output, u32 id) → void {\n    output ⇇ input + right\n}",
        ),
        &[&A, &B],
        12,
    );
    let runner = runner_values(&format!(
        "incipit {{\n{}{}varia tensor<f32, [3, 4]> c ← tf32[3, 4] {{}}\nc ⇇ a + b\nnota c.planata()\n}}",
        host_tensor("a", &A, "[3, 4]"),
        host_tensor("b", &B, "[3, 4]")
    ));
    assert_eq!(device, runner);
}
