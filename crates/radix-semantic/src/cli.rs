//! Target-independent CLI surface analysis.
//!
//! Faber source can declare command-line programs with annotations such as
//! `@ cli`, `@ imperium`, `@ optio`, and `@ operandus`. This module turns those
//! annotations into a normalized CLI IR that later compiler phases and codegen
//! can consume without re-reading syntax details from the AST.
//!
//! The analyzer is deliberately target-independent. It validates the language
//! contract for command paths, option names, operand ordering, global surface
//! placement, mounted command modules, and supported CLI-facing types, but it
//! does not decide how Rust, TypeScript, or another backend should parse argv.
//!
//! INVARIANTS
//! ==========
//! - A root CLI program has at most one `@ cli` incipit entry point.
//! - Command modules mounted into another CLI may expose `@ imperium` commands
//!   but must not declare their own root `@ cli` entry point.
//! - Global options and operands use `ubique` and are owned by the root entry
//!   point, not by individual commands.
//! - Validation errors are semantic diagnostics; analysis still returns the
//!   best normalized shape it can construct for tooling and inspection.

use crate::{SemanticError, SemanticErrorKind};
use handroll::{FxHashMap, FxHashSet};
use radix_lexer::{Interner, Span};
use radix_syntax::{
    Annotation, AnnotationKind, BracedAnnotationError, BracedView, Expr, ExprKind, FuncDecl,
    GenericArgExpr, Ident, IncipitStmt, Literal, MemberExpr, OperandusAnnotation, OptioAnnotation,
    Program, Stmt, StmtKind, TypeExpr, TypeExprKind, braced_cli_nomen, braced_cli_nomen_strict,
    braced_imperium_nomen_strict, braced_operandus_view_strict, braced_optio_view_strict,
};

const ISSUE_ALIAS_COLLIDES_WITH_PATH: &str = "alias_collides_with_path";
const ISSUE_CLI_ON_NON_INCIPIT: &str = "cli_on_non_incipit";
const ISSUE_COMMAND_GLOBAL_SURFACE: &str = "command_global_surface";
const ISSUE_COMMAND_PARAMS_UNSUPPORTED: &str = "command_params_unsupported";
const ISSUE_DUPLICATE_BINDING: &str = "duplicate_binding";
const ISSUE_DUPLICATE_CLI_ENTRY: &str = "duplicate_cli_entry";
const ISSUE_DUPLICATE_COMMAND_ALIAS: &str = "duplicate_command_alias";
const ISSUE_DUPLICATE_COMMAND_PATH: &str = "duplicate_command_path";
const ISSUE_DUPLICATE_LONG_FLAG: &str = "duplicate_long_flag";
const ISSUE_DUPLICATE_REST_OPERAND: &str = "duplicate_rest_operand";
const ISSUE_DUPLICATE_SHORT_FLAG: &str = "duplicate_short_flag";
const ISSUE_GLOBAL_OPERAND_PLACEMENT: &str = "global_operand_placement";
const ISSUE_GLOBAL_OPTION_PLACEMENT: &str = "global_option_placement";
const ISSUE_IMPERIUM_ON_NON_FUNC: &str = "imperium_on_non_func";
const ISSUE_INVALID_COMMAND_ALIAS: &str = "invalid_command_alias";
const ISSUE_INVALID_COMMAND_PATH: &str = "invalid_command_path";
const ISSUE_INVALID_LONG_FLAG: &str = "invalid_long_flag";
const ISSUE_INVALID_SHORT_FLAG: &str = "invalid_short_flag";
const ISSUE_LIST_ELEMENT_NOT_NAMED: &str = "list_element_not_named";
const ISSUE_LOCAL_OPERAND_GLOBAL_COLLISION: &str = "local_operand_global_collision";
const ISSUE_LOCAL_OPTION_GLOBAL_COLLISION: &str = "local_option_global_collision";
const ISSUE_MISSING_ENTRY_ARGUMENT: &str = "missing_entry_argument";
const ISSUE_MISSING_OPTION_FLAG: &str = "missing_option_flag";
const ISSUE_MOUNTED_MODULE_ROOT_CLI: &str = "mounted_module_root_cli";
const ISSUE_NESTED_MODULE_MOUNT: &str = "nested_module_mount";
const ISSUE_OPERAND_AFTER_REST: &str = "operand_after_rest";
const ISSUE_ORPHAN_CLI_SURFACE: &str = "orphan_cli_surface";
const ISSUE_REST_NOT_FINAL: &str = "rest_not_final";
const ISSUE_SUBCOMMAND_LOCAL_SURFACE: &str = "subcommand_local_surface";
const ISSUE_SUBCOMMAND_ROOT_BODY: &str = "subcommand_root_body";
const ISSUE_UNSUPPORTED_LIST_ELEMENT_TYPE: &str = "unsupported_list_element_type";
const ISSUE_UNSUPPORTED_TYPE: &str = "unsupported_type";
const ISSUE_UNSUPPORTED_TYPE_SHAPE: &str = "unsupported_type_shape";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    /// Program contains no CLI entry point or mounted command surface.
    NotCli,

    /// One `@ cli` incipit owns the complete option and operand surface.
    SingleCommand,

    /// Root entry point dispatches to one or more `@ imperium` commands.
    Subcommand,
}

/// Result of extracting a CLI surface from one parsed program.
///
/// The analyzer separates structural mode from the optional normalized program
/// because mounted modules may be non-CLI files, and invalid CLI files still
/// need diagnostics even when a partial IR exists.
#[derive(Debug, Clone)]
pub struct CliAnalysis {
    /// Classification of the parsed source as a CLI surface.
    pub mode: CliMode,

    /// Normalized CLI contract, present when a root or mounted command surface
    /// was found.
    pub program: Option<CliProgram>,

