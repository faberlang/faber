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
/// every closure or lane body nested inside them, and module constants.
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
        scopes: Vec::new(),
    };

    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                if let Some(body) = &function.body {
                    builder.add_block_component(body)?;
                }
            }
            HirItemKind::Struct(strukt) => {
                for field in &strukt.fields {
                    // A field initializer (a class `fixum` constant's
                    // `praefixum { … }` body is the statement-bearing case)
                    // evaluates in its own context like a module constant.
                    if let Some(init) = &field.init {
                        builder.add_expr_component(init)?;
                    }
                }
                for method in &strukt.methods {
                    if let Some(body) = &method.func.body {
                        builder.add_block_component(body)?;
                    }
                }
            }
            // A module constant's initializer (a `praefixum { … }` block is
            // the statement-bearing case) evaluates in its own compile-time
            // context; it becomes one more disconnected component so its
            // statements keep real locations.
            HirItemKind::Constant(constant) => builder.add_expr_component(&constant.value)?,
            _ => {}
        }
    }
    if let Some(entry) = &hir.entry {
        builder.add_block_component(entry)?;
    }

    Ok(builder.cfg)
}

/// One entry of the active break/continue scope stack.
///
/// Mirrors the MIR `BreakableContext`: a loop owns both its break exit and
/// its continue point; a breakable `fac` block (ruling R3: "a breakable
/// block is not a loop") owns only its exit and passes `perge` through to
/// the enclosing loop. `rumpe` targets the innermost scope; `perge` targets
/// the innermost scope that still carries a continue point.
#[derive(Clone, Copy)]
struct Scope {
    break_to: CfgNodeId,
    continue_to: Option<CfgNodeId>,
}

struct Fragment {
    entry: Option<CfgNodeId>,
    normal: Vec<CfgNodeId>,
}

impl Fragment {
    fn empty() -> Self {
        Self {
            entry: None,
            normal: Vec::new(),
        }
    }

    fn terminal(entry: CfgNodeId) -> Self {
        Self {
            entry: Some(entry),
            normal: vec![entry],
        }
    }

    fn terminal_with_entry(entry: CfgNodeId, exit: CfgNodeId) -> Self {
        Self {
            entry: Some(entry),
            normal: vec![exit],
        }
    }
}

struct Builder {
    cfg: HirCfg,
    /// Active break/continue scopes; empty inside a fresh component (a
    /// function, closure or lane body owns no ambient loop), exactly as the
    /// resolver's scope chain stops at `ScopeKind::Function`.
    scopes: Vec<Scope>,
}

impl Builder {
    /// One disconnected component whose body is a block (a function, a
    /// method, the module entry).
    fn add_block_component(&mut self, block: &HirBlock) -> Result<(), CfgError> {
        self.isolated_component(block.span, |builder| builder.build_block(block))
    }

    /// One disconnected component whose body is a single expression (a
    /// closure body, a reduction lane body that is not a block, a module
    /// constant initializer).
    fn add_expr_component(&mut self, body: &HirExpression) -> Result<(), CfgError> {
        let span = body.span;
        self.isolated_component(span, |builder| builder.build_expression(body))
    }

    /// Build a fresh entry/exit pair around `build`, wire the fragment in,
    /// and build with no ambient break/continue scopes.
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

        // A component owns no ambient loop: transfers inside it must resolve
        // within it or fail closed, never silently borrow the caller's flow.
        let outer = std::mem::take(&mut self.scopes);
        let fragment = build(self);
        self.scopes = outer;

