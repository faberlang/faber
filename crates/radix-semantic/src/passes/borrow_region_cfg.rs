//! HIR-hosted control-flow graph construction for borrow-region inference.
//!
//! The graph is deliberately independent of MIR.  It retains HIR identities
//! on every source node and makes loop transfers explicit so later region
//! analysis can use an ordinary finite fixpoint.
//!
//! The adapter is total over the HIR statement and expression grammar: every
//! statement-bearing construct (functions, the entry block, closure bodies,
//! reduction/scan/sort lane bodies) becomes explicit nodes and edges, and no
//! shape collapses silently into a single node.  Adding an edge can only
//! lengthen inferred regions (the solver grows live sets with edges), so the
//! conservative rule throughout is: when the exact transfer points of a
//! construct cannot be named cheaply, model it with more edges, never fewer.

use radix_hir::{
    DefId, HirBlock, HirCape, HirExpression, HirExpressionKind, HirId, HirItemKind, HirModule,
    HirStatement, HirStatementKind,
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
    /// Transfer from a protected body to its error handler (a `cape` catch
    /// or the `⊥` default of `call ⊥ default`).
    ///
    /// WHY: any failable point inside the body may take this edge, and the
    /// adapter does not name failable points individually; an edge from every
    /// body node keeps every region at least as long as the truth, while a
    /// missing edge could shorten a region below its true liveness.
    Error,
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
///
/// The builder covers every HIR shape; this error remains as the tripwire for
/// malformed HIR (a loop transfer outside any loop) and for future HIR kinds
/// that have not been given a lowering yet, so a gap is loud instead of an
/// approximation.
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

/// A complete HIR CFG.  Functions, the module entry, closure bodies and
/// reduction lane bodies are represented as disconnected components, each
/// with an explicit synthetic entry and exit.
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

/// Build a deterministic HIR CFG for functions, the optional module entry,
/// and every closure or lane body nested inside them.
///
/// Malformed HIR (a loop transfer outside a loop) returns [`CfgError`] rather
/// than being approximated.  This keeps the semantic region consumer fail
/// closed: its caller may reject loudly instead of silently keeping the old
/// block-scope approximation.
///
/// # Errors
/// Returns `CfgError` when a control-flow shape is unsupported.
pub fn build_cfg(hir: &HirModule) -> Result<HirCfg, CfgError> {
    let mut builder = Builder {
        cfg: HirCfg::default(),
    };

    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                if let Some(body) = &function.body {
                    builder.add_block_component(body)?;
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(body) = &method.func.body {
                        builder.add_block_component(body)?;
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(entry) = &hir.entry {
        builder.add_block_component(entry)?;
    }

    Ok(builder.cfg)
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
    /// One disconnected component whose body is a block (a function, a
    /// method, the module entry).
    fn add_block_component(&mut self, block: &HirBlock) -> Result<(), CfgError> {
        self.isolated_component(block.span, |builder| builder.build_block(block, None))
    }

    /// One disconnected component whose body is a single expression (a
    /// closure body or a reduction lane body that is not a block).
    fn add_expr_component(&mut self, body: &HirExpression) -> Result<(), CfgError> {
        let span = body.span;
        self.isolated_component(span, |builder| builder.build_expression(body, None))
    }

    /// Build a fresh entry/exit pair around `build`, wire the fragment in,
    /// and reject loop transfers that escape the component.
    fn isolated_component(
        &mut self,
        span: Span,
        build: impl FnOnce(&mut Self) -> Result<Fragment, CfgError>,
    ) -> Result<(), CfgError> {
        let entry = self
            .cfg
            .add_node(CfgNodeKind::Entry, location(None, None, span));
        let exit = self
            .cfg
            .add_node(CfgNodeKind::Exit, location(None, None, span));
        self.cfg.entries.push(entry);
        self.cfg.exits.push(exit);

        let fragment = build(self)?;
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
                span,
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
        if (20..=30).contains(&statement.id.0) {
            eprintln!("DBG build_statement id={}", statement.id.0);
        }
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
                // The statement keeps its own node even with a value: a
                // borrow decision keyed at the `redde` site (a returned view)
                // needs a location, and the value's nodes alone would leave
                // none.
                if statement.id.0 == 24 {
                    eprintln!("DBG redde 24 value_some={} value_id={:?}", value.is_some(), value.as_ref().map(|v| v.id.0));
                }
                let mut fragment = if let Some(value) = value {
                    self.build_expression(value, loop_context)?
                } else {
                    Fragment::empty()
                };
                let node = self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                );
                self.connect_normals(&fragment, node, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(node);
                // A `redde` terminates the flow; there is no fall-through.
                fragment.normal = Vec::new();
                Ok(fragment)
            }
            HirStatementKind::Rumpe => {
                let node = self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                );
                if loop_context.is_none() {
                    return Err(CfgError::TransferOutsideLoop {
                        transfer: "rumpe",
                        span: statement.span,
                    });
                }
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
                if loop_context.is_none() {
                    return Err(CfgError::TransferOutsideLoop {
                        transfer: "perge",
                        span: statement.span,
                    });
                }
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
            HirStatementKind::Custodi(custodi) => {
                self.build_custodi(statement, custodi, loop_context)
            }
        }
    }

    /// A guard section is a chain of early-exit clauses.  Each clause's
    /// condition falls to the next clause when false; a clause body that
    /// completes normally continues at the next clause's condition (mirroring
    /// the MIR lowering, which emits one `si` per clause), and the last
    /// fall-through lands on the section's join.
    fn build_custodi(
        &mut self,
        statement: &HirStatement,
        custodi: &radix_hir::HirCustodi,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let mut clauses = Vec::new();
        for clause in &custodi.clauses {
            let condition = self.cfg.add_node(
                CfgNodeKind::Condition,
                location(Some(clause.cond.id), None, clause.cond.span),
            );
            let body = self.build_block(&clause.body, loop_context)?;
            clauses.push((condition, body));
        }
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(statement.id), None, statement.span),
        );
        for (index, (condition, body)) in clauses.iter().enumerate() {
            let successor = clauses
                .get(index + 1)
                .map_or(join, |(next, _)| *next);
            self.cfg
                .add_edge(*condition, successor, CfgEdgeKind::BranchFalse);
            if let Some(body_entry) = body.entry {
                self.cfg
                    .add_edge(*condition, body_entry, CfgEdgeKind::BranchTrue);
            }
            self.connect_normals(body, successor, CfgEdgeKind::Next);
        }
        let entry = clauses.first().map_or(join, |(condition, _)| *condition);
        let mut fragment = Fragment::terminal_with_entry(entry, join);
        for (_, body) in &clauses {
            fragment.breaks.extend(body.breaks.iter().copied());
            fragment.continues.extend(body.continues.iter().copied());
        }
        Ok(fragment)
    }

    fn build_expression(
        &mut self,
        expression: &HirExpression,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        if expression.id.0 == 25 {
            eprintln!("DBG build_expression id=25 loop_ctx={:?}", loop_context.is_some());
        }
        match &expression.kind {
            HirExpressionKind::Block(block) => self.build_block(block, loop_context),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => self.build_si(
                expression,
                cond,
                then_block,
                then_catch.as_deref(),
                else_block.as_ref(),
                loop_context,
            ),
            HirExpressionKind::Dum(cond, body) => self.build_dum(expression, cond, body),
            HirExpressionKind::Loop(body) => self.build_loop(expression, body),
            HirExpressionKind::Itera {
                iter: source, body, ..
            } => self.build_source_loop(
                expression,
                std::slice::from_ref(&**source),
                body,
                loop_context,
            ),
            HirExpressionKind::IteraLockstep { sources, body, .. } => {
                self.build_source_loop(expression, sources, body, loop_context)
            }
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                self.build_source_loop(expression, ranges, body, loop_context)
            }
            HirExpressionKind::Discerne { scrutinees, arms, .. } => {
                self.build_discerne(expression, scrutinees, arms, loop_context)
            }
            HirExpressionKind::Handled { body, catch } => {
                self.build_handled(expression, body, catch, loop_context)
            }
            HirExpressionKind::Trap(body) => self.build_trap(expression, body, loop_context),
            HirExpressionKind::CallDefault { body, default, .. } => {
                self.build_call_default(expression, body, default, loop_context)
            }
            // A closure value is created at this site; its body executes
            // later, on call, so the body is its own component with real
            // locations and no invented call-site edges.
            HirExpressionKind::Clausura(_, _, _, body) => {
                self.add_expr_component(body)?;
                Ok(self.leaf_fragment(expression))
            }
            // A reduction lane body is evaluated per element by the
            // operator; the operand, group, and identity expressions are the
            // only parts that run in the enclosing flow.
            HirExpressionKind::Reduction {
                source,
                group,
                body: Some(body),
                identity,
                ..
            } => {
                eprintln!("DBG reduction arm hit id={}", expression.id.0);
                let operands = [Some(&**source), group.as_deref(), identity.as_deref()];
                let mut fragment = self.build_operand_chain(&operands, loop_context)?;
                self.add_block_component(body)?;
                let node = self.leaf_node(expression);
                self.connect_normals(&fragment, node, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(node);
                fragment.normal = vec![node];
                Ok(fragment)
            }
            HirExpressionKind::Scan {
                source,
                group,
                body,
                ..
            } => {
                let operands = [Some(&**source), group.as_deref()];
                let mut fragment = self.build_operand_chain(&operands, loop_context)?;
                self.add_block_component(body)?;
                let node = self.leaf_node(expression);
                self.connect_normals(&fragment, node, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(node);
                fragment.normal = vec![node];
                Ok(fragment)
            }
            HirExpressionKind::Sort {
                source,
                body: Some(body),
                ..
            } => {
                let operands = [Some(&**source)];
                let mut fragment = self.build_operand_chain(&operands, loop_context)?;
                self.add_block_component(body)?;
                let node = self.leaf_node(expression);
                self.connect_normals(&fragment, node, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(node);
                fragment.normal = vec![node];
                Ok(fragment)
            }
            // VALUE EXPRESSIONS (named domain invariant): none of these kinds
            // hosts statements or control-flow edges of its own, so one node
            // carries the whole expression and inner uses key to it through
            // the ambient location.  A diverging `mori`/`iace` still falls
            // through here: the extra edge can only lengthen regions.  A new
            // expression kind with interior control flow must be matched
            // above — folding it here would silently approximate.
            HirExpressionKind::Path(_)
            | HirExpressionKind::Literal(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Binary(..)
            | HirExpressionKind::Unary(..)
            | HirExpressionKind::Call(..)
            | HirExpressionKind::Gradient { .. }
            | HirExpressionKind::MethodCall(..)
            | HirExpressionKind::Field(..)
            | HirExpressionKind::Index(..)
            | HirExpressionKind::OptionalChain(..)
            | HirExpressionKind::NonNull(..)
            | HirExpressionKind::Intervallum { .. }
            | HirExpressionKind::Assign(..)
            | HirExpressionKind::Marked { .. }
            | HirExpressionKind::ConversioAssign { .. }
            | HirExpressionKind::Array(_)
            | HirExpressionKind::Struct(..)
            | HirExpressionKind::Tuple(..)
            | HirExpressionKind::Scribe(..)
            | HirExpressionKind::Scriptum(..)
            | HirExpressionKind::ReadLine(_)
            | HirExpressionKind::Praefixum(_)
            | HirExpressionKind::Insere(_)
            | HirExpressionKind::Adfirma(..)
            | HirExpressionKind::Panic(_)
            | HirExpressionKind::Throw(_)
            | HirExpressionKind::Cede(_)
            | HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Yield(_)
            | HirExpressionKind::Verte { .. }
            | HirExpressionKind::Conversio { .. }
            | HirExpressionKind::Widen { .. }
            | HirExpressionKind::Ad { .. }
            | HirExpressionKind::Ref(..)
            | HirExpressionKind::Deref(_)
            | HirExpressionKind::TypeCheck { .. }
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::SinkInto { .. }
            | HirExpressionKind::Format { .. }
            | HirExpressionKind::Error
            | HirExpressionKind::Reduction { body: None, .. }
            | HirExpressionKind::Sort { body: None, .. } => Ok(self.leaf_fragment(expression)),
        }
    }

    fn build_si(
        &mut self,
        expression: &HirExpression,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let condition = self
            .cfg
            .add_node(CfgNodeKind::Condition, location(Some(cond.id), None, cond.span));
        let then_watermark = self.cfg.nodes.len();
        let then_fragment = self.build_block(then_block, loop_context)?;
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        if let Some(catch) = then_catch {
            // `si cond { … } cape e { … }`: errors inside the then-arm
            // transfer to the catch from any point, exactly like a handled
            // body; the false edge skips the arm and its handler entirely.
            self.connect_branch(condition, &then_fragment, CfgEdgeKind::BranchTrue);
            self.connect_normals(&then_fragment, join, CfgEdgeKind::Next);
            let catch_fragment = self.build_block(&catch.body, loop_context)?;
            self.connect_error_edges(then_watermark, catch_fragment.entry.unwrap_or(join));
            self.connect_normals(&catch_fragment, join, CfgEdgeKind::Next);
            self.cfg.add_edge(condition, join, CfgEdgeKind::BranchFalse);
            let mut fragment = Fragment::terminal_with_entry(condition, join);
            fragment.breaks.extend(then_fragment.breaks);
            fragment.breaks.extend(catch_fragment.breaks);
            fragment.continues.extend(then_fragment.continues);
            fragment.continues.extend(catch_fragment.continues);
            return Ok(fragment);
        }
        let else_fragment = if let Some(else_block) = else_block {
            self.build_block(else_block, loop_context)?
        } else {
            Fragment::empty()
        };
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

    fn build_dum(
        &mut self,
        expression: &HirExpression,
        cond: &HirExpression,
        body: &HirBlock,
    ) -> Result<Fragment, CfgError> {
        let header = self
            .cfg
            .add_node(CfgNodeKind::Condition, location(Some(cond.id), None, cond.span));
        let exit = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        let body_fragment = self.build_block(body, Some(LoopContext))?;
        if let Some(body_entry) = body_fragment.entry {
            self.cfg.add_edge(header, body_entry, CfgEdgeKind::BranchTrue);
        } else {
            self.cfg.add_edge(header, header, CfgEdgeKind::LoopBack);
        }
        self.cfg.add_edge(header, exit, CfgEdgeKind::BranchFalse);
        self.connect_normals(&body_fragment, header, CfgEdgeKind::LoopBack);
        self.connect_transfers(&body_fragment, header, exit);
        Ok(Fragment::terminal_with_entry(header, exit))
    }

    fn build_loop(&mut self, expression: &HirExpression, body: &HirBlock) -> Result<Fragment, CfgError> {
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

    /// The shared shape of `itera`, `itera ex`, and `itera in …` loops:
    /// operand expressions run first in the enclosing flow, then a loop
    /// header whose body cycles back and whose exit is the loop value.
    fn build_source_loop(
        &mut self,
        expression: &HirExpression,
        sources: &[HirExpression],
        body: &HirBlock,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let mut source_fragment = Fragment::empty();
        for source in sources {
            let next = self.build_expression(source, loop_context)?;
            self.sequence(&mut source_fragment, next);
        }
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
            self.cfg.add_edge(header, body_entry, CfgEdgeKind::BranchTrue);
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

    /// A `discerne` evaluates its scrutinees in order, then dispatches to one
    /// arm.  Every arm is reachable from the dispatch (each arm may run), the
    /// no-arm fall-through goes to the join, and each arm's normal completion
    /// joins the expression value.
    fn build_discerne(
        &mut self,
        expression: &HirExpression,
        scrutinees: &[HirExpression],
        arms: &[radix_hir::HirCasuArm],
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let mut scrutinee_fragment = Fragment::empty();
        for scrutinee in scrutinees {
            let next = self.build_expression(scrutinee, loop_context)?;
            self.sequence(&mut scrutinee_fragment, next);
        }
        let dispatch = self.cfg.add_node(
            CfgNodeKind::Condition,
            location(Some(expression.id), None, expression.span),
        );
        self.connect_normals(&scrutinee_fragment, dispatch, CfgEdgeKind::Next);
        let mut arm_fragments = Vec::with_capacity(arms.len());
        for arm in arms {
            arm_fragments.push(self.build_expression(&arm.body, loop_context)?);
        }
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        self.cfg.add_edge(dispatch, join, CfgEdgeKind::BranchFalse);
        for fragment in &arm_fragments {
            if let Some(entry) = fragment.entry {
                self.cfg.add_edge(dispatch, entry, CfgEdgeKind::BranchTrue);
            }
            self.connect_normals(fragment, join, CfgEdgeKind::Next);
        }
        let mut fragment =
            Fragment::terminal_with_entry(scrutinee_fragment.entry.unwrap_or(dispatch), join);
        for built in &arm_fragments {
            fragment.breaks.extend(built.breaks.iter().copied());
            fragment.continues.extend(built.continues.iter().copied());
        }
        fragment.breaks.extend(scrutinee_fragment.breaks);
        fragment.continues.extend(scrutinee_fragment.continues);
        Ok(fragment)
    }

    /// A `cape`-handled body: the body runs; any failable point inside it may
    /// transfer to the catch; both the body's normal completion and the
    /// catch's completion join at the expression value.
    fn build_handled(
        &mut self,
        expression: &HirExpression,
        body: &HirBlock,
        catch: &HirCape,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let watermark = self.cfg.nodes.len();
        let body_fragment = self.build_block(body, loop_context)?;
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        let catch_fragment = self.build_block(&catch.body, loop_context)?;
        self.connect_error_edges(watermark, catch_fragment.entry.unwrap_or(join));
        self.connect_normals(&body_fragment, join, CfgEdgeKind::Next);
        self.connect_normals(&catch_fragment, join, CfgEdgeKind::Next);
        let mut fragment =
            Fragment::terminal_with_entry(body_fragment.entry.unwrap_or(join), join);
        fragment.breaks.extend(body_fragment.breaks);
        fragment.breaks.extend(catch_fragment.breaks);
        fragment.continues.extend(body_fragment.continues);
        fragment.continues.extend(catch_fragment.continues);
        Ok(fragment)
    }

    /// A `capta` capture boundary: the body's trailing expression is the
    /// success value and every internal error path wraps into the same
    /// value.  Error paths only skip later body statements, and skipping
    /// code never lengthens a region, so the sequential body plus one join is
    /// the conservative lowering.
    fn build_trap(
        &mut self,
        expression: &HirExpression,
        body: &HirBlock,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let body_fragment = self.build_block(body, loop_context)?;
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        self.connect_normals(&body_fragment, join, CfgEdgeKind::Next);
        let mut fragment =
            Fragment::terminal_with_entry(body_fragment.entry.unwrap_or(join), join);
        fragment.breaks.extend(body_fragment.breaks);
        fragment.continues.extend(body_fragment.continues);
        Ok(fragment)
    }

    /// `call ⊥ default`: the default is evaluated only on failure, and
    /// failure is reachable from any point of the body, so every body node
    /// carries an error edge into the default.
    fn build_call_default(
        &mut self,
        expression: &HirExpression,
        body: &HirBlock,
        default: &HirExpression,
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let watermark = self.cfg.nodes.len();
        let body_fragment = self.build_block(body, loop_context)?;
        let default_fragment = self.build_expression(default, loop_context)?;
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        self.connect_error_edges(watermark, default_fragment.entry.unwrap_or(join));
        self.connect_normals(&body_fragment, join, CfgEdgeKind::Next);
        self.connect_normals(&default_fragment, join, CfgEdgeKind::Next);
        let mut fragment =
            Fragment::terminal_with_entry(body_fragment.entry.unwrap_or(join), join);
        fragment.breaks.extend(body_fragment.breaks);
        fragment.breaks.extend(default_fragment.breaks);
        fragment.continues.extend(body_fragment.continues);
        fragment.continues.extend(default_fragment.continues);
        Ok(fragment)
    }

    /// Sequence the non-`None` operand expressions of a lane operator.
    fn build_operand_chain(
        &mut self,
        operands: &[Option<&HirExpression>],
        loop_context: Option<LoopContext>,
    ) -> Result<Fragment, CfgError> {
        let mut fragment = Fragment::empty();
        for operand in operands.iter().flatten() {
            let next = self.build_expression(operand, loop_context)?;
            self.sequence(&mut fragment, next);
        }
        Ok(fragment)
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

    fn leaf_node(&mut self, expression: &HirExpression) -> CfgNodeId {
        self.cfg.add_node(
            CfgNodeKind::Expression,
            location(Some(expression.id), None, expression.span),
        )
    }

    fn leaf_fragment(&mut self, expression: &HirExpression) -> Fragment {
        Fragment::terminal(self.leaf_node(expression))
    }

    /// Error edges from every node built since `watermark` (see
    /// [`CfgEdgeKind::Error`]).
    fn connect_error_edges(&mut self, watermark: usize, handler: CfgNodeId) {
        for index in watermark..self.cfg.nodes.len() {
            self.cfg.add_edge(CfgNodeId(index), handler, CfgEdgeKind::Error);
        }
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

/// Whether a local binding's initializer determines control flow and so must
/// be built before the binding's own statement node.
fn is_control_expression(expression: &HirExpression) -> bool {
    matches!(
        expression.kind,
        HirExpressionKind::Block(_)
            | HirExpressionKind::Si { .. }
            | HirExpressionKind::Dum(_, _)
            | HirExpressionKind::Loop(_)
            | HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. }
            | HirExpressionKind::Discerne { .. }
            | HirExpressionKind::Handled { .. }
            | HirExpressionKind::Trap(_)
            | HirExpressionKind::CallDefault { .. }
    )
}

fn location(hir_id: Option<HirId>, def_id: Option<DefId>, span: Span) -> CfgLocation {
    CfgLocation {
        hir_id,
        def_id,
        span,
    }
}