    /// Semantic validation errors found while reading CLI annotations.
    pub errors: Vec<SemanticError>,
}

/// Normalized command-line program contract consumed by codegen and tooling.
///
/// This type intentionally models the CLI surface, not the implementation body.
/// Bindings point back to Faber argument records and command functions so later
/// phases can generate parser glue without depending on annotation syntax.
#[derive(Debug, Clone)]
pub struct CliProgram {
    /// User-facing command name from the root `@ cli` annotation.
    pub name: String,

    /// Binding name used by the root `incipit argumenta <ident>` declaration.
    pub entry_args: String,

    /// Whether this surface is a single command or subcommand dispatcher.
    pub mode: CliMode,

    /// Optional version metadata declared with `@ versio`.
    pub version: Option<String>,

    /// Optional long description declared with `@ descriptio`.
    pub description: Option<String>,

    /// Options available to every command through `ubique`.
    pub global_options: Vec<CliOption>,

    /// Operands available to every command through `ubique`.
    pub global_operands: Vec<CliOperand>,

    /// Single-command local options declared on the root entry point.
    pub options: Vec<CliOption>,

    /// Single-command local operands declared on the root entry point.
    pub operands: Vec<CliOperand>,

    /// Subcommands declared by `@ imperium` functions.
    pub commands: Vec<CliCommand>,

    /// Root exit expression reduced to the forms codegen currently supports.
    pub exit: Option<CliExit>,
}

/// Exit-code expression captured from a CLI `incipit`.
///
/// The variants preserve enough structure for codegen to emit stable exit
/// handling while refusing to invent meaning for arbitrary expressions.
#[derive(Debug, Clone)]
pub enum CliExit {
    /// Literal numeric exit status.
    Fixed(i64),

    /// Exit status comes from a local binding.
    Binding(String),

    /// Exit status comes from a field on a local binding.
    Field { object: String, field: String },

    /// Expression was present but cannot be lowered as a CLI exit contract.
    Unsupported,
}

/// Normalized subcommand declaration.
///
/// A command path is already split into routing segments. Mounted modules may
/// prefix the path and aliases after local validation, which is why this type
/// carries both source-local function identity and user-facing route metadata.
#[derive(Debug, Clone)]
pub struct CliCommand {
    /// Slash-separated route segments, stored normalized for collision checks.
    pub path: Vec<String>,

    /// Module path when this command came from an imported command module.
    pub module_path: Option<Vec<String>>,

    /// Faber function name that implements the command.
    pub function: String,

    /// Interned function symbol for later semantic/codegen lookup.
    pub function_symbol: radix_lexer::Symbol,

    /// Optional `argumenta <ident>` binding used by the command function.
    pub args_binding: Option<String>,

    /// Alternate route names declared with `@ alias`.
    pub aliases: Vec<String>,

    /// Optional user-facing command description.
    pub description: Option<String>,

    /// Options local to this command.
    pub options: Vec<CliOption>,

    /// Positional operands local to this command.
    pub operands: Vec<CliOperand>,

    /// Source span used for command-level diagnostics.
    pub span: Span,
}

/// Normalized option declared with `@ optio`.
#[derive(Debug, Clone)]
pub struct CliOption {
    /// Faber binding name that receives the parsed value.
    pub binding: String,

    /// Interned binding symbol for semantic/codegen lookup.
    pub binding_symbol: radix_lexer::Symbol,

    /// CLI-supported value type after syntax normalization.
    pub ty: CliType,

    /// Single-character short flag without leading `-`.
    pub short: Option<String>,

    /// Long flag without leading `--`.
    pub long: Option<String>,

    /// Optional user-facing help text.
    pub description: Option<String>,

    /// Whether the option is global through `ubique`.
    pub global: bool,

    /// Default value declared in source or synthesized for boolean flags.
    pub default: Option<CliDefault>,

    /// True when the option is a boolean flag rather than value-taking option.
    pub flag: bool,

    /// Source span used for option diagnostics.
    pub span: Span,
}

/// Normalized positional operand declared with `@ operandus`.
#[derive(Debug, Clone)]
pub struct CliOperand {
    /// Faber binding name that receives the parsed value.
    pub binding: String,

    /// Interned binding symbol for semantic/codegen lookup.
    pub binding_symbol: radix_lexer::Symbol,

    /// CLI-supported value type after syntax normalization.
    pub ty: CliType,

    /// Whether this operand captures the remaining positional arguments.
    pub rest: bool,

    /// Optional user-facing help text.
    pub description: Option<String>,

    /// Whether the operand is global through `ubique`.
    pub global: bool,

    /// Default value declared in source.
    pub default: Option<CliDefault>,

    /// Source span used for operand diagnostics.
    pub span: Span,
}

/// Faber types currently accepted at the CLI boundary.
///
/// The CLI type set is narrower than the language type system because argv
/// parsing needs an explicit, predictable conversion contract. Unsupported
/// types are reported during analysis instead of being guessed in codegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliType {
    /// `textus`
    Textus,

    /// `numerus`
    Numerus,

    /// `fractus`
    Fractus,

    /// `bivalens`
    Bivalens,

    /// `octeti`
    Octeti,

    /// `ignotum`
    Ignotum,

    /// `lista<textus>`
    ListaTextus,

    /// `lista<numerus>`
    ListaNumerus,
}

/// Default value captured from CLI annotation syntax.
///
/// Literal defaults keep typed shape. Non-literal expressions are preserved as
/// debug text so inspection surfaces can explain what was seen while validation
/// and codegen decide whether the form is supported.
#[derive(Debug, Clone)]
pub enum CliDefault {
    /// String literal default.
    Text(String),

    /// Integer literal default.
    Integer(i64),

    /// Floating-point literal default.
    Float(f64),

    /// Boolean literal default.
    Bool(bool),

