//! `faber model` — model file inspection and metadata display.
//!
//! # Policy
//!
//! - **No tensor materialization.** This module parses only the safetensors
//!   JSON header. It never decodes tensor payload bytes, runs tokenizers,
//!   executes inference, or claims GPU/runtime support.
//! - **Safetensors only.** GGUF, ONNX, and other model formats are out of
//!   scope. Non-safetensors files produce a clear diagnostic.
//!
//! The safetensors wire format is:
//! ```text
//! [8 bytes: little-endian u64 header length N]
//! [N bytes: UTF-8 JSON header, padded with 0x20 to 8-byte boundary]
//! [tensor payload bytes]
//! ```
//! See <https://huggingface.co/docs/safetensors/en/spec>.

use crate::cli::{ModelCommand, ModelInspectArgs};
use serde_json::Value;
use std::fmt;
use std::fs;

/// Describes one tensor entry from a safetensors header.
#[derive(Debug)]
struct TensorInfo {
    name: String,
    dtype: String,
    shape: Vec<u64>,
    data_offsets: Vec<u64>,
    element_count: u64,
}

impl fmt::Display for TensorInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shape_str = self
            .shape
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let offset_str = self
            .data_offsets
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        write!(
            f,
            "  {:40} {:>8}  [{}]  offsets [{}]  {} elements",
            self.name, self.dtype, shape_str, offset_str, self.element_count
        )
    }
}

/// Parsed safetensors header metadata.
#[derive(Debug)]
struct SafetensorsHeader {
    metadata: Vec<(String, String)>,
    tensors: Vec<TensorInfo>,
}

/// Main dispatch for `faber model`.
pub(crate) fn cmd_model(command: ModelCommand) {
    match command {
        ModelCommand::Inspect(args) => cmd_model_inspect(args),
    }
}

/// `faber model inspect <path>`
fn cmd_model_inspect(args: ModelInspectArgs) {
    let path = &args.path;

    if !path.is_file() {
        eprintln!("error: no such file: {}", path.display());
        std::process::exit(1);
    }
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("safetensors"))
    {
        eprintln!(
            "error: unsupported file format: {} (expected .safetensors)",
            path.display()
        );
        std::process::exit(1);
    }

    let data = match fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {}: {}", path.display(), e);
            std::process::exit(1);
        }
    };

    let header = match parse_safetensors_metadata(&data) {
        Ok(h) => h,
        Err(msg) => {
            eprintln!("error: invalid safetensors file: {}", msg);
            std::process::exit(1);
        }
    };

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<unknown>");

    println!("Model: {}", file_name);
    println!("Format: safetensors");
    println!("Tensors: {}", header.tensors.len());
    for (key, value) in &header.metadata {
        println!("  metadata  {}: {}", key, value);
    }
    println!();
    for (i, tensor) in header.tensors.iter().enumerate() {
        println!("  #{}  {}", i + 1, tensor);
    }
}

/// Parse safetensors header bytes into structured metadata.
///
/// Reads the 8-byte little-endian header length, extracts the JSON header
/// section, and deserializes tensor metadata entries. Returns an error
/// message string on failure — never panics.
fn parse_safetensors_metadata(data: &[u8]) -> Result<SafetensorsHeader, String> {
    if data.len() < 8 {
        return Err(format!(
            "file too short: {} bytes, need at least 8 for header length",
            data.len()
        ));
    }

    // Read 8-byte little-endian header length.
    let mut header_len_bytes = [0_u8; 8];
    header_len_bytes.copy_from_slice(&data[..8]);
    let header_len = u64::from_le_bytes(header_len_bytes) as usize;
    if header_len == 0 {
        return Err("header length is zero".into());
    }
    let header_end = 8 + header_len;
    if data.len() < header_end {
        return Err(format!(
            "file too short: {} bytes, header claims {} bytes",
            data.len(),
            header_end
        ));
    }

    // Extract the JSON header (trim trailing padding spaces, 0x20).
    let header_bytes = &data[8..header_end];
    let header_str =
        std::str::from_utf8(header_bytes).map_err(|_| "header is not valid UTF-8".to_string())?;
    let header_str = header_str.trim_end_matches('\x20');

    let json: Value =
        serde_json::from_str(header_str).map_err(|e| format!("invalid JSON header: {}", e))?;

    let obj = json
        .as_object()
        .ok_or("header JSON root is not an object")?;

    // Extract __metadata__.
    let mut metadata: Vec<(String, String)> = Vec::new();
    if let Some(meta_val) = obj.get("__metadata__") {
        if let Some(meta_obj) = meta_val.as_object() {
            for (k, v) in meta_obj {
                let value_str = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => v.to_string(),
                };
                metadata.push((k.clone(), value_str));
            }
        }
    }

    // Collect tensor entries (everything except __metadata__).
    let mut tensors: Vec<TensorInfo> = Vec::new();
    for (name, value) in obj {
        if name == "__metadata__" {
            continue;
        }
        let tensor_obj = value
            .as_object()
            .ok_or_else(|| format!("tensor entry '{}' is not a JSON object", name))?;

        let dtype = tensor_obj
            .get("dtype")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("tensor '{}' missing or invalid 'dtype'", name))?
            .to_string();

        let shape: Vec<u64> = tensor_obj
            .get("shape")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("tensor '{}' missing or invalid 'shape'", name))?
            .iter()
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| format!("tensor '{}' shape contains non-u64 value", name))
            })
            .collect::<Result<Vec<_>, String>>()?;

        let data_offsets: Vec<u64> = tensor_obj
            .get("data_offsets")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("tensor '{}' missing or invalid 'data_offsets'", name))?
            .iter()
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| format!("tensor '{}' data_offsets contains non-u64 value", name))
            })
            .collect::<Result<Vec<_>, String>>()?;

        let element_count: u64 = shape.iter().product();

        tensors.push(TensorInfo {
            name: name.clone(),
            dtype,
            shape,
            data_offsets,
            element_count,
        });
    }

    Ok(SafetensorsHeader { metadata, tensors })
}

#[cfg(test)]
#[path = "model_test.rs"]
mod tests;
