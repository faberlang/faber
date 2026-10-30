use crate::test_support::analyze_source_la;
use radix_hir::visit::{self, HirVisitor};
use radix_hir::{HirStatement, HirStatementKind};

struct Sites { sites: Vec<(usize, &'static str)>, stack: Vec<String> }
impl HirVisitor for Sites {
    fn visit_expr(&mut self, e: &radix_hir::HirExpression) {
        let kind = match &e.kind {
            radix_hir::HirExpressionKind::Reduction { .. } => "Reduction",
            radix_hir::HirExpressionKind::Block(_) => "Block",
            radix_hir::HirExpressionKind::MethodCall(..) => "MethodCall",
            _ => "other",
        };
        if kind != "other" {
            if (135..=145).contains(&e.id.0) || e.id.0 == 25 {
                println!("EXPRALL@{} kind={}", e.id.0, kind);
            }
            self.stack.push(kind.to_string());
        }
        visit::walk_expr(self, e);
        self.stack.pop();
    }
    fn visit_stmt(&mut self, s: &HirStatement) {
        let c = match &s.kind {
            HirStatementKind::Local(_) => "local",
            HirStatementKind::Expr(_) => "expr",
            HirStatementKind::Redde(_) => "redde",
            HirStatementKind::ReddeFilum(_) => "redde-filum",
            HirStatementKind::Rumpe => "rumpe",
            HirStatementKind::Perge => "perge",
            HirStatementKind::Tacet => "tacet",
            HirStatementKind::IncDec(_) => "inc-dec",
            HirStatementKind::Custodi(_) => "custodi",
        };
        if let HirStatementKind::Redde(v) = &s.kind {
            println!("STMT redde@{} value={:?}", s.id.0, v.as_ref().map(|x| x.id.0));
        }
        visit::walk_stmt(self, s);
    }
}

#[test]
fn debug_reducta_ops_sites() {
    let raw = std::fs::read_to_string("../../corpus/summa/reducta-ops.fab").unwrap();
    let rest = raw.split("+++").nth(2).unwrap();
    let result = analyze_source_la(rest);
    let hir = result.hir.as_ref().expect("hir");
    let facts = result.access_facts.as_ref().expect("facts");
    let regions = crate::passes::borrow_region::analyze(hir, &result.types, facts).expect("regions");
    let mut sites = Sites { sites: vec![], stack: vec![] };
    sites.visit_program(hir);
    let total = sites.sites.len();
    for (id, c) in sites.sites {
        let located = regions.cfg().node_for_hir_id(radix_hir::HirId(id as u32)).is_some();
        println!("{c}@{id} located={located}");
    }
    println!("total sites: {total}");
    for node in regions.cfg().nodes() {
        if let Some(h) = node.location.hir_id {
            if (20..=40).contains(&h.0) {
                println!("CFGNODE {:?} hir={} kind={:?}", node.id, h.0, node.kind);
            }
        }
    }
}
