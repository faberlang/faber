//! G6 GO3/GO4 — write planned Go package artifacts and invoke `go build`.
//!
//! Layout under `<package>/target/faber/go/`:
//! - `main.go` (entry)
//! - optional sibling `*.go` module files (same `package main`)
//! - namespace vars for local Faber imports (`binding.Method` → package funcs)
//! - `go.mod` module `faber/<package>`
//! - binary at `<package>/target/faber/go/bin/<name>`

use radix::diagnostics::Diagnostic;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::discovery::BuildLayout;

/// One package-level Go function extracted from generated module source.
#[derive(Debug, Clone)]
pub(crate) struct GoFuncSig {
    pub name: String,
    /// Full signature after `func `, e.g. `linea_operandorum(verba []string) string`.
    pub after_func: String,
}

/// Parse top-level `func name(...) ...` lines (not methods).
pub(crate) fn parse_go_func_sigs(code: &str) -> Vec<GoFuncSig> {
    let mut out = Vec::new();
    for line in code.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("func ") {
            continue;
        }
        // Skip methods: `func (recv T) Name`
        let rest = &trimmed["func ".len()..];
        if rest.starts_with('(') {
            continue;
        }
        let Some(paren) = rest.find('(') else {
            continue;
        };
        let name = rest[..paren].trim();
        if name.is_empty() || name == "main" {
            continue;
        }
        // Signature body ends before trailing ` {` if present on same line.
        let after = rest.trim_end();
        let after = after.strip_suffix('{').map(str::trim_end).unwrap_or(after);
        out.push(GoFuncSig {
            name: name.to_owned(),
            after_func: after.to_owned(),
        });
    }
    out
}

/// Capitalize first character (matches Go field access emit).
pub(crate) fn go_capitalize(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + chars.as_str(),
    }
}

/// `norma:consolum` host surface for Go package assembly.
///
/// This mirrors the built-in package dispatch contract closely enough that Go
/// package builds can use the public consolum API instead of a narrow echo-only
/// slice. The generated shim stays local to package assembly; other Norma
/// modules still fail closed until they grow explicit host support.
pub(crate) fn render_norma_consolum_shim(binding: &str) -> String {
    // Field names match Go field-access capitalize (dic → Dic, scribe → Scribe).
    format!(
        r#"var {binding}_reader = bufio.NewReader(os.Stdin)

func {binding}_hauri(magnitudo int64) []byte {{
	if magnitudo < 0 {{
		magnitudo = 0
	}}
	data := make([]byte, magnitudo)
	n, err := {binding}_reader.Read(data)
	if err != nil && err != io.EOF {{
		panic(err)
	}}
	return data[:n]
}}

func {binding}_lege() string {{
	line, err := {binding}_reader.ReadString('\n')
	if err != nil && err != io.EOF {{
		panic(err)
	}}
	line = strings.TrimSuffix(line, "\n")
	line = strings.TrimSuffix(line, "\r")
	return line
}}

func {binding}_funde(data []byte) {{
	if _, err := os.Stdout.Write(data); err != nil {{
		panic(err)
	}}
}}

func {binding}_isTerminal(file *os.File) bool {{
	info, err := file.Stat()
	if err != nil {{
		return false
	}}
	return (info.Mode() & os.ModeCharDevice) != 0
}}

var {binding} = struct {{
	Hauri func(int64) []byte
	Hauriet func(int64) []byte
	Lege func() string
	Leget func() string
	Funde func([]byte)
	Fundet func([]byte)
	Dic func(string)
	Dicet func(string)
	Scribet func(string)
	Scribe func(string)
	Mone func(string)
	Monet func(string)
	Vide func(string)
	Videbit func(string)
	Audit func() bool
	Loquitur func() bool
	Admonet func() bool
}}{{
	Hauri: {binding}_hauri,
	Hauriet: {binding}_hauri,
	Lege: {binding}_lege,
	Leget: {binding}_lege,
	Funde: {binding}_funde,
	Fundet: {binding}_funde,
	Dic: func(msg string) {{ fmt.Print(msg) }},
	Dicet: func(msg string) {{ fmt.Print(msg) }},
	Scribet: func(msg string) {{ fmt.Println(msg) }},
	Scribe: func(msg string) {{ fmt.Println(msg) }},
	Mone: func(msg string) {{ fmt.Fprintln(os.Stderr, msg) }},
	Monet: func(msg string) {{ fmt.Fprintln(os.Stderr, msg) }},
	Vide: func(msg string) {{ fmt.Fprintln(os.Stderr, msg) }},
	Videbit: func(msg string) {{ fmt.Fprintln(os.Stderr, msg) }},
	Audit: func() bool {{ return {binding}_isTerminal(os.Stdin) }},
	Loquitur: func() bool {{ return {binding}_isTerminal(os.Stdout) }},
	Admonet: func() bool {{ return {binding}_isTerminal(os.Stderr) }},
}}
"#
    )
}

