//! HIR-hosted control-flow graph construction for borrow-region inference.
//!
//! The graph is deliberately independent of MIR.  It retains HIR identities
//! on every source node and makes loop transfers explicit so later region
//! analysis can use an ordinary finite fixpoint.

use radix_hir::visit::{HirVisitor, walk_expr, walk_stmt};
use radix_hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirModule, HirStatement,
    HirStatementKind,
};
use radix_lexer::Span;

/// Stable index of a node in a [`HirCfg`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CfgNodeId(pub usize);

/// The semantic edge vocabulary used by the region solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfgEdgeKind {
    /// Ordinary fall-through or expression sequencing.
    Next,
    /// True branch of a condition or loop header.
    BranchTrue,
    /// False branch of a condition or loop header.
    BranchFalse,
    /// Back-edge to a loop header.
    LoopBack,
    /// Edge leaving a loop through `rumpe` or an iterator exhaustion path.
    LoopExit,
    /// Explicit `perge` transfer to the active loop's continue point: the
    /// condition for `dum` and `fac … dum`, the next iteration for `itera`.
    Continue,
    /// Explicit `rumpe` transfer to the active loop exit.
    Break,
}

/// The kind of executable point represented by a graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfgNodeKind {
    Entry,
    Exit,
    Statement,
    Expression,
    Condition,
    LoopHeader,
    Join,
}

/// HIR identity retained by one graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfgLocation {
    pub hir_id: Option<HirId>,
    pub def_id: Option<DefId>,
    pub span: Span,
}

/// One deterministic graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfgNode {
    pub id: CfgNodeId,
    pub kind: CfgNodeKind,
    pub location: CfgLocation,
}

/// One directed graph edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CfgEdge {
    pub from: CfgNodeId,
    pub to: CfgNodeId,
    pub kind: CfgEdgeKind,
}

/// Fail-closed CFG construction error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgError {
    /// A control-flow shape has no conservative lowering in this adapter.
    UnsupportedShape { construct: &'static str, span: Span },
    /// A loop transfer appeared outside a loop context.
    TransferOutsideLoop { transfer: &'static str, span: Span },
}

impl std::fmt::Display for CfgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedShape { construct, .. } => {
                write!(
                    f,
                    "borrow-region CFG does not support HIR shape `{construct}`"
                )
            }
            Self::TransferOutsideLoop { transfer, .. } => {
                write!(f, "borrow-region CFG found `{transfer}` outside a loop")
            }
        }
    }
}

impl std::error::Error for CfgError {}

/// A complete HIR CFG.  Functions and the module entry are represented as
/// disconnected components, each with an explicit synthetic entry and exit.
#[derive(Debug, Clone, Default)]
pub struct HirCfg {
    nodes: Vec<CfgNode>,
    edges: Vec<CfgEdge>,
    entries: Vec<CfgNodeId>,
    exits: Vec<CfgNodeId>,
}

/// Short alias for callers that use the generic CFG vocabulary.
pub type Cfg = HirCfg;

impl HirCfg {
    #[must_use]
    pub fn nodes(&self) -> &[CfgNode] {
        &self.nodes
    }

    #[must_use]
    pub fn edges(&self) -> &[CfgEdge] {
        &self.edges
    }

    #[must_use]
    pub fn entries(&self) -> &[CfgNodeId] {
        &self.entries
    }

    #[must_use]
    pub fn exits(&self) -> &[CfgNodeId] {
        &self.exits
    }

    /// Find the first node carrying a HIR identity.
    #[must_use]
    pub fn node_for_hir_id(&self, hir_id: HirId) -> Option<CfgNodeId> {
        self.nodes
            .iter()
            .find(|node| node.location.hir_id == Some(hir_id))
            .map(|node| node.id)
    }