    /// `nihil` default.
    Nil,

    /// Non-literal expression captured for diagnostics/inspection.
    Expr(String),
}

/// Analyze a parsed root program for a CLI surface.
#[must_use]
pub fn analyze(program: &Program, interner: &Interner) -> CliAnalysis {
    let mut builder = CliBuilder {
        interner,
        errors: Vec::new(),
    };
    builder.analyze(program)
}

/// Analyze an imported command module and prefix its exposed routes.
///
/// Mounted modules are only allowed to contribute subcommands. They cannot
/// declare a root `@ cli` entry point or nested command mounts because the root
/// package owns dispatch topology.
#[must_use]
pub fn analyze_mounted_module(
    program: &Program,
    interner: &Interner,
    mount_prefix: &[String],
) -> CliAnalysis {
    let mut builder = CliBuilder {
        interner,
        errors: Vec::new(),
    };
    builder.analyze_mounted_module(program, mount_prefix)
}

struct CliBuilder<'a> {
    interner: &'a Interner,
    errors: Vec<SemanticError>,
}

struct RootCliProgramInput<'a> {
    entry_stmt: &'a Stmt,
    entry: &'a IncipitStmt,
    cli_name: radix_lexer::Symbol,
    entry_args: String,
    mode: CliMode,
    globals: CliSurface,
    single: CliSurface,
    commands: Vec<CliCommand>,
}