        let fragment = fragment?;
        if let Some(fragment_entry) = fragment.entry {
            self.cfg.add_edge(entry, fragment_entry, CfgEdgeKind::Next);
        } else {
            self.cfg.add_edge(entry, exit, CfgEdgeKind::Next);
        }
        for normal in fragment.normal {
            self.cfg.add_edge(normal, exit, CfgEdgeKind::Next);
        }
        Ok(())
    }

    fn build_block(&mut self, block: &HirBlock) -> Result<Fragment, CfgError> {
        // Ruling R3: a breakable block (`fac { … }`, the try body of
        // `fac … cape`) is not a loop. Body fall-through and every `rumpe`
        // land on one after-join; `perge` passes through to the enclosing
        // loop. Both exits share the node, exactly as the MIR lowering gives
        // them the same exit block, so a `rumpe` skips the rest of the body
        // without entering a `cape` handler.
        if block
            .breakable
            .is_some_and(radix_hir::HirBreakableKind::is_scope)
        {
            let after = self
                .cfg
                .add_node(CfgNodeKind::Join, location(None, None, block.span));
            self.scopes.push(Scope {
                break_to: after,
                continue_to: self.scopes.last().and_then(|scope| scope.continue_to),
            });
            let fragment = self.build_block_inner(block);
            self.scopes.pop();
            let fragment = fragment?;
            self.connect_normals(&fragment, after, CfgEdgeKind::Next);
            return Ok(Fragment {
                entry: fragment.entry.or(Some(after)),
                normal: vec![after],
            });
        }
        self.build_block_inner(block)
    }

    fn build_block_inner(&mut self, block: &HirBlock) -> Result<Fragment, CfgError> {
        let mut result = Fragment::empty();
        for statement in &block.statements {
            let next = self.build_statement(statement)?;
            self.sequence(&mut result, next);
        }
        if let Some(expr) = &block.expr {
            let next = self.build_expression(expr)?;
            self.sequence(&mut result, next);
        }
        Ok(result)
    }

    fn build_statement(&mut self, statement: &HirStatement) -> Result<Fragment, CfgError> {
        match &statement.kind {
            HirStatementKind::Local(local) => {
                let binding = self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), Some(local.def_id), statement.span),
                );
                let mut fragment = Fragment::empty();
                if let Some(init) = &local.init {
                    if is_control_expression(init) {
                        // The initializer decides control flow; build it into
                        // the flow before the binding's own node.
                        let next = self.build_expression(init)?;
                        self.sequence(&mut fragment, next);
                    } else {
                        // A plain initializer has no control flow of its own,
                        // but it may still host separate components (a
                        // reduction or closure value) whose statements need
                        // locations and whose operands run before the bind.
                        self.build_nested_components(init, &mut fragment)?;
                    }
                }
                self.connect_normals(&fragment, binding, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(binding);
                fragment.normal = vec![binding];
                Ok(fragment)
            }
            HirStatementKind::Expr(expr) => {
                let mut fragment = self.build_expression(expr)?;
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
                let mut fragment = if let Some(value) = value {
                    self.build_expression(value)?
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
                let Some(scope) = self.scopes.last().copied() else {
                    return Err(CfgError::TransferOutsideLoop {
                        transfer: "rumpe",
                        span: statement.span,
                    });
                };
                self.cfg.add_edge(node, scope.break_to, CfgEdgeKind::Break);
                Ok(Fragment {
                    entry: Some(node),
                    normal: Vec::new(),
                })
            }
            HirStatementKind::Perge => {
                let node = self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                );
                let Some(continue_to) = self.scopes.last().and_then(|scope| scope.continue_to)
                else {
                    return Err(CfgError::TransferOutsideLoop {
                        transfer: "perge",
                        span: statement.span,
                    });
                };
                self.cfg.add_edge(node, continue_to, CfgEdgeKind::Continue);
                Ok(Fragment {
                    entry: Some(node),
                    normal: Vec::new(),
                })
            }
            HirStatementKind::Tacet | HirStatementKind::IncDec(_) => {
                Ok(Fragment::terminal(self.cfg.add_node(
                    CfgNodeKind::Statement,
                    location(Some(statement.id), None, statement.span),
                )))
            }
            HirStatementKind::Custodi(custodi) => self.build_custodi(statement, custodi),
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
    ) -> Result<Fragment, CfgError> {
        let mut clauses = Vec::new();
        for clause in &custodi.clauses {
            let condition = self.cfg.add_node(
                CfgNodeKind::Condition,
                location(Some(clause.cond.id), None, clause.cond.span),
            );
            let body = self.build_block(&clause.body)?;
            clauses.push((condition, body));
        }
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(statement.id), None, statement.span),
        );
        for (index, (condition, body)) in clauses.iter().enumerate() {
            let successor = clauses.get(index + 1).map_or(join, |(next, _)| *next);
            self.cfg
                .add_edge(*condition, successor, CfgEdgeKind::BranchFalse);
            self.connect_branch(*condition, body, successor, CfgEdgeKind::BranchTrue);
            self.connect_normals(body, successor, CfgEdgeKind::Next);
        }
        let entry = clauses.first().map_or(join, |(condition, _)| *condition);
        Ok(Fragment::terminal_with_entry(entry, join))
    }

    fn build_expression(&mut self, expression: &HirExpression) -> Result<Fragment, CfgError> {
        match &expression.kind {
            HirExpressionKind::Block(block) => self.build_block(block),
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
            ),
            HirExpressionKind::Dum(cond, body) => self.build_dum(expression, cond, body),
            HirExpressionKind::Loop(body) => self.build_loop(expression, body),
            HirExpressionKind::Itera {
                iter: source, body, ..
            } => self.build_source_loop(expression, std::slice::from_ref(&**source), body),
            HirExpressionKind::IteraLockstep { sources, body, .. } => {
                self.build_source_loop(expression, sources, body)
            }
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                self.build_source_loop(expression, ranges, body)
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => self.build_discerne(expression, scrutinees, arms),
            HirExpressionKind::Handled { body, catch } => {
                self.build_handled(expression, body, catch)
            }
            HirExpressionKind::Trap(body) => self.build_trap(expression, body),
            HirExpressionKind::CallDefault { body, default, .. } => {
                self.build_call_default(expression, body, default)
            }
            // A closure value is created at this site; its body executes
            // later, on call, so the body is its own component with real
            // locations and no invented call-site edges.
            HirExpressionKind::Clausura(_, _, _, body) => {
                self.add_expr_component(body)?;
                Ok(self.leaf_fragment(expression))
            }
            // A `praefixum` block evaluates at build time in its own
            // context: one more disconnected component, like a closure body,
            // so its statements keep real locations.
            HirExpressionKind::Praefixum(body) => {
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
                let operands = [Some(&**source), group.as_deref(), identity.as_deref()];
                let mut fragment = self.build_operand_chain(&operands)?;
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
                let mut fragment = self.build_operand_chain(&operands)?;
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
                let mut fragment = self.build_operand_chain(&operands)?;
                self.add_block_component(body)?;
                let node = self.leaf_node(expression);
                self.connect_normals(&fragment, node, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(node);
                fragment.normal = vec![node];
                Ok(fragment)
            }
            // VALUE EXPRESSIONS (named domain invariant): none of these kinds
            // hosts control-flow edges of its own, so one node carries the
            // whole expression and inner uses key to it through the ambient
            // location. But a value expression may still *contain* a
            // statement-bearing construct (a closure or lane body nested in
            // a call argument, a conversion, an initializer): those become
            // their own components and their operand chains join this flow,
            // in evaluation order, before the value's own node. A new
            // expression kind with interior control flow must be matched
            // above — folding it here would silently approximate.
            _ => {
                let mut fragment = Fragment::empty();
                self.build_nested_components(expression, &mut fragment)?;
                let node = self.leaf_node(expression);
                self.connect_normals(&fragment, node, CfgEdgeKind::Next);
                fragment.entry.get_or_insert(node);
                fragment.normal = vec![node];
                Ok(fragment)
            }
        }
    }

    /// Walk a value expression and give every statement-bearing construct
    /// in it its components: the expression itself when it is one (a local
    /// initializer may be a whole closure or reduction value), otherwise its
    /// value children in evaluation order. Plain value children recurse
    /// without nodes (their uses key to the ambient location); component
    /// constructs are built through [`Self::build_expression`] so their
    /// operand chains, component bodies and site nodes all exist and
    /// sequence into `fragment`.
    fn build_nested_components(
        &mut self,
        expression: &HirExpression,
        fragment: &mut Fragment,
    ) -> Result<(), CfgError> {
        if is_separate_component(&expression.kind) {
            let next = self.build_expression(expression)?;
            self.sequence(fragment, next);
            return Ok(());
        }
        for child in value_children(&expression.kind) {
            if is_separate_component(&child.kind) {
                let next = self.build_expression(child)?;
                self.sequence(fragment, next);
            } else {
                self.build_nested_components(child, fragment)?;
            }
        }
        Ok(())
    }

    fn build_si(
        &mut self,
        expression: &HirExpression,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
    ) -> Result<Fragment, CfgError> {
        let condition = self.cfg.add_node(
            CfgNodeKind::Condition,
            location(Some(cond.id), None, cond.span),
        );
        let then_watermark = self.cfg.nodes.len();
        let then_fragment = self.build_block(then_block)?;
        let then_end = self.cfg.nodes.len();
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        if let Some(catch) = then_catch {
            // `si cond { … } cape e { … } [secus { … }]`: errors inside the
            // then-arm transfer to the catch from any point; the false edge
            // runs the else arm when present (leaving the handled region —
            // it never enters the catch) and otherwise skips straight to the
            // join, mirroring MIR `lower_handled_si_stmt`.
            self.connect_branch(condition, &then_fragment, join, CfgEdgeKind::BranchTrue);
            self.connect_normals(&then_fragment, join, CfgEdgeKind::Next);
            let catch_fragment = self.build_block(&catch.body)?;
            self.connect_error_edges(
                then_watermark,
                then_end,
                catch_fragment.entry.unwrap_or(join),
            );
            self.connect_normals(&catch_fragment, join, CfgEdgeKind::Next);
            let else_fragment = if let Some(else_block) = else_block {
                self.build_block(else_block)?
            } else {
                Fragment::empty()
            };
            self.connect_branch(condition, &else_fragment, join, CfgEdgeKind::BranchFalse);
            self.connect_normals(&else_fragment, join, CfgEdgeKind::Next);
            return Ok(Fragment::terminal_with_entry(condition, join));
        }
        let else_fragment = if let Some(else_block) = else_block {
            self.build_block(else_block)?
        } else {
            Fragment::empty()
        };
        self.connect_branch(condition, &then_fragment, join, CfgEdgeKind::BranchTrue);
        self.connect_branch(condition, &else_fragment, join, CfgEdgeKind::BranchFalse);
        self.connect_normals(&then_fragment, join, CfgEdgeKind::Next);
        self.connect_normals(&else_fragment, join, CfgEdgeKind::Next);
        Ok(Fragment::terminal_with_entry(condition, join))
    }

    fn build_dum(
        &mut self,
        expression: &HirExpression,
        cond: &HirExpression,
        body: &HirBlock,
    ) -> Result<Fragment, CfgError> {
        let header = self.cfg.add_node(
            CfgNodeKind::Condition,
            location(Some(cond.id), None, cond.span),
        );
        let exit = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        self.scopes.push(Scope {
            break_to: exit,
            continue_to: Some(header),
        });
        let body_fragment = self.build_block(body);
        self.scopes.pop();
        let body_fragment = body_fragment?;
        if let Some(body_entry) = body_fragment.entry {
            self.cfg
                .add_edge(header, body_entry, CfgEdgeKind::BranchTrue);
        } else {
            self.cfg.add_edge(header, header, CfgEdgeKind::LoopBack);
        }
        self.cfg.add_edge(header, exit, CfgEdgeKind::BranchFalse);
        self.connect_normals(&body_fragment, header, CfgEdgeKind::LoopBack);
        Ok(Fragment::terminal_with_entry(header, exit))
    }

    fn build_loop(
        &mut self,
        expression: &HirExpression,
        body: &HirBlock,
    ) -> Result<Fragment, CfgError> {
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
        self.scopes.push(Scope {
            break_to: exit,
            continue_to: Some(header),
        });
        let body_fragment = self.build_block(body);
        self.scopes.pop();
        let body_fragment = body_fragment?;
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
    ) -> Result<Fragment, CfgError> {
        let mut source_fragment = Fragment::empty();
        for source in sources {
            let next = self.build_expression(source)?;
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
        self.scopes.push(Scope {
            break_to: exit,
            continue_to: Some(header),
        });
        let body_fragment = self.build_block(body);
        self.scopes.pop();
        let body_fragment = body_fragment?;
        self.connect_normals(&source_fragment, header, CfgEdgeKind::Next);
        if let Some(body_entry) = body_fragment.entry {
            self.cfg
                .add_edge(header, body_entry, CfgEdgeKind::BranchTrue);
        } else {
            self.cfg.add_edge(header, header, CfgEdgeKind::LoopBack);
        }
        self.cfg.add_edge(header, exit, CfgEdgeKind::LoopExit);
        self.connect_normals(&body_fragment, header, CfgEdgeKind::LoopBack);
        Ok(Fragment::terminal_with_entry(
            source_fragment.entry.unwrap_or(header),
            exit,
        ))
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
    ) -> Result<Fragment, CfgError> {
        let mut scrutinee_fragment = Fragment::empty();
        for scrutinee in scrutinees {
            let next = self.build_expression(scrutinee)?;
            self.sequence(&mut scrutinee_fragment, next);
        }
        let dispatch = self.cfg.add_node(
            CfgNodeKind::Condition,
            location(Some(expression.id), None, expression.span),
        );
        self.connect_normals(&scrutinee_fragment, dispatch, CfgEdgeKind::Next);
        let mut arm_fragments = Vec::with_capacity(arms.len());
        for arm in arms {
            arm_fragments.push(self.build_expression(&arm.body)?);
        }
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        self.cfg.add_edge(dispatch, join, CfgEdgeKind::BranchFalse);
        for fragment in &arm_fragments {
            self.connect_branch(dispatch, fragment, join, CfgEdgeKind::BranchTrue);
            self.connect_normals(fragment, join, CfgEdgeKind::Next);
        }
        Ok(Fragment::terminal_with_entry(
            scrutinee_fragment.entry.unwrap_or(dispatch),
            join,
        ))
    }

    /// A `cape`-handled body: the body runs; any failable point inside it may
    /// transfer to the catch; both the body's normal completion and the
    /// catch's completion join at the expression value.
    fn build_handled(
        &mut self,
        expression: &HirExpression,
        body: &HirBlock,
        catch: &HirCape,
    ) -> Result<Fragment, CfgError> {
        let watermark = self.cfg.nodes.len();
        let body_fragment = self.build_block(body)?;
        let body_end = self.cfg.nodes.len();
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        let catch_fragment = self.build_block(&catch.body)?;
        self.connect_error_edges(watermark, body_end, catch_fragment.entry.unwrap_or(join));
        self.connect_normals(&body_fragment, join, CfgEdgeKind::Next);
        self.connect_normals(&catch_fragment, join, CfgEdgeKind::Next);
        Ok(Fragment::terminal_with_entry(
            body_fragment.entry.unwrap_or(join),
            join,
        ))
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
    ) -> Result<Fragment, CfgError> {
        let body_fragment = self.build_block(body)?;
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        self.connect_normals(&body_fragment, join, CfgEdgeKind::Next);
        Ok(Fragment::terminal_with_entry(
            body_fragment.entry.unwrap_or(join),
            join,
        ))
    }

    /// `call ⊥ default`: the default is evaluated only on failure, and
    /// failure is reachable from any point of the body, so every body node
    /// carries an error edge into the default.
    fn build_call_default(
        &mut self,
        expression: &HirExpression,
        body: &HirBlock,
        default: &HirExpression,
    ) -> Result<Fragment, CfgError> {
        let watermark = self.cfg.nodes.len();
        let body_fragment = self.build_block(body)?;
        let body_end = self.cfg.nodes.len();
        let default_fragment = self.build_expression(default)?;
        let join = self.cfg.add_node(
            CfgNodeKind::Join,
            location(Some(expression.id), None, expression.span),
        );
        self.connect_error_edges(watermark, body_end, default_fragment.entry.unwrap_or(join));
        self.connect_normals(&body_fragment, join, CfgEdgeKind::Next);
        self.connect_normals(&default_fragment, join, CfgEdgeKind::Next);
        Ok(Fragment::terminal_with_entry(
            body_fragment.entry.unwrap_or(join),
            join,
        ))
    }

    /// Sequence the non-`None` operand expressions of a lane operator.
    fn build_operand_chain(
        &mut self,
        operands: &[Option<&HirExpression>],
    ) -> Result<Fragment, CfgError> {
        let mut fragment = Fragment::empty();
        for operand in operands.iter().flatten() {
            let next = self.build_expression(operand)?;
            self.sequence(&mut fragment, next);
        }
        Ok(fragment)
    }

    /// Ruling R3: `perge` lands at the end of a `fac … dum` body, so the
    /// synthesized `si non c { rumpe }` test runs next. The test is built
    /// first so a user `perge` in the body can name its entry; body
    /// fallthrough and every `perge` edge into the test; the test's own
    /// `rumpe` leaves the loop and its fallthrough returns to the header.
    /// Mirrors MIR `lower_loop_expr`.
    fn build_fac_dum(
        &mut self,
        header: CfgNodeId,
        exit: CfgNodeId,
        statements: &[HirStatement],
        test: &HirStatement,
    ) -> Result<Fragment, CfgError> {
        self.scopes.push(Scope {
            break_to: exit,
            continue_to: None,
        });
        let built = self.build_fac_dum_inner(header, exit, statements, test);
        self.scopes.pop();
        built
    }

    fn build_fac_dum_inner(
        &mut self,
        header: CfgNodeId,
        exit: CfgNodeId,
        statements: &[HirStatement],
        test: &HirStatement,
    ) -> Result<Fragment, CfgError> {
        let test_fragment = self.build_statement(test)?;
        let test_entry = test_fragment.entry.ok_or(CfgError::UnsupportedShape {
            construct: "fac dum test without a condition entry",
            span: test.span,
        })?;
        // With the test's entry known, a `perge` in the body names it.
        if let Some(scope) = self.scopes.last_mut() {
            scope.continue_to = Some(test_entry);
        }
        let mut body_fragment = Fragment::empty();
        for statement in statements {
            let next = self.build_statement(statement)?;
            self.sequence(&mut body_fragment, next);
        }
        let body_entry = body_fragment.entry.unwrap_or(test_entry);
        self.cfg.add_edge(header, body_entry, CfgEdgeKind::Next);
        self.connect_normals(&body_fragment, test_entry, CfgEdgeKind::Next);
        self.connect_normals(&test_fragment, header, CfgEdgeKind::LoopBack);
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

    /// Error edges from nodes in `start..end` (see [`CfgEdgeKind::Error`]).
    fn connect_error_edges(&mut self, start: usize, end: usize, handler: CfgNodeId) {
        for index in start..end {
            self.cfg
                .add_edge(CfgNodeId(index), handler, CfgEdgeKind::Error);
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
    }

    fn connect_normals(&mut self, fragment: &Fragment, target: CfgNodeId, kind: CfgEdgeKind) {
        for from in &fragment.normal {
            self.cfg.add_edge(*from, target, kind);
        }
    }

    /// Connect a branch to its fragment, or to `empty_target` when the
    /// fragment has no node (an empty block or expression). Empty branches
    /// still represent reachable control flow and must not disappear.
    fn connect_branch(
        &mut self,
        from: CfgNodeId,
        fragment: &Fragment,
        empty_target: CfgNodeId,
        kind: CfgEdgeKind,
    ) {
        self.cfg
            .add_edge(from, fragment.entry.unwrap_or(empty_target), kind);
    }
}

/// Whether an expression kind is a statement-bearing construct that runs as
/// its own CFG component (its body in a disconnected part of the graph, its
/// operand expressions in the enclosing flow).
fn is_separate_component(kind: &HirExpressionKind) -> bool {
    matches!(
        kind,
        HirExpressionKind::Clausura(..)
            | HirExpressionKind::Praefixum(_)
            | HirExpressionKind::Reduction { body: Some(_), .. }
            | HirExpressionKind::Scan { .. }
            | HirExpressionKind::Sort { body: Some(_), .. }
    )
}

/// The direct value-position children of a value expression, in evaluation
/// order. Control-flow and component kinds return only the operands that
/// still evaluate in the enclosing flow (a body-less reduction or sort names
/// its source, group and identity); their statement-bearing parts are owned
/// by the dedicated builder arms.
fn value_children(kind: &HirExpressionKind) -> Vec<&HirExpression> {
    use HirExpressionKind as K;
    let mut children: Vec<&HirExpression> = Vec::new();

    match kind {
        K::Binary(_, left, right) => children.extend([left.as_ref(), right.as_ref()]),
        K::Unary(_, operand) => children.push(operand),
        K::Call(callee, _, args, _) => {
            children.push(callee);
            children.extend(args.iter().map(|arg| &arg.expr));
        }
        K::Gradient {
            call,
            selected_arguments,
        } => {
            children.push(call);
            children.extend(selected_arguments.iter());
        }
        K::MethodCall(receiver, _, _, args, _) => {
            children.push(receiver);
            children.extend(args.iter().map(|arg| &arg.expr));
        }
        K::Field(object, _) => children.push(object),
        K::Index(object, index) => children.extend([object.as_ref(), index.as_ref()]),
        K::OptionalChain(object, _) | K::NonNull(object, _) => children.push(object),
        K::Intervallum {
            start, end, step, ..
        } => {
            children.extend([start.as_ref(), end.as_ref()]);
            if let Some(step) = step {
                children.push(step);
            }
        }
        K::Assign(target, value) => children.extend([target.as_ref(), value.as_ref()]),
        K::Marked { expr, .. } => children.push(expr),
        K::ConversioAssign {
            target,
            source,
            recovery,
            ..
        } => {
            children.extend([target.as_ref(), source.as_ref()]);
            if let Some(recovery) = recovery {
                children.push(recovery);
            }
        }
        K::Array(elements) => {
            for element in elements {
                match element {
                    radix_hir::HirArrayElement::Expr(expr)
                    | radix_hir::HirArrayElement::Spread(expr) => children.push(expr),
                    radix_hir::HirArrayElement::Infer(_) => {}
                }
            }
        }
        K::Struct(_, fields) => children.extend(fields.iter().map(|(_, value)| value)),
        K::Tuple(elements, _) => children.extend(elements.iter()),
        K::Scribe(_, args) => children.extend(args.iter()),
        K::Scriptum(_, parts) => children.extend(parts.iter()),
        K::Adfirma(condition, message) => {
            children.push(condition);
            if let Some(message) = message {
                children.push(message);
            }
        }
        K::Panic(value) | K::Throw(value) => children.push(value),
        K::Cede(value) | K::Reddet(value) | K::Tacebit(value) | K::Yield(value) => {
            children.push(value);
        }
        K::Verte {
            source, entries, ..
        } => {
            children.push(source);
            if let Some(entries) = entries {
                children.extend(entries.iter().filter_map(|field| field.value.as_ref()));
            }
        }
        K::Conversio {
            source, recovery, ..
        } => {
            children.push(source);
            if let Some(recovery) = recovery {
                children.push(recovery);
            }
        }
        K::Widen { operand, .. } => children.push(operand),
        K::Ad {
            opener: Some(opener),
            ..
        } => children.push(opener),
        K::Ref(_, object) | K::Deref(object) => children.push(object),
        K::TypeCheck { expr, .. } => children.push(expr),
        K::CopyInto { target, value }
        | K::SinkInto {
            sink: target,
            item: value,
        } => {
            children.extend([target.as_ref(), value.as_ref()]);
        }
        K::Format { value, .. } => children.push(value),
        K::Reduction {
            source,
            group,
            identity,
            body: None,
            ..
        } => {
            children.push(source);
            if let Some(group) = group {
                children.push(group);
            }
            if let Some(identity) = identity {
                children.push(identity);
            }
        }
        K::Sort {
            source, body: None, ..
        } => children.push(source),
        _ => {}
    }
    children
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
