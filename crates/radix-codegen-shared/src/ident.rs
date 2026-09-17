//! Shared identifier sanitation and output-resource naming for text emitters.

pub fn sanitize_ident(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in raw.chars().enumerate() {
        let valid = ch == '_' || ch.is_ascii_alphanumeric();
        if idx == 0 && ch.is_ascii_digit() {
            out.push('_');
        }
        out.push(if valid { ch } else { '_' });
    }
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}

/// Returns the output resource name, avoiding collision with the kernel input
/// parameter when the parameter happens to be named `output`.
pub fn output_resource_name(param_name: &str) -> &'static str {
    if param_name == "output" {
        "output_buffer"
    } else {
        "output"
    }
}

#[cfg(test)]
#[path = "ident_test.rs"]
mod tests;
