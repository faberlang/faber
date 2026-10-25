//! Identity-resolved shader-contract ingestion (U2 of the WGSL shader-contract
//! boundary).
//!
//! This module replaces the literal, path-insensitive Triga matchers
//! (`geometry.geometry_vertex_layout_matches` member-spelling checks plus
//! recursive syntax scanning) with a compiler-defined generic contract surface
//! recognized by **resolved identity** — never by alias, member spelling, or
//! package name.
//!
//! ## Surface (decision D1)
//!
//! A library (or shader program) declares a contract-role function with the
//! compiler-owned annotation:
//!
//! ```fab
//! @ shader_contract "vertex_layout"
//! functio vertex_layout_matches(
//!     geo: numerus, index: numerus, source_name: textus,
//!     location: numerus, format_code: numerus, offset_bytes: numerus,
//!     stride_bytes: numerus, step_mode_code: numerus,
//! ) → bivalens { redde verum }
//! ```
//!
//! The annotation is the generic surface and the role string is the contract
//! identity. The driver registers each annotated function into a
//! `DefId → role` table (the `gpu_builtins` precedent) and recognizes contract
//! calls only when the call's callee resolves to one of those `DefId`s. The
//! recognized role functions and their call argument shapes:
//!
//! | Role | Call argument shape |
//! | --- | --- |
//! | `vertex_layout` | `(geo, index, source_name, location, format_code, offset_bytes, stride_bytes, step_mode_code, stage_io_role_code)` |
//! | `varying` | `(source_name, location, format_code, interpolation_code, stage_io_role_code)` |
//! | `fragment_output` | `(location, format_code, stage_io_role_code)` |
//! | `pipeline` | `(color_target_format_code, depth_compare_code, depth_write_enabled_code, primitive_topology_code, vertex_count)` |
//! | `resource_binding` | `(group, binding, kind_code, role_code, access_code, element_count, layout_code, source_name, stage_io_role_code)` |
//! | `bound_resource_binding` | `(group, binding, source_name)` |
//!
//! Numeric codes are the canonical code tables in
//! `radix_mir::shader_contract`; unknown codes fail closed with
//! [`radix_mir::MirAbiError`]. `pipeline` uses `depth_compare_code == 0` for
//! "no depth attachment" (matching the legacy optional-depth shape). The
//! `stage_io_role_code` (`radix_mir::shader_contract::stage_io_role_from_code`)
//! is the explicit stage-I/O role declaration on each call: roles come from
//! the declared contract, never from source-name spelling or location.
//!
//! ## Fail-closed ingestion
//!
//! - Unresolvable role identities (`@ shader_contract "<unknown>"`) are
//!   structured diagnostics (`shader_contract_role_unknown`).
//! - Rejected/unsupported numeric codes are structured diagnostics
//!   (`shader_contract_code_unsupported`).
//! - Fact arguments must be compile-time constants; non-constant arguments are
//!   structured diagnostics (`shader_contract_arg_non_constant`).
//! - Malformed contract calls (wrong argument count) are structured
//!   diagnostics (`shader_contract_arg_count`).
//! - Duplicate/conflicting facts are rejected, not merged
//!   (`shader_contract_fact_conflict`).
//! - No facts are invented: the hard-coded fragment-output, pipeline, and
//!   resource defaults are gone from radix. Facts trace to resolved contract
//!   calls only; the Goal-01 defaults move to Triga's adapter (decision D2).
//!
//! The explicit empty-body default path (an `@ vertex`/`@ fragment` entry with
//! an empty body) remains a source-selected generic mechanism; it declares no
//! facts on its own.

use handroll::{FxHashMap, FxHashSet};

use crate::diagnostics::{Diagnostic, DiagnosticArg, DiagnosticPhase, Span};
use crate::hir::visit::{HirVisitor, walk_expr};
use crate::hir::{DefId, HirCallArg, HirExpression, HirExpressionKind, HirItemKind, HirLiteral, HirModule};
use crate::lexer::{Interner, TokenKind};
use crate::semantic::{Primitive, TypeTable};

