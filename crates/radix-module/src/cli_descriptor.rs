//! Versioned static CLI entry descriptor (campaign D12, Stage 8 S8.2).
//
// Wire counts are u8/u16 by format contract; truncation is intentional and
// bounded by CLI schema validation before encode.
#![allow(clippy::cast_possible_truncation)]
//!
//! The compiler serializes the analyzed [`CliProgram`] facts into a versioned,
//! target-neutral static descriptor that the LLVM host runtime parses argv
//! against at process start. CLI metadata is generated from CliProgram/CLI IR
//! — never reparsed from source — and no CLI logic is keyed by exemplar path
//! (Checkpoint B).
//!
//! The descriptor is a compact, deterministic byte encoding. The Faber LLVM
//! host (`hosts/llvm/src/cli.rs`) decodes this exact format
//! — the encoder here and that decoder must stay in lockstep (they are pinned
//! by the version byte and covered by independent round-trip tests on this
//! side and decode tests on the host side).
//!
//! Byte format (v1):
//! ```text
//! magic            4 bytes  "FCLI"
//! version          1 byte   CLI_DESCRIPTOR_VERSION
//! mode             1 byte   0 = SingleCommand, 1 = Subcommand
//! has_version      1 byte
//! has_description  1 byte
//! name             str
//! [version]        str       if has_version
//! [description]    str       if has_description
//! exit_tag         1 byte   0 none, 1 fixed, 2 binding, 3 field, 4 unsupported
//!   [i64]                    fixed
//!   [str]                    binding
//!   [str, str]               field (object, member)
//! global_options   u16 count + option records
//! global_operands  u16 count + operand records
//! options          u16 count + option records
//! operands         u16 count + operand records
//! commands         u16 count + command records
//! ```
//!
//! `str` is a u16 little-endian length followed by UTF-8 bytes. Integer
//! payloads are i64 little-endian; floats are f64 little-endian.

use crate::cli::{CliCommand, CliDefault, CliExit, CliMode, CliOperand, CliOption, CliProgram, CliType};
#[cfg(feature = "mir-llvm")]
use radix_mir_llvm::{CliAdapterField, CliAdapterPlan, CliRecordFunction};

/// Current static CLI descriptor version.
pub const CLI_DESCRIPTOR_VERSION: u8 = 1;

const CLI_DESCRIPTOR_MAGIC: &[u8; 4] = b"FCLI";

/// CLI value type tags shared by the encoder and the host decoder.
pub mod tags {
    pub const TEXTUS: u8 = 0;
    pub const NUMERUS: u8 = 1;
    pub const FRACTUS: u8 = 2;
    pub const BIVALENS: u8 = 3;
    pub const OCTETI: u8 = 4;
    pub const IGNOTUM: u8 = 5;
    pub const LISTA_TEXTUS: u8 = 6;
    pub const LISTA_NUMERUS: u8 = 7;
}

/// Default value payload tags.
pub mod default_tags {
    pub const TEXT: u8 = 0;
    pub const INTEGER: u8 = 1;
    pub const FLOAT: u8 = 2;
    pub const BOOL: u8 = 3;
    pub const NIL: u8 = 4;
    pub const EXPR: u8 = 5;
}

/// Exit policy tags.
pub mod exit_tags {
    pub const NONE: u8 = 0;
    pub const FIXED: u8 = 1;
    pub const BINDING: u8 = 2;
    pub const FIELD: u8 = 3;
    pub const UNSUPPORTED: u8 = 4;
}