    /// Return all outgoing edges in deterministic insertion order.
    pub fn successors(&self, node: CfgNodeId) -> impl Iterator<Item = CfgEdge> + '_ {
        self.edges
            .iter()
            .copied()
            .filter(move |edge| edge.from == node)
    }

    /// Return all incoming edges in deterministic insertion order.
    pub fn predecessors(&self, node: CfgNodeId) -> impl Iterator<Item = CfgEdge> + '_ {
        self.edges
            .iter()
            .copied()
            .filter(move |edge| edge.to == node)
    }

    fn add_node(&mut self, kind: CfgNodeKind, location: CfgLocation) -> CfgNodeId {
        let id = CfgNodeId(self.nodes.len());
        self.nodes.push(CfgNode { id, kind, location });
        id
    }

    fn add_edge(&mut self, from: CfgNodeId, to: CfgNodeId, kind: CfgEdgeKind) {
        self.edges.push(CfgEdge { from, to, kind });
    }
}

/// Build a deterministic HIR CFG for functions and the optional module entry.
///
/// Unsupported control-flow shapes return [`CfgError`] rather than being
/// approximated.  This makes the semantic region consumer fail closed: its
/// caller may retain the existing checker for a shape the adapter cannot prove.
///
/// # Errors
/// Returns `CfgError` when a control-flow shape is unsupported.
pub fn build_cfg(hir: &HirModule) -> Result<HirCfg, CfgError> {
    validate_supported_shapes(hir)?;
    let mut builder = Builder {
        cfg: HirCfg::default(),
    };

    for item in &hir.items {
        if let HirItemKind::Function(function) = &item.kind {
            if let Some(body) = &function.body {
                builder.add_component(body)?;
            }
        } else if let HirItemKind::Struct(strukt) = &item.kind {
            for method in &strukt.methods {
                if let Some(body) = &method.func.body {
                    builder.add_component(body)?;
                }
            }
        }
    }
    if let Some(entry) = &hir.entry {
        builder.add_component(entry)?;
    }

    Ok(builder.cfg)
}

fn validate_supported_shapes(hir: &HirModule) -> Result<(), CfgError> {
    struct ShapeChecker {
        error: Option<CfgError>,
    }

    impl HirVisitor for ShapeChecker {
        fn visit_stmt(&mut self, statement: &HirStatement) {
            if self.error.is_none() && matches!(&statement.kind, HirStatementKind::Custodi(_)) {
                self.error = Some(CfgError::UnsupportedShape {
                    construct: "custodi",
                    span: statement.span,
                });
            }
            walk_stmt(self, statement);
        }

        fn visit_expr(&mut self, expression: &HirExpression) {
            if self.error.is_none() {
                let construct = match &expression.kind {
                    HirExpressionKind::Discerne { .. } => Some("discerne"),
                    HirExpressionKind::Handled { .. } => Some("handled"),
                    _ => None,
                };
                if let Some(construct) = construct {
                    self.error = Some(CfgError::UnsupportedShape {
                        construct,
                        span: expression.span,
                    });
                }
            }
            walk_expr(self, expression);
        }
    }

    let mut checker = ShapeChecker { error: None };
    checker.visit_program(hir);
    checker.error.map_or(Ok(()), Err)
}

struct Fragment {
    entry: Option<CfgNodeId>,
    normal: Vec<CfgNodeId>,
    breaks: Vec<CfgNodeId>,
    continues: Vec<CfgNodeId>,
}

impl Fragment {
    fn empty() -> Self {
        Self {
            entry: None,
            normal: Vec::new(),
            breaks: Vec::new(),
            continues: Vec::new(),
        }
    }

