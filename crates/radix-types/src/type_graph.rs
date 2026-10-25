//! Strongly connected components of a declared type graph.
//!
//! Two consumers read the same partition: the Rust carrier boxes the fields
//! that close a cycle (D12.2), and semantic analysis warns when a closable
//! value can sit inside a reference cycle, which never closes on last
//! reference (D8.3).

use crate::DefId;
use handroll::{FxHashMap, FxHashSet};

/// Component index per node; nodes on a cycle share an index. A node that is
/// not on any cycle gets its own index (it is cyclic only through a self
/// edge, which callers read from their own edge lists).
#[must_use]
// The hasher is deliberately fixed to Fx: callers and the result use Fx
// collections throughout, so generalizing over `BuildHasher` adds nothing.
#[allow(clippy::implicit_hasher)]
pub fn strongly_connected_components(
    nodes: &FxHashSet<DefId>,
    edges: &FxHashMap<DefId, Vec<DefId>>,
) -> FxHashMap<DefId, usize> {
    struct Tarjan<'e> {
        edges: &'e FxHashMap<DefId, Vec<DefId>>,
        index: FxHashMap<DefId, usize>,
        low: FxHashMap<DefId, usize>,
        stack: Vec<DefId>,
        on_stack: FxHashSet<DefId>,
        component: FxHashMap<DefId, usize>,
        next_index: usize,
        next_component: usize,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, node: DefId) {
            self.index.insert(node, self.next_index);
            self.low.insert(node, self.next_index);
            self.next_index += 1;
            self.stack.push(node);
            self.on_stack.insert(node);
            let successors = self.edges.get(&node).cloned().unwrap_or_default();
            for next in successors {
                if !self.index.contains_key(&next) {
                    self.visit(next);
                    let low = self.low[&node].min(self.low[&next]);
                    self.low.insert(node, low);
                } else if self.on_stack.contains(&next) {
                    let low = self.low[&node].min(self.index[&next]);
                    self.low.insert(node, low);
                }
            }
            if self.low[&node] == self.index[&node] {
                while let Some(member) = self.stack.pop() {
                    self.on_stack.remove(&member);
                    self.component.insert(member, self.next_component);
                    if member == node {
                        break;
                    }
                }
                self.next_component += 1;
            }
        }
    }

    let mut tarjan = Tarjan {
        edges,
        index: FxHashMap::default(),
        low: FxHashMap::default(),
        stack: Vec::new(),
        on_stack: FxHashSet::default(),
        component: FxHashMap::default(),
        next_index: 0,
        next_component: 0,
    };
    // Deterministic traversal order; the partition itself is order-free.
    let mut ordered: Vec<DefId> = nodes.iter().copied().collect();
    ordered.sort_unstable_by_key(|def| def.0);
    for node in ordered {
        if !tarjan.index.contains_key(&node) {
            tarjan.visit(node);
        }
    }
    tarjan.component
}