#[derive(Debug, Clone, PartialEq)]
pub enum DescriptorExit {
    None,
    Fixed(i64),
    Binding(String),
    Field { object: String, field: String },
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DescriptorDefault {
    pub tag: u8,
    pub text: Option<String>,
    pub integer: Option<i64>,
    pub float: Option<f64>,
    pub boolean: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DescriptorOption {
    pub binding: String,
    pub ty: u8,
    pub short: Option<String>,
    pub long: Option<String>,
    pub description: Option<String>,
    pub default: Option<DescriptorDefault>,
    pub flag: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DescriptorOperand {
    pub binding: String,
    pub ty: u8,
    pub rest: bool,
    pub description: Option<String>,
    pub default: Option<DescriptorDefault>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DescriptorCommand {
    pub path: Vec<String>,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub function: String,
    pub args_binding: Option<String>,
    pub options: Vec<DescriptorOption>,
    pub operands: Vec<DescriptorOperand>,
}

/// Decoded form of the versioned static CLI descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct CliDescriptorV1 {
    pub name: String,
    pub mode: u8,
    pub version: Option<String>,
    pub description: Option<String>,
    pub exit: DescriptorExit,
    pub global_options: Vec<DescriptorOption>,
    pub global_operands: Vec<DescriptorOperand>,
    pub options: Vec<DescriptorOption>,
    pub operands: Vec<DescriptorOperand>,
    pub commands: Vec<DescriptorCommand>,
}

/// Serialize an analyzed [`CliProgram`] into the versioned static descriptor.
#[must_use]
pub fn serialize_cli_descriptor(program: &CliProgram) -> Vec<u8> {
    let mut writer = DescrWriter::default();
    writer.bytes(CLI_DESCRIPTOR_MAGIC);
    writer.u8(CLI_DESCRIPTOR_VERSION);
    writer.u8(match program.mode {
        CliMode::SingleCommand => 0,
        CliMode::Subcommand => 1,
        CliMode::NotCli => 0,
    });
    writer.u8(u8::from(program.version.is_some()));
    writer.u8(u8::from(program.description.is_some()));
    writer.string(&program.name);
    if let Some(version) = &program.version {
        writer.string(version);
    }
    if let Some(description) = &program.description {
        writer.string(description);
    }
    write_exit(&mut writer, program.exit.as_ref());
    write_options(&mut writer, &program.global_options);
    write_operands(&mut writer, &program.global_operands);
    write_options(&mut writer, &program.options);
    write_operands(&mut writer, &program.operands);
    let commands = program.commands.len();
    writer.u16(commands as u16);
    for command in &program.commands {
        write_command(&mut writer, command);
    }
    writer.finish()
}

fn write_exit(writer: &mut DescrWriter, exit: Option<&CliExit>) {
    match exit {
        None => writer.u8(exit_tags::NONE),
        Some(CliExit::Fixed(code)) => {
            writer.u8(exit_tags::FIXED);
            writer.i64(*code);
        }
        Some(CliExit::Binding(binding)) => {
            writer.u8(exit_tags::BINDING);
            writer.string(binding);
        }
        Some(CliExit::Field { object, field }) => {
            writer.u8(exit_tags::FIELD);
            writer.string(object);
            writer.string(field);
        }
        Some(CliExit::Unsupported) => writer.u8(exit_tags::UNSUPPORTED),
    }
}

fn write_options(writer: &mut DescrWriter, options: &[CliOption]) {
    writer.u16(options.len() as u16);
    for option in options {
        writer.u8(cli_type_tag(&option.ty));
        writer.u8(u8::from(option.flag));
        writer.opt_string(option.short.as_ref());
        writer.opt_string(option.long.as_ref());
        writer.opt_string(option.description.as_ref());
        writer.opt_default(option.default.as_ref());
        writer.string(&option.binding);
    }
}

fn write_operands(writer: &mut DescrWriter, operands: &[CliOperand]) {
    writer.u16(operands.len() as u16);
    for operand in operands {
        writer.u8(cli_type_tag(&operand.ty));
        writer.u8(u8::from(operand.rest));
        writer.opt_string(operand.description.as_ref());
        writer.opt_default(operand.default.as_ref());
        writer.string(&operand.binding);
    }
}

fn write_command(writer: &mut DescrWriter, command: &CliCommand) {
    writer.u8(command.path.len() as u8);
    for segment in &command.path {
        writer.string(segment);
    }
    writer.u8(command.aliases.len() as u8);
    for alias in &command.aliases {
        writer.string(alias);
    }
    writer.opt_string(command.description.as_ref());
    writer.string(&command.function);
    writer.opt_string(command.args_binding.as_ref());
    write_options(writer, &command.options);
    write_operands(writer, &command.operands);
}

fn cli_type_tag(ty: &CliType) -> u8 {
    match ty {
        CliType::Textus => tags::TEXTUS,
        CliType::Numerus => tags::NUMERUS,
        CliType::Fractus => tags::FRACTUS,
        CliType::Bivalens => tags::BIVALENS,
        CliType::Octeti => tags::OCTETI,
        CliType::Ignotum => tags::IGNOTUM,
        CliType::ListaTextus => tags::LISTA_TEXTUS,
        CliType::ListaNumerus => tags::LISTA_NUMERUS,
    }
}

/// Build the static CLI adapter plan for the LLVM emitter: the serialized
/// descriptor plus the per-function record surfaces (entry first, then
/// commands in descriptor order) the emitted adapter seeds from the runtime
/// typed value table.
#[cfg(feature = "mir-llvm")]
#[must_use]
pub fn build_cli_adapter_plan(program: &CliProgram) -> CliAdapterPlan {
    let mut record_functions = Vec::with_capacity(program.commands.len() + 1);
    // Entry record surface: global + root-local options then operands, in the
    // same order `incipit_args_type` builds the entry record type.
    record_functions.push(CliRecordFunction {
        function: None,
        fields: cli_record_fields(
            program.global_options.iter().chain(program.options.iter()),
            program
                .global_operands
                .iter()
                .chain(program.operands.iter()),
        ),
    });
    for command in &program.commands {
        // Only commands with an `argumenta` binding carry a record surface;
        // dispatch-only commands stay out of the record plan.
        if command.args_binding.is_some() {
            record_functions.push(CliRecordFunction {
                function: Some(command.function_symbol),
                fields: cli_record_fields(
                    program.global_options.iter().chain(command.options.iter()),
                    program
                        .global_operands
                        .iter()
                        .chain(command.operands.iter()),
                ),
            });
        }
    }
    CliAdapterPlan {
        descriptor: serialize_cli_descriptor(program),
        subcommand: program.mode == CliMode::Subcommand,
        commands: program
            .commands
            .iter()
            .map(|command| command.function_symbol)
            .collect(),
        record_functions,
    }
}

#[cfg(feature = "mir-llvm")]
fn cli_record_fields<'a>(
    options: impl Iterator<Item = &'a CliOption> + 'a,
    operands: impl Iterator<Item = &'a CliOperand> + 'a,
) -> Vec<CliAdapterField> {
    let mut fields = Vec::new();
    for option in options {
        fields.push(CliAdapterField {
            name: option.binding_symbol,
            ty: cli_type_tag(&option.ty),
            optional: option.default.is_none() && !option.flag,
            list: false,
        });
    }
    for operand in operands {
        fields.push(CliAdapterField {
            name: operand.binding_symbol,
            ty: cli_type_tag(&operand.ty),
            optional: false,
            list: operand.rest || matches!(operand.ty, CliType::ListaTextus | CliType::ListaNumerus),
        });
    }
    fields
}

/// Decode a versioned static CLI descriptor.
///
/// # Errors
///
/// Returns [`CliDescriptorError`] for truncated payloads, an unknown version,
/// a missing magic, or out-of-range counts.
pub fn parse_cli_descriptor(bytes: &[u8]) -> Result<CliDescriptorV1, CliDescriptorError> {
    let mut reader = DescrReader::new(bytes)?;
    if reader.bytes(CLI_DESCRIPTOR_MAGIC.len())? != CLI_DESCRIPTOR_MAGIC {
        return Err(CliDescriptorError::bad_magic());
    }
    let version = reader.u8()?;
    if version != CLI_DESCRIPTOR_VERSION {
        return Err(CliDescriptorError::unsupported_version(version));
    }
    let mode = reader.u8()?;
    let has_version = reader.u8()? != 0;
    let has_description = reader.u8()? != 0;
    let name = reader.string()?;
    let version = has_version.then(|| reader.string()).transpose()?;
    let description = has_description.then(|| reader.string()).transpose()?;
    let exit = read_exit(&mut reader)?;
    let global_options = read_options(&mut reader)?;
    let global_operands = read_operands(&mut reader)?;
    let options = read_options(&mut reader)?;
    let operands = read_operands(&mut reader)?;
    let command_count = reader.u16()? as usize;
    let mut commands = Vec::with_capacity(command_count);
    for _ in 0..command_count {
        commands.push(read_command(&mut reader)?);
    }
    Ok(CliDescriptorV1 {
        name,
        mode,
        version,
        description,
        exit,
        global_options,
        global_operands,
        options,
        operands,
        commands,
    })
}

fn read_exit(reader: &mut DescrReader<'_>) -> Result<DescriptorExit, CliDescriptorError> {
    match reader.u8()? {
        exit_tags::NONE => Ok(DescriptorExit::None),
        exit_tags::FIXED => Ok(DescriptorExit::Fixed(reader.i64()?)),
        exit_tags::BINDING => Ok(DescriptorExit::Binding(reader.string()?)),
        exit_tags::FIELD => Ok(DescriptorExit::Field { object: reader.string()?, field: reader.string()? }),
        exit_tags::UNSUPPORTED => Ok(DescriptorExit::Unsupported),
        other => Err(CliDescriptorError::bad_tag("exit", other)),
    }
}

fn read_options(reader: &mut DescrReader<'_>) -> Result<Vec<DescriptorOption>, CliDescriptorError> {
    let count = reader.u16()? as usize;
    let mut options = Vec::with_capacity(count);
    for _ in 0..count {
        options.push(DescriptorOption {
            ty: reader.u8()?,
            flag: reader.u8()? != 0,
            short: reader.opt_string()?,
            long: reader.opt_string()?,
            description: reader.opt_string()?,
            default: reader.opt_default()?,
            binding: reader.string()?,
        });
    }
    Ok(options)
}

fn read_operands(reader: &mut DescrReader<'_>) -> Result<Vec<DescriptorOperand>, CliDescriptorError> {
    let count = reader.u16()? as usize;
    let mut operands = Vec::with_capacity(count);
    for _ in 0..count {
        operands.push(DescriptorOperand {
            ty: reader.u8()?,
            rest: reader.u8()? != 0,
            description: reader.opt_string()?,
            default: reader.opt_default()?,
            binding: reader.string()?,
        });
    }
    Ok(operands)
}

fn read_command(reader: &mut DescrReader<'_>) -> Result<DescriptorCommand, CliDescriptorError> {
    let path_len = reader.u8()? as usize;
    let mut path = Vec::with_capacity(path_len);
    for _ in 0..path_len {
        path.push(reader.string()?);
    }
    let alias_count = reader.u8()? as usize;
    let mut aliases = Vec::with_capacity(alias_count);
    for _ in 0..alias_count {
        aliases.push(reader.string()?);
    }
    let description = reader.opt_string()?;
    let function = reader.string()?;
    let args_binding = reader.opt_string()?;
    let options = read_options(reader)?;
    let operands = read_operands(reader)?;
    Ok(DescriptorCommand { path, aliases, description, function, args_binding, options, operands })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliDescriptorError {
    pub reason: String,
}

impl CliDescriptorError {
    fn bad_magic() -> Self {
        Self { reason: "bad descriptor magic".to_owned() }
    }

    fn unsupported_version(version: u8) -> Self {
        Self { reason: format!("unsupported CLI descriptor version {version}") }
    }

    fn bad_tag(kind: &str, tag: u8) -> Self {
        Self { reason: format!("unknown {kind} tag {tag}") }
    }

    fn truncated() -> Self {
        Self { reason: "truncated CLI descriptor".to_owned() }
    }

    fn count_overflow() -> Self {
        Self { reason: "CLI descriptor count overflow".to_owned() }
    }
}

#[derive(Default)]
struct DescrWriter {
    bytes: Vec<u8>,
}

impl DescrWriter {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn bytes(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u16(value.len() as u16);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn opt_string(&mut self, value: Option<&String>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.string(value);
            }
            None => self.u8(0),
        }
    }

    fn opt_default(&mut self, value: Option<&CliDefault>) {
        match value {
            None => self.u8(0),
            Some(default) => {
                self.u8(1);
                match default {
                    CliDefault::Text(value) => {
                        self.u8(default_tags::TEXT);
                        self.string(value);
                    }
                    CliDefault::Integer(value) => {
                        self.u8(default_tags::INTEGER);
                        self.i64(*value);
                    }
                    CliDefault::Float(value) => {
                        self.u8(default_tags::FLOAT);
                        self.f64(*value);
                    }
                    CliDefault::Bool(value) => {
                        self.u8(default_tags::BOOL);
                        self.u8(u8::from(*value));
                    }
                    CliDefault::Nil => {
                        self.u8(default_tags::NIL);
                    }
                    CliDefault::Expr(value) => {
                        self.u8(default_tags::EXPR);
                        self.string(value);
                    }
                }
            }
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

struct DescrReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> DescrReader<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, CliDescriptorError> {
        if bytes.len() < 5 {
            return Err(CliDescriptorError::truncated());
        }
        Ok(Self { bytes, offset: 0 })
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CliDescriptorError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(CliDescriptorError::count_overflow)?;
        if end > self.bytes.len() {
            return Err(CliDescriptorError::truncated());
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, CliDescriptorError> {
        Ok(self.take(1)?[0])
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], CliDescriptorError> {
        self.take(len)
    }

    fn u16(&mut self) -> Result<u16, CliDescriptorError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn i64(&mut self) -> Result<i64, CliDescriptorError> {
        let bytes = self.take(8)?;
        let mut arr = [0_u8; 8];
        arr.copy_from_slice(bytes);
        Ok(i64::from_le_bytes(arr))
    }

    fn f64(&mut self) -> Result<f64, CliDescriptorError> {
        let bytes = self.take(8)?;
        let mut arr = [0_u8; 8];
        arr.copy_from_slice(bytes);
        Ok(f64::from_le_bytes(arr))
    }

    fn string(&mut self) -> Result<String, CliDescriptorError> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| CliDescriptorError { reason: "non-UTF-8 CLI descriptor string".to_owned() })
    }

    fn opt_string(&mut self) -> Result<Option<String>, CliDescriptorError> {
        if self.u8()? == 0 {
            Ok(None)
        } else {
            self.string().map(Some)
        }
    }

    fn opt_default(&mut self) -> Result<Option<DescriptorDefault>, CliDescriptorError> {
        if self.u8()? == 0 {
            return Ok(None);
        }
        let default = match self.u8()? {
            default_tags::TEXT => {
                DescriptorDefault { tag: default_tags::TEXT, text: Some(self.string()?), ..DescriptorDefault::none() }
            }
            default_tags::EXPR => {
                DescriptorDefault { tag: default_tags::EXPR, text: Some(self.string()?), ..DescriptorDefault::none() }
            }
            default_tags::INTEGER => DescriptorDefault {
                tag: default_tags::INTEGER,
                integer: Some(self.i64()?),
                ..DescriptorDefault::none()
            },
            default_tags::FLOAT => {
                DescriptorDefault { tag: default_tags::FLOAT, float: Some(self.f64()?), ..DescriptorDefault::none() }
            }
            default_tags::BOOL => DescriptorDefault {
                tag: default_tags::BOOL,
                boolean: Some(self.u8()? != 0),
                ..DescriptorDefault::none()
            },
            default_tags::NIL => DescriptorDefault { tag: default_tags::NIL, ..DescriptorDefault::none() },
            other => return Err(CliDescriptorError::bad_tag("default", other)),
        };
        Ok(Some(default))
    }
}

impl DescriptorDefault {
    fn none() -> Self {
        Self { tag: 0, text: None, integer: None, float: None, boolean: None }
    }
}

#[cfg(test)]
#[path = "cli_descriptor_test.rs"]
mod tests;