use radix_mir::{
    ContractProvenance, MirAbiError, MirDepthStencilState, MirScalarLayout, MirStencilFaceState, MirType,
    ShaderBoundResourceBinding, ShaderFragmentOutput, ShaderPipelineState, ShaderResourceBinding, ShaderVarying,
    ShaderVertexAttribute, color_target_format_from_code, depth_compare_function_from_code, input_step_mode_from_code,
    primitive_topology_from_code, resource_access_from_code, resource_kind_from_code, resource_role_from_code,
    stage_io_role_from_code, varying_interpolation_from_code, vertex_format_from_code,
};

/// Compiler-owned annotation family for the generic contract surface.
pub(crate) const SHADER_CONTRACT_ANNOTATION: &str = "shader_contract";

const CODE_ROLE_UNKNOWN: &str = "SHADERCONTRACT001";
const CODE_CODE_UNSUPPORTED: &str = "SHADERCONTRACT002";
const CODE_ARG_NON_CONSTANT: &str = "SHADERCONTRACT003";
const CODE_FACT_CONFLICT: &str = "SHADERCONTRACT004";
const CODE_ARG_COUNT: &str = "SHADERCONTRACT005";

/// A recognized role on the compiler-defined generic contract surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShaderContractRole {
    VertexLayout,
    Varying,
    FragmentOutput,
    Pipeline,
    ResourceBinding,
    BoundResourceBinding,
}

impl ShaderContractRole {
    fn from_spelling(spelling: &str) -> Option<Self> {
        match spelling {
            "vertex_layout" => Some(Self::VertexLayout),
            "varying" => Some(Self::Varying),
            "fragment_output" => Some(Self::FragmentOutput),
            "pipeline" => Some(Self::Pipeline),
            "resource_binding" => Some(Self::ResourceBinding),
            "bound_resource_binding" => Some(Self::BoundResourceBinding),
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn spelling(self) -> &'static str {
        match self {
            Self::VertexLayout => "vertex_layout",
            Self::Varying => "varying",
            Self::FragmentOutput => "fragment_output",
            Self::Pipeline => "pipeline",
            Self::ResourceBinding => "resource_binding",
            Self::BoundResourceBinding => "bound_resource_binding",
        }
    }
}

/// Contract facts ingested from resolved contract-role calls.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShaderContractFacts {
    pub vertex_attributes: Vec<ShaderVertexAttribute>,
    pub varyings: Vec<ShaderVarying>,
    pub fragment_outputs: Vec<ShaderFragmentOutput>,
    pub pipeline_states: Vec<ShaderPipelineState>,
    pub resource_bindings: Vec<ShaderResourceBinding>,
    pub bound_resource_bindings: Vec<ShaderBoundResourceBinding>,
}

/// Collect shader-contract facts from the analyzed unit.
///
/// The contract surface is registered from `@ shader_contract "<role>"`
/// annotations on functions declared in the unit; contract calls are
/// recognized by the resolved callee `DefId`, never by spelling. All
/// ingestion failures are structured diagnostics.
pub(crate) fn collect_shader_contract_facts(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> ShaderContractFacts {
    let mut surface: FxHashMap<DefId, ShaderContractRole> = FxHashMap::default();
    register_contract_surface(hir, interner, file, diagnostics, &mut surface);

    let mut collector = ContractCollector {
        types,
        interner,
        surface: &surface,
        facts: ShaderContractFacts::default(),
        diagnostics,
        file,
        keys: FactKeys::default(),
    };
    let mut visitor = AdfirmaVisitor { collector: &mut collector };
    visitor.visit_program(hir);
    collector.facts
}

/// Register every `@ shader_contract "<role>"` function in the unit into the
/// recognized surface, keyed by its resolved definition identity.
fn register_contract_surface(
    hir: &HirModule,
    interner: &Interner,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
    surface: &mut FxHashMap<DefId, ShaderContractRole>,
) {
    for item in &hir.items {
        let HirItemKind::Function(_) = &item.kind else {
            continue;
        };
        for annotation in &item.annotations {
            if interner.resolve(annotation.family) != SHADER_CONTRACT_ANNOTATION {
                continue;
            }
            let Some(role) = contract_role_from_annotation(annotation, interner) else {
                diagnostics.push(role_unknown_diagnostic(annotation, interner, file));
                continue;
            };
            // A function may carry at most one contract role; a second,
            // different role on the same definition is a conflicting
            // declaration.
            if let Some(existing) = surface.get(&item.def_id) {
                if *existing != role {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "shader contract function carries conflicting roles '{}' and '{}'",
                            existing.spelling(),
                            role.spelling()
                        ))
                        .with_phase(DiagnosticPhase::Analysis)
                        .with_code(CODE_FACT_CONFLICT)
                        .with_args(vec![
                            DiagnosticArg::new("issue", "shader_contract_fact_conflict"),
                            DiagnosticArg::new("role", role.spelling()),
                            DiagnosticArg::new("key", "role-declaration"),
                        ])
                        .with_span(annotation.span)
                        .with_file(file.to_owned()),
                    );
                }
                continue;
            }
            surface.insert(item.def_id, role);
        }
    }
}

