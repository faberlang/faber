//! TLA+ module emission from recognized aspect records.
//!
//! Renders `discerne omnia` exhaustiveness obligations as TLA+ modules with a
//! `Totality == Variants \\subseteq Covered` invariant. Emission is pure text
//! generation from the report; it does not invoke any external checker.

use super::{HirAspectDetail, HirAspectKind, HirAspectRecord, HirAspectRecordKind, HirAspectReport, HirAspectSpan};
use crate::hir::DefId;

#[cfg(test)]
#[path = "tla_test.rs"]
mod tests;

const DISCERNE_OMNIA_MODULE_PREFIX: &str = "FaberAspectDiscerneOmnia_";
const TLA_MODULE_HEADER_SUFFIX: &str = " ----\n";
const TLA_MODULE_FOOTER: &str = "=============================================================================\n";
const TLA_TOTALITY_INVARIANT: &str = "Totality == Variants \\subseteq Covered\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectTlaModule {
    pub name: String,
    pub text: String,
}

#[must_use]
pub fn emit_tla_modules(report: &HirAspectReport) -> Vec<HirAspectTlaModule> {
    report
        .records()
        .iter()
        .enumerate()
        .filter_map(|(index, record)| emit_record(index, record))
        .collect()
}

fn emit_record(index: usize, record: &HirAspectRecord) -> Option<HirAspectTlaModule> {
    if record.aspect != HirAspectKind::DiscerneOmnia || record.record_kind != HirAspectRecordKind::Recognized {
        return None;
    }

    let HirAspectDetail::DiscerneOmnia { variants, covered } = &record.detail else {
        return None;
    };

    let name = tla_module_name(record, index);
    let text = render_discerne_omnia_module(&name, record, variants, covered);
    Some(HirAspectTlaModule { name, text })
}

pub(crate) fn tla_module_name(record: &HirAspectRecord, index: usize) -> String {
    match record.node_id {
        Some(node_id) => format!("{DISCERNE_OMNIA_MODULE_PREFIX}Hir{}", node_id.0),
        None => format!("{DISCERNE_OMNIA_MODULE_PREFIX}Record{index}"),
    }
}

fn render_discerne_omnia_module(name: &str, record: &HirAspectRecord, variants: &[DefId], covered: &[DefId]) -> String {
    let mut out = String::new();
    out.push_str("---- MODULE ");
    out.push_str(name);
    out.push_str(TLA_MODULE_HEADER_SUFFIX);
    out.push_str("EXTENDS TLC\n\n");
    out.push_str("\\* aspect: discerne omnia\n");
    out.push_str("\\* hir_id: ");
    out.push_str(&hir_id_label(record));
    out.push('\n');
    out.push_str("\\* source_span: ");
    out.push_str(&source_span(record));
    out.push_str("\n\n");
    append_variant_constants(&mut out, variants);
    out.push('\n');
    out.push_str("Variants == ");
    out.push_str(&set_expr(variants));
    out.push('\n');
    out.push_str("Covered == ");
    out.push_str(&set_expr(covered));
    out.push_str("\n\n");
    out.push_str("VARIABLE checker_state\n");
    out.push_str("Init == checker_state = \"init\"\n");
    out.push_str("Next == UNCHANGED checker_state\n");
    out.push_str("Spec == Init /\\ [][Next]_checker_state\n\n");
    out.push_str(TLA_TOTALITY_INVARIANT);
    out.push_str(TLA_MODULE_FOOTER);
    out
}

fn hir_id_label(record: &HirAspectRecord) -> String {
    record
        .node_id
        .map_or_else(|| "none".to_owned(), |id| id.0.to_string())
}

fn append_variant_constants(out: &mut String, variants: &[DefId]) {
    for variant in variants {
        let ident = variant_ident(*variant);
        out.push_str(&ident);
        out.push_str(" == \"");
        out.push_str(&ident);
        out.push_str("\"\n");
    }
}

fn source_span(record: &HirAspectRecord) -> String {
    match record.span {
        HirAspectSpan::Source(span) => format!("{}..{}", span.start, span.end),
        HirAspectSpan::NoSpan(reason) => format!("none:{reason:?}"),
    }
}

fn set_expr(variants: &[DefId]) -> String {
    if variants.is_empty() {
        return "{}".to_owned();
    }

    let items = variants
        .iter()
        .map(|variant| variant_ident(*variant))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{items}}}")
}

fn variant_ident(variant: DefId) -> String {
    format!("Variant_{}", variant.0)
}
