//! Program-level display-token table and the per-function id into it.
//!
//! Every function that holds a print-family site (`print`, interpolation,
//! `nota`, `mone`, `scribe`, `vide`) names the [`DisplayTokens`] of the file
//! it was written in. The table is deduplicated; ids are indexes into it and
//! are remapped when units merge. Nothing reads the table yet.

use crate::MirFunctionId;
use handroll::FxHashMap;
use radix_runtime_contract::display_tokens::DisplayTokens;

/// Index into [`MirDisplayTokens::tokens`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayTokensId(pub u32);

/// Deduplicated token table plus the id each print-bearing function uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirDisplayTokens {
    pub tokens: Vec<DisplayTokens>,
    pub by_function: FxHashMap<MirFunctionId, DisplayTokensId>,
}

impl MirDisplayTokens {
    /// Id of `tokens`, appending it when the table does not hold it yet.
    ///
    /// The table holds one entry per distinct locale in the program (a handful),
    /// so the index always fits `u32`; the conversion saturates rather than panics.
    pub fn intern(&mut self, tokens: &DisplayTokens) -> DisplayTokensId {
        let index = self
            .tokens
            .iter()
            .position(|existing| existing == tokens)
            .unwrap_or_else(|| {
                self.tokens.push(tokens.clone());
                self.tokens.len() - 1
            });
        DisplayTokensId(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// Record that `function` prints through `tokens`.
    pub fn assign(&mut self, function: MirFunctionId, tokens: &DisplayTokens) {
        let id = self.intern(tokens);
        self.by_function.insert(function, id);
    }

    /// Absorb `other`: its entries join this table (deduplicated), its
    /// function ids are remapped to this table, and its function ids pass
    /// through `map_function` (the unit's id shift). A function already present keeps its own id.
    pub fn absorb(&mut self, other: &Self, map_function: impl Fn(MirFunctionId) -> MirFunctionId) {
        let remap = other
            .tokens
            .iter()
            .map(|tokens| self.intern(tokens))
            .collect::<Vec<_>>();
        for (function, id) in &other.by_function {
            if let Some(mapped) = remap.get(id.0 as usize) {
                self.by_function
                    .entry(map_function(*function))
                    .or_insert(*mapped);
            }
        }
    }

    /// The tokens a function prints through, when it holds a print site.
    #[must_use]
    pub fn for_function(&self, function: MirFunctionId) -> Option<&DisplayTokens> {
        let id = self.by_function.get(&function)?;
        self.tokens.get(id.0 as usize)
    }
}

#[cfg(test)]
#[path = "display_tokens_test.rs"]
mod tests;