/// Build a namespace var so `binding.CapitalizedField(...)` resolves to package funcs.
pub(crate) fn render_namespace_var(binding: &str, funcs: &[GoFuncSig]) -> String {
    if funcs.is_empty() {
        return format!("var {binding} = struct{{}}{{}}\n");
    }
    let mut w = String::new();
    w.push_str("var ");
    w.push_str(binding);
    w.push_str(" = struct {\n");
    for f in funcs {
        let field = go_capitalize(&f.name);
        // after_func is `name(params) rets` — replace name with field for type only.
        let sig_tail = f.after_func.strip_prefix(&f.name).unwrap_or(&f.after_func);
        w.push('\t');
        w.push_str(&field);
        w.push_str(" func");
        w.push_str(sig_tail);
        w.push('\n');
    }
    w.push_str("}{\n");
    for f in funcs {
        let field = go_capitalize(&f.name);
        w.push('\t');
        w.push_str(&field);
        w.push_str(": ");
        w.push_str(&f.name);
        w.push_str(",\n");
    }
    w.push_str("}\n");
    w
}

/// Drop package/import preamble; keep declaration body for same-package merge.
pub(crate) fn strip_go_preamble(code: &str) -> String {
    let mut out = String::new();
    let mut lines = code.lines().peekable();
    // Skip leading comments / package / imports.
    while let Some(line) = lines.peek().copied() {
        let t = line.trim_start();
        if t.is_empty()
            || t.starts_with("//")
            || t.starts_with("package ")
            || t.starts_with("import ")
            || t == "import ("
        {
            lines.next();
            if t == "import (" {
                for l in lines.by_ref() {
                    if l.trim_start().starts_with(')') {
                        break;
                    }
                }
            }
            continue;
        }
        break;
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Wrap a stripped module body with `package main` and a Go import block.
pub(crate) fn wrap_module_file(
    body: &str,
    imports: &std::collections::BTreeSet<&'static str>,
) -> String {
    let mut w = String::new();
    w.push_str("// Generated by faber package Go assembly\n");
    w.push_str("package main\n");
    if !imports.is_empty() {
        w.push('\n');
        if imports.len() == 1 {
            if let Some(imp) = imports.iter().next() {
                w.push_str(&format!("import {imp:?}\n"));
            }
        } else {
            w.push_str("import (\n");
            for imp in imports {
                w.push_str(&format!("\t{imp:?}\n"));
            }
            w.push_str(")\n");
        }
    }
    w.push('\n');
    w.push_str(body);
    if !body.ends_with('\n') {
        w.push('\n');
    }
    w
}

/// Insert namespace vars after the import block / package header of entry code.
pub(crate) fn inject_after_imports(entry_code: &str, namespaces: &str) -> String {
    if namespaces.trim().is_empty() {
        return entry_code.to_owned();
    }
    let mut out = String::new();
    let mut lines = entry_code.lines().peekable();
    let mut past_header = false;
    while let Some(line) = lines.next() {
        out.push_str(line);
        out.push('\n');
        let t = line.trim_start();
        if !past_header {
            if t.starts_with("import (") {
                // consume until closing paren already in stream - we just wrote the open
                // continue writing until )
                for l in lines.by_ref() {
                    out.push_str(l);
                    out.push('\n');
                    if l.trim_start().starts_with(')') {
                        break;
                    }
                }
                out.push('\n');
                out.push_str(namespaces);
                if !namespaces.ends_with('\n') {
                    out.push('\n');
                }
                past_header = true;
                continue;
            }
            if t.starts_with("import \"") || t.starts_with("import '") {
                // single import; check if more import lines follow
                while let Some(n) = lines.peek().copied() {
                    let nt = n.trim_start();
                    if nt.starts_with("import ") || nt.is_empty() {
                        if let Some(import_line) = lines.next() {
                            out.push_str(import_line);
                            out.push('\n');
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                out.push('\n');
                out.push_str(namespaces);
                if !namespaces.ends_with('\n') {
                    out.push('\n');
                }
                past_header = true;
                continue;
            }
            if t.starts_with("package ") {
                // may be no imports
                if lines.peek().is_some_and(|n| {
                    let nt = n.trim_start();
                    !nt.starts_with("import ") && !nt.is_empty() && !nt.starts_with("//")
                }) {
                    // next non-empty is not import — inject after blank lines
                    while let Some(n) = lines.peek().copied() {
                        if n.trim().is_empty() {
                            if let Some(blank_line) = lines.next() {
                                out.push_str(blank_line);
                                out.push('\n');
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    if lines
                        .peek()
                        .is_some_and(|n| !n.trim_start().starts_with("import "))
                    {
                        out.push_str(namespaces);
                        if !namespaces.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push('\n');
                        past_header = true;
                    }
                }
            }
        }
    }
    if !past_header {
        // Fallback: prepend namespaces after first line
        let mut rebuilt = String::new();
        let mut it = entry_code.lines();
        if let Some(first) = it.next() {
            rebuilt.push_str(first);
            rebuilt.push('\n');
            rebuilt.push('\n');
            rebuilt.push_str(namespaces);
            rebuilt.push('\n');
            for l in it {
                rebuilt.push_str(l);
                rebuilt.push('\n');
            }
            return rebuilt;
        }
    }
    out
}

/// Stable Go file name for a non-entry module unit.
pub(crate) fn module_go_file_name(segments: &[String], path: &Path) -> String {
    if segments.is_empty() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("mod");
        return format!("{stem}.go");
    }
    format!("{}.go", segments.join("_"))
}

/// On-disk layout for a assembled Go package.
#[derive(Debug, Clone)]
pub(crate) struct GoBuildLayout {
    pub module_root: PathBuf,
    pub binary_path: PathBuf,
    pub package_name: String,
}

impl GoBuildLayout {
    pub(crate) fn from_package(layout: &BuildLayout) -> Self {
        let module_root = layout.package_root.join("target").join("faber").join("go");
        let package_name = layout.binary_name().to_owned();
        let binary_path = module_root.join("bin").join(&package_name);
        Self {
            module_root,
            binary_path,
            package_name,
        }
    }
}

/// Write Go sources + `go.mod` for a single-package product assembly.
#[allow(clippy::result_large_err)]
pub(crate) fn emit_go_module(
    layout: &GoBuildLayout,
    entry_code: &str,
    modules: &[(String, String)],
) -> Result<(), Diagnostic> {
    fs::create_dir_all(&layout.module_root).map_err(|err| {
        crate::package_diagnostic_error(format!(
            "failed to create Go module root '{}': {err}",
            layout.module_root.display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;
    fs::create_dir_all(layout.module_root.join("bin")).map_err(|err| {
        crate::package_diagnostic_error(format!(
            "failed to create Go binary dir '{}': {err}",
            layout.module_root.join("bin").display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;
    remove_stale_owned_go_files(layout, modules)?;

    let main_path = layout.module_root.join("main.go");
    fs::write(&main_path, entry_code).map_err(|err| {
        crate::package_diagnostic_error(format!("failed to write '{}': {err}", main_path.display()))
            .with_arg("issue", "package_go_emit_failed")
    })?;

    for (file_name, code) in modules {
        let path = layout.module_root.join(file_name);
        // Module files from compile already include package/import via wrap_module_file.
        let has_package = code.lines().any(|l| l.trim_start().starts_with("package "));
        let file_code = if has_package {
            code.clone()
        } else {
            wrap_module_file(code, &std::collections::BTreeSet::new())
        };
        fs::write(&path, file_code).map_err(|err| {
            crate::package_diagnostic_error(format!("failed to write '{}': {err}", path.display()))
                .with_arg("issue", "package_go_emit_failed")
        })?;
    }

    let go_mod = format!(
        "module faber/{}\n\ngo 1.22\n",
        sanitize_go_module_segment(&layout.package_name)
    );
    let go_mod_path = layout.module_root.join("go.mod");
    fs::write(&go_mod_path, go_mod).map_err(|err| {
        crate::package_diagnostic_error(format!(
            "failed to write '{}': {err}",
            go_mod_path.display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;

    Ok(())
}

#[allow(clippy::result_large_err)]
fn remove_stale_owned_go_files(
    layout: &GoBuildLayout,
    modules: &[(String, String)],
) -> Result<(), Diagnostic> {
    let owned: BTreeSet<String> = std::iter::once("main.go".to_owned())
        .chain(modules.iter().map(|(file_name, _)| file_name.clone()))
        .collect();
    let entries = fs::read_dir(&layout.module_root).map_err(|err| {
        crate::package_diagnostic_error(format!(
            "failed to read Go module root '{}': {err}",
            layout.module_root.display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            crate::package_diagnostic_error(format!(
                "failed to inspect Go module root '{}': {err}",
                layout.module_root.display()
            ))
            .with_arg("issue", "package_go_emit_failed")
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("go") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if owned.contains(name) {
            continue;
        }
        fs::remove_file(&path).map_err(|err| {
            crate::package_diagnostic_error(format!(
                "failed to remove stale '{}': {err}",
                path.display()
            ))
            .with_arg("issue", "package_go_emit_failed")
        })?;
    }
    Ok(())
}

/// Invoke `go build` for an emitted module; returns the binary path.
#[allow(clippy::result_large_err)]
pub(crate) fn invoke_go_build(layout: &GoBuildLayout) -> Result<PathBuf, Diagnostic> {
    if let Some(parent) = layout.binary_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            crate::package_diagnostic_error(format!(
                "failed to create '{}': {err}",
                parent.display()
            ))
            .with_arg("issue", "package_go_build_failed")
        })?;
    }

    let output = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&layout.binary_path)
        .arg(".")
        .current_dir(&layout.module_root)
        .output()
        .map_err(|err| {
            crate::package_diagnostic_error(format!("failed to execute `go build`: {err}"))
                .with_arg("issue", "package_go_build_failed")
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(crate::package_diagnostic_error(format!(
            "go build failed for '{}':\n{stderr}{stdout}",
            layout.module_root.display()
        ))
        .with_arg("issue", "package_go_build_failed"));
    }

    if !layout.binary_path.exists() {
        return Err(crate::package_diagnostic_error(format!(
            "go build reported success but binary missing at '{}'",
            layout.binary_path.display()
        ))
        .with_arg("issue", "package_go_build_failed"));
    }

    Ok(layout.binary_path.clone())
}

fn sanitize_go_module_segment(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Run a built Go binary with forwarded argv.
#[allow(dead_code)] // used by binary `commands/run` (not the lib test surface)
#[allow(clippy::result_large_err)]
pub(crate) fn run_go_binary(binary: &Path, args: &[String]) -> Result<i32, Diagnostic> {
    let status = Command::new(binary).args(args).status().map_err(|err| {
        crate::package_diagnostic_error(format!("failed to execute '{}': {err}", binary.display()))
            .with_arg("issue", "package_go_run_failed")
    })?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
#[path = "go_build_test.rs"]
mod tests;