    fn terminal(entry: CfgNodeId) -> Self {
        Self {
            entry: Some(entry),
            normal: vec![entry],
            breaks: Vec::new(),
            continues: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct LoopContext;

struct Builder {
    cfg: HirCfg,
}

impl Builder {
    fn add_component(&mut self, block: &HirBlock) -> Result<(), CfgError> {
        let entry = self
            .cfg
            .add_node(CfgNodeKind::Entry, location(None, None, block.span));
        let exit = self
            .cfg
            .add_node(CfgNodeKind::Exit, location(None, None, block.span));
        self.cfg.entries.push(entry);
        self.cfg.exits.push(exit);

        let fragment = self.build_block(block, None)?;
        if let Some(fragment_entry) = fragment.entry {
            self.cfg.add_edge(entry, fragment_entry, CfgEdgeKind::Next);
        } else {
            self.cfg.add_edge(entry, exit, CfgEdgeKind::Next);
        }
        for normal in fragment.normal {
            self.cfg.add_edge(normal, exit, CfgEdgeKind::Next);
        }
        for transfer in fragment.breaks {
            self.cfg.add_edge(transfer, exit, CfgEdgeKind::Break);
        }
        if !fragment.continues.is_empty() {
            return Err(CfgError::TransferOutsideLoop {
                transfer: "perge",
                span: block.span,
            });
        }
        Ok(())
    }

    fn build_block(
        &mut self,
        block: &HirBlock,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let mut result = Fragment::empty();
        for statement in &block.statements {
            let next = self.build_statement(statement, loop_context)?;
            self.sequence(&mut result, next);
        }
        if let Some(expr) = &block.expr {
            let next = self.build_expression(expr, loop_context)?;
            self.sequence(&mut result, next);
        }
        Ok(result)
    }

    fn build_statement(
        &mut self,
        statement: &HirStatement,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        match &statement.kind {
            HirStatementKind::Local(local) => {
                if let Some(init) = &local.init
                    && is_control_expression(init)
                {
                    let mut fragment = self.build_expression(init, loop_context)?;
                    let binding = self.cfg.add_node(
                        CfgNodeKind::Statement,
                        location(Some(statement.id), Some(local.def_id), statement.span),
                    );
                    self.connect_normals(&fragment, binding, CfgEdgeKind::Next);
                    fragment.entry.get_or_insert(binding);
                    fragment.normal = vec![binding];
                    return Ok(fragment);
                }
                Ok(Fragment::terminal(self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), Some(local.def_id), statement.span),
                )))
            }
            HirStatementKind::Expr(expr) => {
                let mut fragment = self.build_expression(expr, loop_context)?;
                let statement_node = self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                );
                self.connect_normals(&fragment, statement_node, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(statement_node);
                fragment.normal = vec![statement_node];
                Ok(fragment)
            }
            HirStatementKind::Redde(value) | HirStatementKind::ReddeFilum(value) => {
                let mut fragment = if let Some(value) = value {
                    self.build_expression(value, loop_context)?
                } else {
                    Fragment::terminal(self.cfg.add_node(
                        CfgNodeKind::Statement,
                        location(Some(statement.id), None, statement.span),
                    ))
                };
                fragment.normal.clear();
                Ok(fragment)
            }
            HirStatementKind::Rumpe => {
                let node = self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                );
                let Some(_) = loop_context else {
                    return Err(CfgError::TransferOutsideLoop {
                        transfer: "rumpe",
                        span: statement.span,
                    });
                };
                Ok(Fragment {
                    entry: Some(node),
                    normal: Vec::new(),
                    breaks: vec![node],
                    continues: Vec::new(),
                })
            }
            HirStatementKind::Perge => {
                let node = self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                );
                let Some(_) = loop_context else {
                    return Err(CfgError::TransferOutsideLoop {
                        transfer: "perge",
                        span: statement.span,
                    });
                };
                Ok(Fragment {
                    entry: Some(node),
                    normal: Vec::new(),
                    breaks: Vec::new(),
                    continues: vec![node],
                })
            }
            HirStatementKind::Tacet | HirStatementKind::IncDec(_) => {
                Ok(Fragment::terminal(self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                )))
            }
            HirStatementKind::Custodi(_) => Err(CfgError::UnsupportedShape {
                construct: "custodi",
                span: statement.span,
            }),
        }
    }

    fn build_expression(
        &mut self,
        expression: &HirExpression,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        match &expression.kind {
            HirExpressionKind::Block(block) => self.build_block(block, loop_context),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                let condition = self.cfg.add_node(
                    CfgNodeKind::Condition,
                    location(Some(cond.id), None, cond.span),
                );
                let then_fragment = self.build_block(then_block, loop_context)?;
                let else_fragment = if let Some(else_block) = else_block {
                    self.build_block(else_block, loop_context)?
                } else if let Some(catch) = then_catch {
                    self.build_block(&catch.body, loop_context)?
                } else {
                    Fragment::empty()
                };
                let join = self.cfg.add_node(
                    CfgNodeKind::Join,
                    location(Some(expression.id), None, expression.span),
                );
                self.connect_branch(condition, &then_fragment, CfgEdgeKind::BranchTrue);
                if else_fragment.entry.is_some() {
                    self.connect_branch(condition, &else_fragment, CfgEdgeKind::BranchFalse);
                } else {
                    self.cfg.add_edge(condition, join, CfgEdgeKind::BranchFalse);
                }
                self.connect_normals(&then_fragment, join, CfgEdgeKind::Next);
                self.connect_normals(&else_fragment, join, CfgEdgeKind::Next);
                let mut fragment = Fragment::terminal_with_entry(condition, join);
                fragment.breaks.extend(then_fragment.breaks);
                fragment.breaks.extend(else_fragment.breaks);
                fragment.continues.extend(then_fragment.continues);
                fragment.continues.extend(else_fragment.continues);
                Ok(fragment)
            }
            HirExpressionKind::Dum(cond, body) => {
                let header = self.cfg.add_node(
                    CfgNodeKind::Condition,
                    location(Some(cond.id), None, cond.span),
                );
                let exit = self.cfg.add_node(
                    CfgNodeKind::Join,
                    location(Some(expression.id), None, expression.span),
                );
                let body_fragment = self.build_block(body, Some(LoopContext))?;
                if let Some(body_entry) = body_fragment.entry {
                    self.cfg
                        .add_edge(header, body_entry, CfgEdgeKind::BranchTrue);
                } else {
                    self.cfg.add_edge(header, header, CfgEdgeKind::LoopBack);
                }
                self.cfg.add_edge(header, exit, CfgEdgeKind::BranchFalse);
                self.connect_normals(&body_fragment, header, CfgEdgeKind::LoopBack);
                self.connect_transfers(&body_fragment, header, exit);
                Ok(Fragment::terminal_with_entry(header, exit))
            }
            HirExpressionKind::Loop(body) => {
                let header = self.cfg.add_node(
                    CfgNodeKind::LoopHeader,
                    location(Some(expression.id), None, expression.span),
                );
                let exit = self.cfg.add_node(
                    CfgNodeKind::Join,
                    location(Some(expression.id), None, expression.span),
                );
                if let Some((statements, test, _)) = body.fac_dum_parts() {
                    return self.build_fac_dum(header, exit, statements, test);
                }
                let body_fragment = self.build_block(body, Some(LoopContext))?;
                if let Some(body_entry) = body_fragment.entry {
                    self.cfg.add_edge(header, body_entry, CfgEdgeKind::Next);
                } else {
                    self.cfg.add_edge(header, header, CfgEdgeKind::LoopBack);
                }
                if body_fragment.normal.is_empty() {
                    self.cfg.add_edge(header, header, CfgEdgeKind::LoopBack);
                } else {
                    self.connect_normals(&body_fragment, header, CfgEdgeKind::LoopBack);
                }
                self.connect_transfers(&body_fragment, header, exit);
                Ok(Fragment::terminal_with_entry(header, exit))
            }
            HirExpressionKind::Itera {
                iter: source, body, ..
            } => {
                let source_fragment = self.build_expression(source, loop_context)?;
                let header = self.cfg.add_node(
                    CfgNodeKind::LoopHeader,
                    location(Some(expression.id), None, expression.span),
                );
                let exit = self.cfg.add_node(
                    CfgNodeKind::Join,
                    location(Some(expression.id), None, expression.span),
                );
                let body_fragment = self.build_block(body, Some(LoopContext))?;
                self.connect_normals(&source_fragment, header, CfgEdgeKind::Next);
                if let Some(body_entry) = body_fragment.entry {
                    self.cfg
                        .add_edge(header, body_entry, CfgEdgeKind::BranchTrue);
                } else {
                    self.cfg.add_edge(header, header, CfgEdgeKind::LoopBack);
                }
                self.cfg.add_edge(header, exit, CfgEdgeKind::LoopExit);
                self.connect_normals(&body_fragment, header, CfgEdgeKind::LoopBack);
                self.connect_transfers(&body_fragment, header, exit);
                let mut fragment =
                    Fragment::terminal_with_entry(source_fragment.entry.unwrap_or(header), exit);
                fragment.breaks = source_fragment.breaks;
                fragment.continues = source_fragment.continues;
                Ok(fragment)
            }
            HirExpressionKind::Discerne { .. } => Err(CfgError::UnsupportedShape {
                construct: "discerne",
                span: expression.span,
            }),
            HirExpressionKind::Handled { .. } => Err(CfgError::UnsupportedShape {
                construct: "handled",
                span: expression.span,
            }),
            _ => Ok(Fragment::terminal(self.cfg.add_node(
                CfgNodeKind::Expression,
                location(Some(expression.id), None, expression.span),
            ))),
        }
    }

    /// Ruling R3: `perge` lands at the end of a `fac … dum` body, so the
    /// synthesized `si non c { rumpe }` test runs next. Body fallthrough and
    /// every `perge` edge into the test; the test's `rumpe` leaves the loop and
    /// its fallthrough returns to the header. Mirrors MIR `lower_loop_expr`.
    fn build_fac_dum(
        &mut self,
        header: CfgNodeId,
        exit: CfgNodeId,
        statements: &[HirStatement],
        test: &HirStatement,
    ) -> Result<Fragment, CfgError> {
        let mut body_fragment = Fragment::empty();
        for statement in statements {
            let next = self.build_statement(statement, Some(LoopContext))?;
            self.sequence(&mut body_fragment, next);
        }
        let test_fragment = self.build_statement(test, Some(LoopContext))?;
        let test_entry = test_fragment.entry.ok_or(CfgError::UnsupportedShape {
            construct: "fac dum test without a condition entry",
            span: test.span,
        })?;
        let body_entry = body_fragment.entry.unwrap_or(test_entry);
        self.cfg.add_edge(header, body_entry, CfgEdgeKind::Next);
        self.connect_normals(&body_fragment, test_entry, CfgEdgeKind::Next);
        self.connect_transfers(&body_fragment, test_entry, exit);
        self.connect_normals(&test_fragment, header, CfgEdgeKind::LoopBack);
        self.connect_transfers(&test_fragment, header, exit);
        Ok(Fragment::terminal_with_entry(header, exit))
    }

    fn sequence(&mut self, previous: &mut Fragment, next: Fragment) {
        if previous.entry.is_none() {
            *previous = next;
            return;
        }
        if previous.normal.is_empty() {
            return;
        }
        // WHY: an empty fragment (an empty block) has no node to enter and
        // nothing to fall out of; it is the identity, not a terminator.
        let Some(next_entry) = next.entry else {
            return;
        };
        self.connect_normals(previous, next_entry, CfgEdgeKind::Next);
        previous.normal = next.normal;
        previous.breaks.extend(next.breaks);
        previous.continues.extend(next.continues);
    }

    fn connect_normals(&mut self, fragment: &Fragment, target: CfgNodeId, kind: CfgEdgeKind) {
        for from in &fragment.normal {
            self.cfg.add_edge(*from, target, kind);
        }
    }

    fn connect_branch(&mut self, from: CfgNodeId, fragment: &Fragment, kind: CfgEdgeKind) {
        if let Some(entry) = fragment.entry {
            self.cfg.add_edge(from, entry, kind);
        }
    }

    fn connect_transfers(&mut self, fragment: &Fragment, header: CfgNodeId, exit: CfgNodeId) {
        for from in &fragment.continues {
            self.cfg.add_edge(*from, header, CfgEdgeKind::Continue);
        }
        for from in &fragment.breaks {
            self.cfg.add_edge(*from, exit, CfgEdgeKind::Break);
        }
    }
}

impl Fragment {
    fn terminal_with_entry(entry: CfgNodeId, exit: CfgNodeId) -> Self {
        Self {
            entry: Some(entry),
            normal: vec![exit],
            breaks: Vec::new(),
            continues: Vec::new(),
        }
    }
}

fn is_control_expression(expression: &HirExpression) -> bool {
    matches!(
        expression.kind,
        HirExpressionKind::Block(_)
            | HirExpressionKind::Si { .. }
            | HirExpressionKind::Dum(_, _)
            | HirExpressionKind::Loop(_)
            | HirExpressionKind::Itera { .. }
            | HirExpressionKind::Discerne { .. }
            | HirExpressionKind::Handled { .. }
    )
}

fn location(hir_id: Option<HirId>, def_id: Option<DefId>, span: Span) -> CfgLocation {
    CfgLocation {
        hir_id,
        def_id,
        span,
    }
}