/// Read the role string from `@ shader_contract "<role>"`. The surface accepts
/// a single string argument carrying the contract identity.
fn contract_role_from_annotation(
    annotation: &crate::hir::HirAnnotation,
    interner: &Interner,
) -> Option<ShaderContractRole> {
    if annotation.raw_args.len() != 1 {
        return None;
    }
    let TokenKind::String(symbol) = annotation.raw_args[0].kind else {
        return None;
    };
    let spelling = interner.resolve(symbol);
    ShaderContractRole::from_spelling(spelling)
}

fn role_unknown_diagnostic(annotation: &crate::hir::HirAnnotation, interner: &Interner, file: &str) -> Diagnostic {
    let role = annotation
        .raw_args
        .first()
        .and_then(|token| match token.kind {
            TokenKind::String(symbol) => Some(interner.resolve(symbol).to_owned()),
            _ => None,
        })
        .unwrap_or_else(|| "<missing>".to_owned());
    Diagnostic::error(format!("shader contract annotation declares unknown role '{role}'"))
        .with_phase(DiagnosticPhase::Analysis)
        .with_code(CODE_ROLE_UNKNOWN)
        .with_args(vec![
            DiagnosticArg::new("issue", "shader_contract_role_unknown"),
            DiagnosticArg::new("role", role),
            DiagnosticArg::new("annotation", SHADER_CONTRACT_ANNOTATION),
        ])
        .with_span(annotation.span)
        .with_file(file.to_owned())
}

/// Walk the HIR and hand every `adfirma` assertion condition to the collector.
struct AdfirmaVisitor<'a, 'b> {
    collector: &'a mut ContractCollector<'b>,
}

impl HirVisitor for AdfirmaVisitor<'_, '_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Adfirma(cond, _) = &expr.kind {
            self.collector.ingest_assertion(cond);
        }
        walk_expr(self, expr);
    }
}

/// Fact identity keys already declared, for duplicate/conflict rejection.
#[derive(Debug, Clone, Default)]
struct FactKeys {
    vertex_locations: FxHashSet<u32>,
    varying_locations: FxHashSet<u32>,
    fragment_output_locations: FxHashSet<u32>,
    /// `(group, binding)` slots claimed by `resource_binding` and
    /// `bound_resource_binding` calls alike: one slot, one declaration.
    resource_bindings: FxHashSet<(u32, u32)>,
    pipeline_declared: bool,
}

struct ContractCollector<'a> {
    types: &'a TypeTable,
    interner: &'a Interner,
    surface: &'a FxHashMap<DefId, ShaderContractRole>,
    facts: ShaderContractFacts,
    diagnostics: &'a mut Vec<Diagnostic>,
    file: &'a str,
    keys: FactKeys,
}

