//! Generational arena handles for stable identity independent of list order.
//!
//! WHY: Scene graphs and shared resources need identity that survives reparent /
//! reorder. Integer list indexes retarget on remove; this store pairs slot index
//! with a generation so freed slots reject stale handles.
//!
//! Contract (language-surface mirror in examples/arena-handle):
//! - `insert` allocates a live slot and returns `ArenaHandle { index, generation }`
//! - `get` / `get_mut` succeed only when the handle generation matches
//! - `remove` frees the slot and bumps generation so the old handle is stale
//! - handles are `Copy` values; payloads are not deep-copied when handles are

use std::fmt;

/// Stable reference into an [`Arena`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArenaHandle {
    pub index: u32,
    pub generation: u32,
}

impl ArenaHandle {
    #[must_use]
    pub fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
}

impl fmt::Display for ArenaHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "manus({}:{})", self.index, self.generation)
    }
}

#[derive(Debug)]
enum Slot<T> {
    Free {
        generation: u32,
        next_free: Option<u32>,
    },
    Live {
        generation: u32,
        value: T,
    },
}

/// Generational arena store.
#[derive(Debug)]
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_head: Option<u32>,
    live_count: usize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Arena<T> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            live_count: 0,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.live_count
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live_count == 0
    }

    /// Allocate `value` and return a stable handle.
    pub fn insert(&mut self, value: T) -> ArenaHandle {
        if let Some(index) = self.free_head {
            let slot = &mut self.slots[index as usize];
            let generation = match slot {
                Slot::Free {
                    generation,
                    next_free,
                } => {
                    self.free_head = *next_free;
                    *generation
                }
                Slot::Live { .. } => unreachable!("free list pointed at live slot"),
            };
            *slot = Slot::Live { generation, value };
            self.live_count += 1;
            return ArenaHandle::new(index, generation);
        }

        // SAFETY: arena index fits in u32 for practical use.
        #[allow(clippy::cast_possible_truncation)]
        let index = self.slots.len() as u32;
        let generation = 0;
        self.slots.push(Slot::Live { generation, value });
        self.live_count += 1;
        ArenaHandle::new(index, generation)
    }

    /// Lookup by handle. Stale or out-of-range handles return `None`.
    #[must_use]
    pub fn get(&self, handle: ArenaHandle) -> Option<&T> {
        let slot = self.slots.get(handle.index as usize)?;
        match slot {
            Slot::Live { generation, value } if *generation == handle.generation => Some(value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, handle: ArenaHandle) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        match slot {
            Slot::Live { generation, value } if *generation == handle.generation => Some(value),
            _ => None,
        }
    }

    /// Free the slot. Bumps generation so the old handle is permanently stale.
    /// Returns the removed value when the handle was live.
    pub fn remove(&mut self, handle: ArenaHandle) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        match slot {
            Slot::Live { generation, .. } if *generation == handle.generation => {
                let next_gen = generation.wrapping_add(1);
                let old = std::mem::replace(
                    slot,
                    Slot::Free {
                        generation: next_gen,
                        next_free: self.free_head,
                    },
                );
                self.free_head = Some(handle.index);
                self.live_count = self.live_count.saturating_sub(1);
                match old {
                    Slot::Live { value, .. } => Some(value),
                    Slot::Free { .. } => None,
                }
            }
            _ => None,
        }
    }

    /// True when `get(handle)` would succeed.
    #[must_use]
    pub fn contains(&self, handle: ArenaHandle) -> bool {
        self.get(handle).is_some()
    }
}

#[cfg(test)]
#[path = "arena_test.rs"]
mod tests;