impl CliBuilder<'_> {
    fn analyze(&mut self, program: &Program) -> CliAnalysis {
        let cli_stmt = self.find_cli_stmt(program);
        let Some((entry_stmt, entry, cli_name)) = cli_stmt else {
            return CliAnalysis {
                mode: CliMode::NotCli,
                program: None,
                errors: std::mem::take(&mut self.errors),
            };
        };

        let entry_args = self.entry_args(entry, entry_stmt.span);
        let (globals, single) = self.collect_entry_surfaces(&entry_stmt.annotations);

        let commands = self.collect_commands(program);
        let mode = self.root_mode(&commands, &single, entry_stmt.span, entry);

        self.validate_root_analysis(mode, &globals, &single, &commands);
        let cli_program = self.root_cli_program(RootCliProgramInput {
            entry_stmt,
            entry,
            cli_name,
            entry_args,
            mode,
            globals,
            single,
            commands,
        });

        CliAnalysis {
            mode,
            program: Some(cli_program),
            errors: std::mem::take(&mut self.errors),
        }
    }

    fn analyze_mounted_module(
        &mut self,
        program: &Program,
        mount_prefix: &[String],
    ) -> CliAnalysis {
        if let Some((stmt, _, _)) = self.find_cli_stmt(program) {
            self.error(stmt.span, ISSUE_MOUNTED_MODULE_ROOT_CLI);
        }
        self.reject_nested_module_mounts(program);

        let mut commands = self.collect_commands(program);
        // WHY: validate once, as authored. Prefixing preserves every path and
        // alias collision, and the authored-alias `/` check only holds before
        // the mount prefix is added; a second pass reports each error twice.
        self.validate_commands(&commands, &CliSurface::default());
        prefix_mounted_commands(&mut commands, mount_prefix);

        self.mounted_analysis(commands)
    }

    fn entry_args(&mut self, entry: &IncipitStmt, span: Span) -> String {
        if let Some(args) = &entry.args {
            self.ident(args)
        } else {
            self.error(span, ISSUE_MISSING_ENTRY_ARGUMENT);
            String::new()
        }
    }

    fn collect_entry_surfaces(&mut self, annotations: &[Annotation]) -> (CliSurface, CliSurface) {
        let mut globals = CliSurface::default();
        let mut single = CliSurface::default();
        self.collect_surface(
            annotations,
            SurfacePlacement::TopLevel,
            &mut globals,
            &mut single,
        );
        (globals, single)
    }

    fn root_mode(
        &mut self,
        commands: &[CliCommand],
        single: &CliSurface,
        entry_span: Span,
        entry: &IncipitStmt,
    ) -> CliMode {
        if commands.is_empty() {
            return CliMode::SingleCommand;
        }

        if !single.options.is_empty() || !single.operands.is_empty() {
            self.error(entry_span, ISSUE_SUBCOMMAND_LOCAL_SURFACE);
        }
        if !incipit_body_is_empty(&entry.body) {
            self.error(entry_span, ISSUE_SUBCOMMAND_ROOT_BODY);
        }
        CliMode::Subcommand
    }

    fn validate_root_analysis(
        &mut self,
        mode: CliMode,
        globals: &CliSurface,
        single: &CliSurface,
        commands: &[CliCommand],
    ) {
        self.validate_surface("top-level", &globals.options, &globals.operands);
        self.validate_surface("single-command", &single.options, &single.operands);
        if mode == CliMode::SingleCommand {
            self.validate_global_surface_collisions(
                "single-command",
                &single.options,
                &single.operands,
                globals,
            );
        }
        self.validate_commands(commands, globals);
    }

    fn root_cli_program(&self, input: RootCliProgramInput<'_>) -> CliProgram {
        let RootCliProgramInput {
            entry_stmt,
            entry,
            cli_name,
            entry_args,
            mode,
            globals,
            single,
            commands,
        } = input;
        CliProgram {
            name: self.resolve(cli_name),
            entry_args,
            mode,
            version: string_metadata(&entry_stmt.annotations, self.interner, "versio"),
            description: string_metadata(&entry_stmt.annotations, self.interner, "descriptio"),
            global_options: globals.options,
            global_operands: globals.operands,
            options: single.options,
            operands: single.operands,
            commands,
            exit: entry.exitus.as_deref().map(|expr| self.exit(expr)),
        }
    }

    fn mounted_analysis(&mut self, commands: Vec<CliCommand>) -> CliAnalysis {
        let mode = if commands.is_empty() {
            CliMode::NotCli
        } else {
            CliMode::Subcommand
        };
        let program = if commands.is_empty() {
            None
        } else {
            Some(CliProgram {
                name: String::new(),
                entry_args: String::new(),
                mode: CliMode::Subcommand,
                version: None,
                description: None,
                global_options: Vec::new(),
                global_operands: Vec::new(),
                options: Vec::new(),
                operands: Vec::new(),
                commands,
                exit: None,
            })
        };

        CliAnalysis {
            mode,
            program,
            errors: std::mem::take(&mut self.errors),
        }
    }

    fn find_cli_stmt<'a>(
        &mut self,
        program: &'a Program,
    ) -> Option<(&'a Stmt, &'a IncipitStmt, radix_lexer::Symbol)> {
        let mut found = None;
        for stmt in &program.statements {
            for annotation in &stmt.annotations {
                let cli_name = match &annotation.kind {
                    AnnotationKind::Cli(cli) => Some(cli.name),
                    AnnotationKind::Braced(braced) => {
                        match braced_cli_nomen_strict(braced, self.interner) {
                            BracedView::Valid(nomen) => Some(nomen),
                            BracedView::Malformed(error) => {
                                self.malformed_annotation(error);
                                None
                            }
                            BracedView::Absent => None,
                        }
                    }
                    _ => None,
                };
                let Some(cli_name) = cli_name else {
                    continue;
                };
                let StmtKind::Incipit(entry) = &stmt.kind else {
                    self.error(annotation.span, ISSUE_CLI_ON_NON_INCIPIT);
                    continue;
                };
                if found.is_some() {
                    self.error(annotation.span, ISSUE_DUPLICATE_CLI_ENTRY);
                } else {
                    found = Some((stmt, entry, cli_name));
                }
            }
        }
        found
    }

    fn collect_commands(&mut self, program: &Program) -> Vec<CliCommand> {
        let mut commands = Vec::new();
        for stmt in &program.statements {
            let Some(command_name) = self.imperium_annotation(&stmt.annotations) else {
                if !has_cli_annotation(&stmt.annotations, self.interner) {
                    self.reject_cli_surface_without_owner(stmt);
                }
                continue;
            };

            let StmtKind::Func(func) = &stmt.kind else {
                self.error(stmt.span, ISSUE_IMPERIUM_ON_NON_FUNC);
                continue;
            };

            let mut surface = CliSurface::default();
            let mut globals = CliSurface::default();
            self.collect_surface(
                &stmt.annotations,
                SurfacePlacement::Command,
                &mut globals,
                &mut surface,
            );
            let path = self.command_path(command_name, stmt.span);
            let command = CliCommand {
                path,
                module_path: None,
                function: self.ident(&func.name),
                function_symbol: func.name.name,
                args_binding: command_argument_binding(&func.modifiers)
                    .map(|ident| self.ident(ident)),
                aliases: string_metadata_all(&stmt.annotations, self.interner, "alias"),
                description: string_metadata(&stmt.annotations, self.interner, "descriptio"),
                options: surface.options,
                operands: surface.operands,
                span: stmt.span,
            };
            self.validate_command_signature(func, stmt.span);
            if !globals.options.is_empty() || !globals.operands.is_empty() {
                self.error(stmt.span, ISSUE_COMMAND_GLOBAL_SURFACE);
            }
            commands.push(command);
        }
        commands
    }

    fn reject_cli_surface_without_owner(&mut self, stmt: &Stmt) {
        for annotation in &stmt.annotations {
            match &annotation.kind {
                AnnotationKind::Optio(_) | AnnotationKind::Operandus(_) => {
                    self.error(annotation.span, ISSUE_ORPHAN_CLI_SURFACE);
                }
                AnnotationKind::Braced(braced) => {
                    match braced_optio_view_strict(braced, self.interner) {
                        BracedView::Valid(_) => {
                            self.error(annotation.span, ISSUE_ORPHAN_CLI_SURFACE);
                        }
                        BracedView::Malformed(error) => self.malformed_annotation(error),
                        BracedView::Absent => {
                            match braced_operandus_view_strict(braced, self.interner) {
                                BracedView::Valid(_) => {
                                    self.error(annotation.span, ISSUE_ORPHAN_CLI_SURFACE);
                                }
                                BracedView::Malformed(error) => self.malformed_annotation(error),
                                BracedView::Absent => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn reject_nested_module_mounts(&mut self, program: &Program) {
        for stmt in &program.statements {
            for annotation in &stmt.annotations {
                if let AnnotationKind::Statement(annotation_stmt) = &annotation.kind
                    && self.interner.resolve(annotation_stmt.name.name) == "imperia"
                {
                    self.error(annotation.span, ISSUE_NESTED_MODULE_MOUNT);
                }
            }
        }
    }

    fn collect_surface(
        &mut self,
        annotations: &[Annotation],
        placement: SurfacePlacement,
        globals: &mut CliSurface,
        local: &mut CliSurface,
    ) {
        for annotation in annotations {
            self.collect_surface_annotation(annotation, placement, globals, local);
        }
    }

    fn collect_surface_annotation(
        &mut self,
        annotation: &Annotation,
        placement: SurfacePlacement,
        globals: &mut CliSurface,
        local: &mut CliSurface,
    ) {
        match &annotation.kind {
            AnnotationKind::Optio(optio) => {
                let option = self.option(optio, annotation.span);
                self.push_option(option, placement, annotation.span, globals, local);
            }
            AnnotationKind::Operandus(operandus) => {
                let operand = self.operand(operandus, annotation.span);
                self.push_operand(operand, placement, annotation.span, globals, local);
            }
            AnnotationKind::Braced(braced) => {
                self.collect_braced_surface(braced, annotation.span, placement, globals, local);
            }
            _ => {}
        }
    }

    fn collect_braced_surface(
        &mut self,
        braced: &radix_syntax::BracedAnnotation,
        span: Span,
        placement: SurfacePlacement,
        globals: &mut CliSurface,
        local: &mut CliSurface,
    ) {
        match braced_optio_view_strict(braced, self.interner) {
            BracedView::Valid(view) => {
                let option = self.option_from_braced(&view, span);
                self.push_option(option, placement, span, globals, local);
            }
            BracedView::Malformed(error) => self.malformed_annotation(error),
            BracedView::Absent => {
                self.collect_braced_operandus_surface(braced, span, placement, globals, local);
            }
        }
    }

    fn collect_braced_operandus_surface(
        &mut self,
        braced: &radix_syntax::BracedAnnotation,
        span: Span,
        placement: SurfacePlacement,
        globals: &mut CliSurface,
        local: &mut CliSurface,
    ) {
        match braced_operandus_view_strict(braced, self.interner) {
            BracedView::Valid(view) => {
                let operand = self.operand_from_braced(&view, span);
                self.push_operand(operand, placement, span, globals, local);
            }
            BracedView::Malformed(error) => self.malformed_annotation(error),
            BracedView::Absent => {}
        }
    }

    fn push_option(
        &mut self,
        option: CliOption,
        placement: SurfacePlacement,
        span: Span,
        globals: &mut CliSurface,
        local: &mut CliSurface,
    ) {
        match (option.global, placement) {
            (true, SurfacePlacement::TopLevel) => globals.options.push(option),
            (true, SurfacePlacement::Command) => {
                self.error(span, ISSUE_GLOBAL_OPTION_PLACEMENT);
                globals.options.push(option);
            }
            (false, SurfacePlacement::TopLevel | SurfacePlacement::Command) => {
                local.options.push(option);
            }
        }
    }

    fn push_operand(
        &mut self,
        operand: CliOperand,
        placement: SurfacePlacement,
        span: Span,
        globals: &mut CliSurface,
        local: &mut CliSurface,
    ) {
        match (operand.global, placement) {
            (true, SurfacePlacement::TopLevel) => globals.operands.push(operand),
            (true, SurfacePlacement::Command) => {
                self.error(span, ISSUE_GLOBAL_OPERAND_PLACEMENT);
                globals.operands.push(operand);
            }
            (false, SurfacePlacement::TopLevel | SurfacePlacement::Command) => {
                local.operands.push(operand);
            }
        }
    }

    fn option_from_braced(
        &mut self,
        view: &radix_syntax::BracedOptioView<'_>,
        span: Span,
    ) -> CliOption {
        let ty = self.optional_cli_type(view.ty, span);
        let default = self.option_default(view.default, &ty);
        CliOption {
            binding: self.ident(view.binding),
            binding_symbol: view.binding.name,
            flag: ty == CliType::Bivalens,
            ty,
            short: view.short.map(|sym| self.resolve(sym)),
            long: view.long.map(|sym| self.resolve(sym)),
            description: view.description.map(|sym| self.resolve(sym)),
            global: view.global,
            default,
            span,
        }
    }

    fn operand_from_braced(
        &mut self,
        view: &radix_syntax::BracedOperandusView<'_>,
        span: Span,
    ) -> CliOperand {
        CliOperand {
            binding: self.ident(view.binding),
            binding_symbol: view.binding.name,
            ty: self.cli_type(view.ty, span),
            rest: view.is_rest,
            description: view.description.map(|sym| self.resolve(sym)),
            global: view.global,
            default: view.default.map(|expr| self.default(expr)),
            span,
        }
    }

    fn option(&mut self, optio: &OptioAnnotation, span: Span) -> CliOption {
        let ty = self.optional_cli_type(optio.ty.as_ref(), span);
        let default = self.option_default(optio.default.as_deref(), &ty);
        CliOption {
            binding: self.ident(&optio.binding),
            binding_symbol: optio.binding.name,
            flag: ty == CliType::Bivalens,
            ty,
            short: optio.short.map(|sym| self.resolve(sym)),
            long: optio.long.map(|sym| self.resolve(sym)),
            description: optio.description.map(|sym| self.resolve(sym)),
            global: optio.global,
            default,
            span,
        }
    }

    fn operand(&mut self, operandus: &OperandusAnnotation, span: Span) -> CliOperand {
        CliOperand {
            binding: self.ident(&operandus.binding),
            binding_symbol: operandus.binding.name,
            ty: self.cli_type(&operandus.ty, span),
            rest: operandus.is_rest,
            description: operandus.description.map(|sym| self.resolve(sym)),
            global: operandus.global,
            default: operandus.default.as_deref().map(|expr| self.default(expr)),
            span,
        }
    }

    fn optional_cli_type(&mut self, ty: Option<&TypeExpr>, span: Span) -> CliType {
        ty.map_or(CliType::Textus, |ty| self.cli_type(ty, span))
    }

    fn option_default(&self, explicit: Option<&Expr>, ty: &CliType) -> Option<CliDefault> {
        explicit
            .map(|expr| self.default(expr))
            .or_else(|| Self::implicit_option_default(ty))
    }

    fn implicit_option_default(ty: &CliType) -> Option<CliDefault> {
        if *ty == CliType::Bivalens {
            Some(CliDefault::Bool(false))
        } else {
            None
        }
    }

    fn cli_type(&mut self, ty: &TypeExpr, span: Span) -> CliType {
        if ty.mode.is_some() {
            self.error(span, ISSUE_UNSUPPORTED_TYPE_SHAPE);
            return CliType::Textus;
        }
        match &ty.kind {
            TypeExprKind::Named(name, params) if params.is_empty() => {
                self.named_cli_type(name.name, span)
            }
            TypeExprKind::Named(name, params)
                if self.interner.resolve(name.name) == "lista" && params.len() == 1 =>
            {
                // A bare named element (`lista<numerus>`) parses as
                // `GenericArgExpr::Name`; both element shapes lower the same.
                let element = match &params[0] {
                    GenericArgExpr::Type(element) => element.clone(),
                    GenericArgExpr::Name(element) => TypeExpr {
                        nullable: false,
                        mode: None,
                        kind: TypeExprKind::Named(element.clone(), Vec::new()),
                        span: element.span,
                        form: radix_syntax::TypeFormLevel::LONG,
                        iuncta_label: None,
                    },
                    GenericArgExpr::Size(_) | GenericArgExpr::Range(_) => {
                        self.error(span, ISSUE_UNSUPPORTED_TYPE_SHAPE);
                        return CliType::Textus;
                    }
                };
                self.list_cli_type(&element, span)
            }
            _ => {
                self.error(span, ISSUE_UNSUPPORTED_TYPE_SHAPE);
                CliType::Textus
            }
        }
    }

    fn named_cli_type(&mut self, name: radix_lexer::Symbol, span: Span) -> CliType {
        match self.interner.resolve(name) {
            "textus" => CliType::Textus,
            "numerus" => CliType::Numerus,
            "fractus" => CliType::Fractus,
            "bivalens" => CliType::Bivalens,
            "ignotum" => CliType::Ignotum,
            other => {
                self.error_with(span, ISSUE_UNSUPPORTED_TYPE, [("type", other.to_owned())]);
                CliType::Textus
            }
        }
    }

    fn list_cli_type(&mut self, element: &TypeExpr, span: Span) -> CliType {
        match &element.kind {
            TypeExprKind::Named(inner, inner_params) if inner_params.is_empty() => {
                match self.interner.resolve(inner.name) {
                    "textus" => CliType::ListaTextus,
                    "numerus" => CliType::ListaNumerus,
                    // `lista<b8>` is the raw byte operand (the retired
                    // `octeti`/`bytes` word named the same type).
                    "b8" => CliType::Octeti,
                    other => {
                        self.error_with(
                            span,
                            ISSUE_UNSUPPORTED_LIST_ELEMENT_TYPE,
                            [("type", other.to_owned())],
                        );
                        CliType::ListaTextus
                    }
                }
            }
            _ => {
                self.error(span, ISSUE_LIST_ELEMENT_NOT_NAMED);
                CliType::ListaTextus
            }
        }
    }

    fn default(&self, expr: &Expr) -> CliDefault {
        match &expr.kind {
            ExprKind::Literal(Literal::String(sym)) => CliDefault::Text(self.resolve(*sym)),
            ExprKind::Literal(Literal::Int(value)) => match i64::try_from(*value) {
                Ok(value) => CliDefault::Integer(value),
                // CLI payloads stay signed; an out-of-range u64 magnitude falls
                // back to the unsupported-expression channel instead of wrapping.
                Err(_) => CliDefault::Expr(format!("{:?}", expr.kind)),
            },
            ExprKind::Literal(Literal::Float(value) | Literal::FloatText { value, .. }) => {
                CliDefault::Float(*value)
            }
            ExprKind::Literal(Literal::Bool(value)) => CliDefault::Bool(*value),
            ExprKind::Literal(Literal::Nil) => CliDefault::Nil,
            ExprKind::KeywordLiteral(keyword) => match &keyword.literal {
                Literal::Bool(value) => CliDefault::Bool(*value),
                Literal::Nil => CliDefault::Nil,
                Literal::Float(value) => CliDefault::Float(*value),
                other => CliDefault::Expr(format!("{other:?}")),
            },
            other => CliDefault::Expr(format!("{other:?}")),
        }
    }

    fn exit(&self, expr: &Expr) -> CliExit {
        match &expr.kind {
            ExprKind::Literal(Literal::Int(value)) => match i64::try_from(*value) {
                Ok(value) => CliExit::Fixed(value),
                Err(_) => CliExit::Unsupported,
            },
            ExprKind::Ident(ident) => CliExit::Binding(self.ident(ident)),
            ExprKind::Member(member) => self.member_exit(member),
            ExprKind::Paren(inner) => self.exit(inner),
            _ => CliExit::Unsupported,
        }
    }

    fn member_exit(&self, member: &MemberExpr) -> CliExit {
        let ExprKind::Ident(object) = &member.object.kind else {
            return CliExit::Unsupported;
        };
        CliExit::Field {
            object: self.ident(object),
            field: self.ident(&member.member),
        }
    }

    fn validate_surface(&mut self, label: &str, options: &[CliOption], operands: &[CliOperand]) {
        let mut bindings = FxHashMap::default();
        let mut shorts = FxHashMap::default();
        let mut longs = FxHashMap::default();

        for option in options {
            self.validate_option_surface(label, option, &mut bindings, &mut shorts, &mut longs);
        }

        let mut seen_rest = false;
        for (index, operand) in operands.iter().enumerate() {
            self.validate_operand_surface(
                label,
                operand,
                index,
                operands.len(),
                &mut bindings,
                &mut seen_rest,
            );
        }
    }

    fn validate_option_surface(
        &mut self,
        label: &str,
        option: &CliOption,
        bindings: &mut FxHashMap<String, Span>,
        shorts: &mut FxHashMap<String, Span>,
        longs: &mut FxHashMap<String, Span>,
    ) {
        self.validate_binding(label, &option.binding, option.span, bindings);
        if option.short.is_none() && option.long.is_none() {
            self.error_with(
                option.span,
                ISSUE_MISSING_OPTION_FLAG,
                [
                    ("label", label.to_owned()),
                    ("binding", option.binding.clone()),
                ],
            );
        }
        self.validate_option_short(label, option, shorts);
        self.validate_option_long(label, option, longs);
    }

    fn validate_option_short(
        &mut self,
        label: &str,
        option: &CliOption,
        shorts: &mut FxHashMap<String, Span>,
    ) {
        let Some(short) = &option.short else {
            return;
        };
        if short.chars().count() != 1 || short.starts_with('-') {
            self.error_with(
                option.span,
                ISSUE_INVALID_SHORT_FLAG,
                [
                    ("short", short.clone()),
                    ("binding", option.binding.clone()),
                ],
            );
        }
        if shorts.insert(short.clone(), option.span).is_some() {
            self.error_with(
                option.span,
                ISSUE_DUPLICATE_SHORT_FLAG,
                [("short", short.clone()), ("label", label.to_owned())],
            );
        }
    }

    fn validate_option_long(
        &mut self,
        label: &str,
        option: &CliOption,
        longs: &mut FxHashMap<String, Span>,
    ) {
        let Some(long) = &option.long else {
            return;
        };
        if long.is_empty() || long.starts_with('-') || long.chars().any(char::is_whitespace) {
            self.error_with(
                option.span,
                ISSUE_INVALID_LONG_FLAG,
                [("long", long.clone()), ("binding", option.binding.clone())],
            );
        }
        if longs.insert(long.clone(), option.span).is_some() {
            self.error_with(
                option.span,
                ISSUE_DUPLICATE_LONG_FLAG,
                [("long", long.clone()), ("label", label.to_owned())],
            );
        }
    }

    fn validate_operand_surface(
        &mut self,
        label: &str,
        operand: &CliOperand,
        index: usize,
        operand_count: usize,
        bindings: &mut FxHashMap<String, Span>,
        seen_rest: &mut bool,
    ) {
        self.validate_binding(label, &operand.binding, operand.span, bindings);
        if *seen_rest {
            self.error_with(
                operand.span,
                ISSUE_OPERAND_AFTER_REST,
                [("binding", operand.binding.clone())],
            );
        }
        if operand.rest {
            if index + 1 != operand_count {
                self.error(operand.span, ISSUE_REST_NOT_FINAL);
            }
            if *seen_rest {
                self.error(operand.span, ISSUE_DUPLICATE_REST_OPERAND);
            }
            *seen_rest = true;
        }
    }

    fn validate_binding(
        &mut self,
        label: &str,
        binding: &str,
        span: Span,
        bindings: &mut FxHashMap<String, Span>,
    ) {
        if bindings.insert(binding.to_owned(), span).is_some() {
            self.error_with(
                span,
                ISSUE_DUPLICATE_BINDING,
                [("binding", binding.to_owned()), ("label", label.to_owned())],
            );
        }
    }

    fn validate_commands(&mut self, commands: &[CliCommand], globals: &CliSurface) {
        let paths = self.command_paths(commands);
        let mut aliases = FxHashSet::default();
        // BO-D-9: build the global binding set once; reuse per command.
        let global_bindings = Self::global_bindings(globals);
        for command in commands {
            self.validate_command(command, &global_bindings, &paths, &mut aliases);
        }
    }

    fn command_paths(&mut self, commands: &[CliCommand]) -> FxHashSet<String> {
        let mut paths = FxHashSet::default();
        for command in commands {
            let path = command.path.join("/");
            if !paths.insert(path.clone()) {
                self.error_with(command.span, ISSUE_DUPLICATE_COMMAND_PATH, [("path", path)]);
            }
        }
        paths
    }

    fn validate_command(
        &mut self,
        command: &CliCommand,
        global_bindings: &FxHashSet<&str>,
        paths: &FxHashSet<String>,
        aliases: &mut FxHashSet<String>,
    ) {
        self.validate_command_aliases(command, paths, aliases);
        let label = command_label(command);
        self.validate_surface(&label, &command.options, &command.operands);
        self.validate_global_option_collisions(&label, &command.options, global_bindings);
        self.validate_global_operand_collisions(&label, &command.operands, global_bindings);
    }

    fn validate_command_aliases(
        &mut self,
        command: &CliCommand,
        paths: &FxHashSet<String>,
        aliases: &mut FxHashSet<String>,
    ) {
        for alias in &command.aliases {
            self.validate_command_alias(command, alias, paths, aliases);
        }
    }

    fn validate_command_alias(
        &mut self,
        command: &CliCommand,
        alias: &str,
        paths: &FxHashSet<String>,
        aliases: &mut FxHashSet<String>,
    ) {
        if path_segments(alias).is_empty() || alias.contains('/') {
            self.error_with(
                command.span,
                ISSUE_INVALID_COMMAND_ALIAS,
                [("alias", alias.to_owned())],
            );
        }
        if !aliases.insert(alias.to_owned()) {
            self.error_with(
                command.span,
                ISSUE_DUPLICATE_COMMAND_ALIAS,
                [("alias", alias.to_owned())],
            );
        }
        if paths.contains(alias) {
            self.error_with(
                command.span,
                ISSUE_ALIAS_COLLIDES_WITH_PATH,
                [("alias", alias.to_owned())],
            );
        }
    }

    fn validate_global_surface_collisions(
        &mut self,
        label: &str,
        local_options: &[CliOption],
        local_operands: &[CliOperand],
        globals: &CliSurface,
    ) {
        let global_bindings = Self::global_bindings(globals);
        self.validate_global_option_collisions(label, local_options, &global_bindings);
        self.validate_global_operand_collisions(label, local_operands, &global_bindings);
    }

    fn global_bindings(globals: &CliSurface) -> FxHashSet<&str> {
        globals
            .options
            .iter()
            .map(|option| option.binding.as_str())
            .chain(
                globals
                    .operands
                    .iter()
                    .map(|operand| operand.binding.as_str()),
            )
            .collect()
    }

    fn validate_global_option_collisions(
        &mut self,
        label: &str,
        options: &[CliOption],
        global_bindings: &FxHashSet<&str>,
    ) {
        for option in options {
            if global_bindings.contains(option.binding.as_str()) {
                self.error_with(
                    option.span,
                    ISSUE_LOCAL_OPTION_GLOBAL_COLLISION,
                    [
                        ("label", label.to_owned()),
                        ("binding", option.binding.clone()),
                    ],
                );
            }
        }
    }

    fn validate_global_operand_collisions(
        &mut self,
        label: &str,
        operands: &[CliOperand],
        global_bindings: &FxHashSet<&str>,
    ) {
        for operand in operands {
            if global_bindings.contains(operand.binding.as_str()) {
                self.error_with(
                    operand.span,
                    ISSUE_LOCAL_OPERAND_GLOBAL_COLLISION,
                    [
                        ("label", label.to_owned()),
                        ("binding", operand.binding.clone()),
                    ],
                );
            }
        }
    }

    fn validate_command_signature(&mut self, func: &FuncDecl, span: Span) {
        if !func.params.is_empty() {
            self.error(span, ISSUE_COMMAND_PARAMS_UNSUPPORTED);
        }
    }

    fn imperium_annotation(&mut self, annotations: &[Annotation]) -> Option<radix_lexer::Symbol> {
        annotations
            .iter()
            .find_map(|annotation| match &annotation.kind {
                AnnotationKind::Imperium(imperium) => Some(imperium.name),
                AnnotationKind::Braced(braced) => {
                    match braced_imperium_nomen_strict(braced, self.interner) {
                        BracedView::Valid(nomen) => Some(nomen),
                        BracedView::Malformed(error) => {
                            self.malformed_annotation(error);
                            None
                        }
                        BracedView::Absent => None,
                    }
                }
                _ => None,
            })
    }

    fn command_path(&mut self, name: radix_lexer::Symbol, span: Span) -> Vec<String> {
        let raw = self.resolve(name);
        let parts = path_segments(&raw);
        if parts.is_empty() || raw.starts_with('/') || raw.ends_with('/') || raw.contains("//") {
            self.error_with(span, ISSUE_INVALID_COMMAND_PATH, [("path", raw)]);
        }
        parts
    }

    fn ident(&self, ident: &Ident) -> String {
        self.resolve(ident.name)
    }

    fn resolve(&self, symbol: radix_lexer::Symbol) -> String {
        self.interner.resolve(symbol).to_owned()
    }

    fn error(&mut self, span: Span, issue: &'static str) {
        self.error_with(span, issue, []);
    }

    fn error_with<const N: usize>(
        &mut self,
        span: Span,
        issue: &'static str,
        args: [(&'static str, String); N],
    ) {
        let mut err = SemanticError::structured(SemanticErrorKind::CliValidation, span)
            .with_arg("issue", issue);
        for (name, value) in args {
            err = err.with_arg(name, value);
        }
        self.errors.push(err);
    }

    fn malformed_annotation(&mut self, error: BracedAnnotationError) {
        self.error_with(
            error.span,
            "malformed_annotation",
            [
                ("annotation", error.annotation.to_owned()),
                ("field", error.field.to_owned()),
                ("expected", error.expected.to_owned()),
            ],
        );
    }
}

fn prefix_mounted_commands(commands: &mut [CliCommand], mount_prefix: &[String]) {
    for command in commands {
        let local_path = std::mem::take(&mut command.path);
        command.path = mount_prefix.iter().cloned().chain(local_path).collect();
        command.aliases = command
            .aliases
            .iter()
            .map(|alias| prefix_mounted_alias(mount_prefix, alias))
            .collect();
    }
}

fn prefix_mounted_alias(mount_prefix: &[String], alias: &str) -> String {
    mount_prefix
        .iter()
        .cloned()
        .chain(path_segments(alias))
        .collect::<Vec<_>>()
        .join("/")
}

fn command_label(command: &CliCommand) -> String {
    format!("command '{}'", command.path.join("/"))
}

fn path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SurfacePlacement {
    TopLevel,
    Command,
}

#[derive(Default)]
struct CliSurface {
    options: Vec<CliOption>,
    operands: Vec<CliOperand>,
}

fn has_cli_annotation(annotations: &[Annotation], interner: &Interner) -> bool {
    annotations.iter().any(|annotation| match &annotation.kind {
        AnnotationKind::Cli(_) => true,
        AnnotationKind::Braced(braced) => braced_cli_nomen(braced, interner).is_some(),
        _ => false,
    })
}

fn command_argument_binding(modifiers: &[radix_syntax::FuncModifier]) -> Option<&Ident> {
    modifiers.iter().find_map(|modifier| match modifier {
        radix_syntax::FuncModifier::Argumenta(ident) => Some(ident),
        _ => None,
    })
}

fn incipit_body_is_empty(body: &radix_syntax::IfBody) -> bool {
    matches!(body, radix_syntax::IfBody::Block(block) if block.statements.is_empty())
}

fn string_metadata(annotations: &[Annotation], interner: &Interner, name: &str) -> Option<String> {
    string_metadata_all(annotations, interner, name)
        .into_iter()
        .next()
}

fn string_metadata_all(annotations: &[Annotation], interner: &Interner, name: &str) -> Vec<String> {
    annotations
        .iter()
        .filter_map(|annotation| match &annotation.kind {
            AnnotationKind::Statement(stmt)
                if interner.resolve(stmt.name.name) == name && stmt.args.len() == 1 =>
            {
                match &stmt.args[0].kind {
                    radix_lexer::TokenKind::String(sym) => Some(interner.resolve(*sym).to_owned()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
#[path = "cli_test.rs"]
mod cli_test;