impl ContractCollector<'_> {
    /// Inspect one assertion condition. Only calls whose callee resolves to a
    /// registered contract-role function are contract facts; every other
    /// assertion is an ordinary program assertion and produces no facts.
    fn ingest_assertion(&mut self, cond: &HirExpression) {
        let HirExpressionKind::Call(callee, _, args, _) = &cond.kind else {
            return;
        };
        let HirExpressionKind::Path(def_id) = &callee.kind else {
            // Non-`Path` callees (member/namespace calls, indirect callees) are
            // not resolvable to a contract identity in the driver. They are
            // ordinary assertions here, not contract facts.
            return;
        };
        let Some(&role) = self.surface.get(def_id) else {
            return;
        };
        self.ingest_contract_call(role, *def_id, args, cond.span);
    }

    fn ingest_contract_call(&mut self, role: ShaderContractRole, callee: DefId, args: &[HirCallArg], span: Span) {
        match role {
            ShaderContractRole::VertexLayout => self.ingest_vertex_layout(callee, args, span),
            ShaderContractRole::Varying => self.ingest_varying(callee, args, span),
            ShaderContractRole::FragmentOutput => self.ingest_fragment_output(callee, args, span),
            ShaderContractRole::Pipeline => self.ingest_pipeline(callee, args, span),
            ShaderContractRole::ResourceBinding => self.ingest_resource_binding(callee, args, span),
            ShaderContractRole::BoundResourceBinding => self.ingest_bound_resource_binding(callee, args, span),
        }
    }

    fn provenance(&self, callee: DefId, span: Span) -> ContractProvenance {
        ContractProvenance { source: Some(callee), span }
    }

    fn int_arg(&mut self, args: &[HirCallArg], index: usize, role: ShaderContractRole, span: Span) -> Option<u64> {
        if let HirExpressionKind::Literal(HirLiteral::Int(value)) = &args[index].expr.kind {
            Some(*value)
        } else {
            self.push_non_constant_diagnostic(role, index, span);
            None
        }
    }

    fn string_arg(
        &mut self,
        args: &[HirCallArg],
        index: usize,
        role: ShaderContractRole,
        span: Span,
    ) -> Option<String> {
        if let HirExpressionKind::Literal(HirLiteral::String(symbol)) = &args[index].expr.kind {
            Some(self.interner.resolve(*symbol).to_owned())
        } else {
            self.push_non_constant_diagnostic(role, index, span);
            None
        }
    }

    /// Unwrap a fail-closed code conversion; a rejected code pushes an
    /// unsupported-code diagnostic and aborts the current fact.
    fn convert_code<T>(
        &mut self,
        conversion: impl FnOnce(u32) -> Result<T, MirAbiError>,
        code: u32,
        span: Span,
    ) -> Option<T> {
        match conversion(code) {
            Ok(value) => Some(value),
            Err(error) => {
                self.push_unsupported_code_diagnostic(error.message, span);
                None
            }
        }
    }

    fn push_non_constant_diagnostic(&mut self, role: ShaderContractRole, index: usize, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(format!(
                "shader contract '{}' fact argument {} must be a compile-time constant",
                role.spelling(),
                index
            ))
            .with_phase(DiagnosticPhase::Analysis)
            .with_code(CODE_ARG_NON_CONSTANT)
            .with_args(vec![
                DiagnosticArg::new("issue", "shader_contract_arg_non_constant"),
                DiagnosticArg::new("role", role.spelling()),
                DiagnosticArg::new("arg_index", index.to_string()),
            ])
            .with_span(span)
            .with_file(self.file.to_owned()),
        );
    }

    fn push_unsupported_code_diagnostic(&mut self, detail: impl Into<String>, span: Span) {
        let detail = detail.into();
        self.diagnostics.push(
            Diagnostic::error(format!("shader contract fact rejected: unsupported code: {detail}"))
                .with_phase(DiagnosticPhase::Analysis)
                .with_code(CODE_CODE_UNSUPPORTED)
                .with_args(vec![
                    DiagnosticArg::new("issue", "shader_contract_code_unsupported"),
                    DiagnosticArg::new("detail", detail),
                ])
                .with_span(span)
                .with_file(self.file.to_owned()),
        );
    }

    fn push_arg_count_diagnostic(&mut self, role: ShaderContractRole, expected: usize, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(format!(
                "shader contract '{}' fact call has the wrong argument count (expected {expected} arguments)",
                role.spelling()
            ))
            .with_phase(DiagnosticPhase::Analysis)
            .with_code(CODE_ARG_COUNT)
            .with_args(vec![
                DiagnosticArg::new("issue", "shader_contract_arg_count"),
                DiagnosticArg::new("role", role.spelling()),
                DiagnosticArg::new("expected", expected.to_string()),
            ])
            .with_span(span)
            .with_file(self.file.to_owned()),
        );
    }

    fn push_conflict_diagnostic(&mut self, role: ShaderContractRole, key: impl Into<String>, span: Span) {
        let key: String = key.into();
        self.diagnostics.push(
            Diagnostic::error(format!(
                "shader contract '{}' fact conflicts with an earlier declaration (key '{key}')",
                role.spelling()
            ))
            .with_phase(DiagnosticPhase::Analysis)
            .with_code(CODE_FACT_CONFLICT)
            .with_args(vec![
                DiagnosticArg::new("issue", "shader_contract_fact_conflict"),
                DiagnosticArg::new("role", role.spelling()),
                DiagnosticArg::new("key", key),
            ])
            .with_span(span)
            .with_file(self.file.to_owned()),
        );
    }

    fn ingest_vertex_layout(&mut self, callee: DefId, args: &[HirCallArg], span: Span) {
        let role = ShaderContractRole::VertexLayout;
        if args.len() != 9 {
            self.push_arg_count_diagnostic(role, 9, span);
            return;
        }
        let Some(source_name) = self.string_arg(args, 2, role, span) else {
            return;
        };
        let Some(location) = self.int_arg(args, 3, role, span) else {
            return;
        };
        let Some(format_code) = self.int_arg(args, 4, role, span) else {
            return;
        };
        let Some(offset_bytes) = self.int_arg(args, 5, role, span) else {
            return;
        };
        let Some(stride_bytes) = self.int_arg(args, 6, role, span) else {
            return;
        };
        let Some(step_mode_code) = self.int_arg(args, 7, role, span) else {
            return;
        };
        let Some(stage_io_role_code) = self.int_arg(args, 8, role, span) else {
            return;
        };
        let location = u32::try_from(location).unwrap_or(u32::MAX);
        let format_code = u32::try_from(format_code).unwrap_or(u32::MAX);
        let step_mode_code = u32::try_from(step_mode_code).unwrap_or(u32::MAX);
        let stage_io_role_code = u32::try_from(stage_io_role_code).unwrap_or(u32::MAX);
        let Some(format) = self.convert_code(vertex_format_from_code, format_code, span) else {
            return;
        };
        let Some(step_mode) = self.convert_code(input_step_mode_from_code, step_mode_code, span) else {
            return;
        };
        let Some(io_role) = self.convert_code(stage_io_role_from_code, stage_io_role_code, span) else {
            return;
        };
        if !self.keys.vertex_locations.insert(location) {
            self.push_conflict_diagnostic(role, format!("location-{location}"), span);
            return;
        }
        self.facts.vertex_attributes.push(ShaderVertexAttribute {
            semantic: Some(source_name),
            location,
            format,
            offset_bytes,
            stride_bytes,
            step_mode,
            role: io_role,
            provenance: self.provenance(callee, span),
        });
    }

    fn ingest_varying(&mut self, callee: DefId, args: &[HirCallArg], span: Span) {
        let role = ShaderContractRole::Varying;
        if args.len() != 5 {
            self.push_arg_count_diagnostic(role, 5, span);
            return;
        }
        let Some(semantic) = self.string_arg(args, 0, role, span) else {
            return;
        };
        let Some(location) = self.int_arg(args, 1, role, span) else {
            return;
        };
        let Some(format_code) = self.int_arg(args, 2, role, span) else {
            return;
        };
        let Some(interpolation_code) = self.int_arg(args, 3, role, span) else {
            return;
        };
        let Some(stage_io_role_code) = self.int_arg(args, 4, role, span) else {
            return;
        };
        let location = u32::try_from(location).unwrap_or(u32::MAX);
        let format_code = u32::try_from(format_code).unwrap_or(u32::MAX);
        let interpolation_code = u32::try_from(interpolation_code).unwrap_or(u32::MAX);
        let stage_io_role_code = u32::try_from(stage_io_role_code).unwrap_or(u32::MAX);
        let Some(format) = self.convert_code(vertex_format_from_code, format_code, span) else {
            return;
        };
        let Some(interpolation) = self.convert_code(varying_interpolation_from_code, interpolation_code, span) else {
            return;
        };
        let Some(io_role) = self.convert_code(stage_io_role_from_code, stage_io_role_code, span) else {
            return;
        };
        if !self.keys.varying_locations.insert(location) {
            self.push_conflict_diagnostic(role, format!("location-{location}"), span);
            return;
        }
        self.facts.varyings.push(ShaderVarying {
            semantic,
            location,
            format,
            interpolation,
            role: io_role,
            provenance: self.provenance(callee, span),
        });
    }

    fn ingest_fragment_output(&mut self, callee: DefId, args: &[HirCallArg], span: Span) {
        let role = ShaderContractRole::FragmentOutput;
        if args.len() != 3 {
            self.push_arg_count_diagnostic(role, 3, span);
            return;
        }
        let Some(location) = self.int_arg(args, 0, role, span) else {
            return;
        };
        let Some(format_code) = self.int_arg(args, 1, role, span) else {
            return;
        };
        let Some(stage_io_role_code) = self.int_arg(args, 2, role, span) else {
            return;
        };
        let location = u32::try_from(location).unwrap_or(u32::MAX);
        let format_code = u32::try_from(format_code).unwrap_or(u32::MAX);
        let stage_io_role_code = u32::try_from(stage_io_role_code).unwrap_or(u32::MAX);
        let Some(format) = self.convert_code(vertex_format_from_code, format_code, span) else {
            return;
        };
        let Some(io_role) = self.convert_code(stage_io_role_from_code, stage_io_role_code, span) else {
            return;
        };
        if !self.keys.fragment_output_locations.insert(location) {
            self.push_conflict_diagnostic(role, format!("location-{location}"), span);
            return;
        }
        self.facts.fragment_outputs.push(ShaderFragmentOutput {
            semantic: None,
            location,
            format,
            role: io_role,
            provenance: self.provenance(callee, span),
        });
    }

    fn ingest_pipeline(&mut self, callee: DefId, args: &[HirCallArg], span: Span) {
        let role = ShaderContractRole::Pipeline;
        if args.len() != 5 {
            self.push_arg_count_diagnostic(role, 5, span);
            return;
        }
        let Some(color_target_code) = self.int_arg(args, 0, role, span) else {
            return;
        };
        let Some(depth_compare_code) = self.int_arg(args, 1, role, span) else {
            return;
        };
        let Some(depth_write_code) = self.int_arg(args, 2, role, span) else {
            return;
        };
        let Some(topology_code) = self.int_arg(args, 3, role, span) else {
            return;
        };
        let Some(vertex_count) = self.int_arg(args, 4, role, span) else {
            return;
        };
        let color_target_code = u32::try_from(color_target_code).unwrap_or(u32::MAX);
        let topology_code = u32::try_from(topology_code).unwrap_or(u32::MAX);
        let depth_compare_code = u32::try_from(depth_compare_code).unwrap_or(u32::MAX);
        let Some(color_target) = self.convert_code(color_target_format_from_code, color_target_code, span) else {
            return;
        };
        let Some(topology) = self.convert_code(primitive_topology_from_code, topology_code, span) else {
            return;
        };
        if self.keys.pipeline_declared {
            self.push_conflict_diagnostic(role, "pipeline", span);
            return;
        }
        let depth_stencil = if depth_compare_code == 0 {
            None
        } else {
            let Some(depth_compare) = self.convert_code(depth_compare_function_from_code, depth_compare_code, span)
            else {
                return;
            };
            Some(MirDepthStencilState {
                depth_write_enabled: depth_write_code != 0,
                depth_compare,
                stencil_read_mask: u32::MAX,
                stencil_write_mask: u32::MAX,
                stencil_front: MirStencilFaceState::DISABLED,
                stencil_back: MirStencilFaceState::DISABLED,
            })
        };
        self.keys.pipeline_declared = true;
        self.facts.pipeline_states.push(ShaderPipelineState {
            topology,
            color_target_formats: vec![color_target],
            depth_stencil,
            vertex_count: u32::try_from(vertex_count).unwrap_or(u32::MAX),
            provenance: self.provenance(callee, span),
        });
    }

    fn ingest_bound_resource_binding(&mut self, callee: DefId, args: &[HirCallArg], span: Span) {
        let role = ShaderContractRole::BoundResourceBinding;
        if args.len() != 3 {
            self.push_arg_count_diagnostic(role, 3, span);
            return;
        }
        let Some(group) = self.int_arg(args, 0, role, span) else {
            return;
        };
        let Some(binding) = self.int_arg(args, 1, role, span) else {
            return;
        };
        let Some(source_name) = self.string_arg(args, 2, role, span) else {
            return;
        };
        let group = u32::try_from(group).unwrap_or(u32::MAX);
        let binding = u32::try_from(binding).unwrap_or(u32::MAX);
        if !self.keys.resource_bindings.insert((group, binding)) {
            self.push_conflict_diagnostic(role, format!("group-{group}-binding-{binding}"), span);
            return;
        }
        self.facts
            .bound_resource_bindings
            .push(ShaderBoundResourceBinding {
                group,
                binding,
                source_name,
                provenance: self.provenance(callee, span),
            });
    }

    fn ingest_resource_binding(&mut self, callee: DefId, args: &[HirCallArg], span: Span) {
        let role = ShaderContractRole::ResourceBinding;
        if args.len() != 9 {
            self.push_arg_count_diagnostic(role, 9, span);
            return;
        }
        let Some(group) = self.int_arg(args, 0, role, span) else {
            return;
        };
        let Some(binding) = self.int_arg(args, 1, role, span) else {
            return;
        };
        let Some(kind_code) = self.int_arg(args, 2, role, span) else {
            return;
        };
        let Some(role_code) = self.int_arg(args, 3, role, span) else {
            return;
        };
        let Some(access_code) = self.int_arg(args, 4, role, span) else {
            return;
        };
        let Some(element_count) = self.int_arg(args, 5, role, span) else {
            return;
        };
        let Some(layout_code) = self.int_arg(args, 6, role, span) else {
            return;
        };
        let Some(source_name) = self.string_arg(args, 7, role, span) else {
            return;
        };
        let Some(stage_io_role_code) = self.int_arg(args, 8, role, span) else {
            return;
        };
        let group = u32::try_from(group).unwrap_or(u32::MAX);
        let binding = u32::try_from(binding).unwrap_or(u32::MAX);
        let kind_code = u32::try_from(kind_code).unwrap_or(u32::MAX);
        let role_code = u32::try_from(role_code).unwrap_or(u32::MAX);
        let access_code = u32::try_from(access_code).unwrap_or(u32::MAX);
        let stage_io_role_code = u32::try_from(stage_io_role_code).unwrap_or(u32::MAX);
        let Some(kind) = self.convert_code(resource_kind_from_code, kind_code, span) else {
            return;
        };
        let Some(role_kind) = self.convert_code(resource_role_from_code, role_code, span) else {
            return;
        };
        let Some(access) = self.convert_code(resource_access_from_code, access_code, span) else {
            return;
        };
        let Some(stage_io_role) = self.convert_code(stage_io_role_from_code, stage_io_role_code, span) else {
            return;
        };
        let Some((element_layout, element_byte_width)) = scalar_layout_from_code(layout_code) else {
            // Unknown scalar layout is a rejected code; surface it as an
            // unsupported-code diagnostic.
            self.push_unsupported_code_diagnostic("unsupported shader resource layout code", span);
            return;
        };
        if !self.keys.resource_bindings.insert((group, binding)) {
            self.push_conflict_diagnostic(role, format!("group-{group}-binding-{binding}"), span);
            return;
        }
        self.facts.resource_bindings.push(ShaderResourceBinding {
            group,
            binding,
            kind,
            role: role_kind,
            access,
            element_ty: MirType::semantic(self.types.primitive(primitive_for_layout(element_layout))),
            element_layout,
            element_byte_width,
            element_count,
            source_name: Some(source_name),
            stage_io_role,
            provenance: self.provenance(callee, span),
        });
    }
}

/// Convert a canonical scalar layout to its semantic primitive family.
fn primitive_for_layout(layout: MirScalarLayout) -> Primitive {
    match layout {
        MirScalarLayout::F32 | MirScalarLayout::F64 => Primitive::Fractus,
        _ => Primitive::Numerus,
    }
}

/// Canonical scalar-layout code table for resource elements (fail-closed).
fn scalar_layout_from_code(code: u64) -> Option<(MirScalarLayout, u16)> {
    match code {
        1 => Some((MirScalarLayout::F32, 4)),
        2 => Some((MirScalarLayout::F64, 8)),
        3 => Some((MirScalarLayout::I32, 4)),
        4 => Some((MirScalarLayout::U32, 4)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "shader_contract_test.rs"]
mod tests;
