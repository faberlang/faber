//! The root arena: one bump allocator owned by the program entry.
//!
//! `main` owns the root arena for the whole program (operator ruling, the
//! hir-c C5 shape): the entry creates it before the incipit runs and releases
//! it after, and every runtime symbol receives it as the hidden first
//! argument, and the entry shim passes it down from the root owner through
//! `faber_clif_incipit`; the arena is reached through what `main` passes,
//! never through a global that functions switch. There is no configuration and no allocator selector. When the
//! later `arena` function regions arrive, a function passes a child arena
//! pointer down instead of the root and nothing else changes.
//!
//! The physical design is the C route's `faber_arena` (`runtime/c/faber.h`):
//! chunks from the system allocator, bump allocation aligned to the chunk
//! alignment, nothing freed while the program runs. Text handles, lists,
//! maps, records, option boxes and every other runtime object live here, so
//! their addresses are stable for the program's life and the release is one
//! sweep at exit.

use radix_runtime_contract::trap::TrapKind;
use std::alloc::{Layout, alloc, dealloc};
#[cfg(test)]
use std::cell::Cell;
use std::cell::UnsafeCell;

/// One arena chunk: a fixed header followed by `capacity` bump bytes.
struct Chunk {
    previous: *mut Chunk,
    capacity: usize,
    used: usize,
}

/// Header size rounded to the chunk alignment, so the bump base keeps the
/// allocation alignment of the chunk itself.
pub(crate) const HEADER: usize = size_of::<Chunk>().next_multiple_of(CHUNK_ALIGN);
pub(crate) const CHUNK_ALIGN: usize = 16;
/// Fresh-chunk capacity (data bytes), the C route's 64 KiB.
pub(crate) const CHUNK_DATA: usize = 1 << 16;

/// End the run when an allocation cannot be satisfied: the C route's trap
/// text and the program-logic exit class.
#[cold]
fn out_of_memory() -> ! {
    crate::exit_trap("out of memory", TrapKind::ProgramLogic)
}

/// The layout of one chunk holding `capacity` data bytes: `None` when the
/// header addition overflows, so an impossible arena request lands in the
/// honest out-of-memory trap instead of a wrapped, undersized allocation.
pub(crate) fn chunk_layout(capacity: usize) -> Option<Layout> {
    Layout::from_size_align(HEADER.checked_add(capacity)?, CHUNK_ALIGN).ok()
}

/// One program-lifetime bump arena. `UnsafeCell` keeps the bump cursor behind
/// a shared reference; Faber programs on this target are single-threaded
/// (device steps are declined), so no locking is needed.
pub struct Arena {
    current: UnsafeCell<*mut Chunk>,
    /// Test-only count of `alloc_raw` calls, so tests can bound how many
    /// fresh runs a sequence of operations allocates.
    #[cfg(test)]
    allocations: Cell<usize>,
}

#[cfg(test)]
thread_local! {
    static TEST_ARENA: Arena = Arena::new();
}

#[cfg(test)]
pub(crate) fn test_arena_ptr() -> *const Arena {
    TEST_ARENA.with(std::ptr::from_ref)
}

impl Arena {
    pub(crate) fn new() -> Self {
        Self {
            current: UnsafeCell::new(std::ptr::null_mut()),
            #[cfg(test)]
            allocations: Cell::new(0),
        }
    }

    /// Test-only: how many `alloc_raw` calls this arena has served. A buffer
    /// growth run is exactly one call, so the count bounds reallocations.
    #[cfg(test)]
    pub(crate) fn allocations(&self) -> usize {
        self.allocations.get()
    }

    /// Bump-allocate `size` bytes at `align`; a new chunk when the current one
    /// cannot serve the request.
    pub(crate) fn alloc_raw(&self, size: usize, align: usize) -> *mut u8 {
        #[cfg(test)]
        {
            self.allocations.set(self.allocations.get() + 1);
        }
        debug_assert!(
            align.is_power_of_two(),
            "arena alignment must be a power of two"
        );
        // SAFETY: single-threaded runtime; the cursor is only touched here.
        let current = unsafe { *self.current.get() };
        if let Some(found) = unsafe { bump(current, size, align) } {
            return found;
        }
        let wanted = size.checked_add(align).unwrap_or_else(|| out_of_memory());
        let capacity = CHUNK_DATA.max(wanted);
        let layout = chunk_layout(capacity).unwrap_or_else(|| out_of_memory());
        // SAFETY: `layout` has non-zero size. The cast is aligned: the
        // allocation is `CHUNK_ALIGN`, above `align_of::<Chunk>()`.
        #[allow(clippy::cast_ptr_alignment)]
        let chunk = unsafe { alloc(layout) }.cast::<Chunk>();
        if chunk.is_null() {
            out_of_memory();
        }
        // SAFETY: `chunk` addresses one fresh `HEADER + capacity` layout.
        unsafe {
            (*chunk).previous = current;
            (*chunk).capacity = capacity;
            (*chunk).used = 0;
            *self.current.get() = chunk;
        }
        // The fresh chunk always serves the request: its base is chunk
        // aligned and its data capacity covers `size + align` of padding.
        unsafe { bump(chunk, size, align) }.unwrap_or_else(|| out_of_memory())
    }

    /// Store one value and return its stable address. The value is never
    /// dropped by the arena: runtime objects live until the arena is released.
    pub(crate) fn put<T>(&self, value: T) -> *mut T {
        let slot = self.alloc_raw(size_of::<T>(), align_of::<T>()).cast::<T>();
        // SAFETY: `slot` is freshly allocated for one `T`.
        unsafe { slot.write(value) };
        slot
    }

    /// Store one `Copy` slice and return its stable address as a fat pointer.
    pub(crate) fn put_slice<T: Copy>(&self, items: &[T]) -> *mut [T] {
        if items.is_empty() {
            return std::ptr::slice_from_raw_parts_mut(
                std::ptr::NonNull::<T>::dangling().as_ptr(),
                0,
            );
        }
        let bytes = size_of::<T>()
            .checked_mul(items.len())
            .unwrap_or_else(|| out_of_memory());
        let slot = self.alloc_raw(bytes, align_of::<T>()).cast::<T>();
        // SAFETY: `slot` covers `items.len()` elements and does not alias.
        unsafe { std::ptr::copy_nonoverlapping(items.as_ptr(), slot, items.len()) };
        std::ptr::slice_from_raw_parts_mut(slot, items.len())
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: single-threaded runtime; dropping walks the chunk chain and
        // releases each chunk with the layout it was allocated under.
        let mut chunk = unsafe { *self.current.get() };
        while let Some(found) = unsafe { chunk.as_mut() } {
            let previous = found.previous;
            let capacity = found.capacity;
            unsafe {
                dealloc(
                    chunk.cast(),
                    Layout::from_size_align_unchecked(HEADER + capacity, CHUNK_ALIGN),
                );
            };
            chunk = previous;
        }
    }
}

/// Serve one request from `chunk` when its free tail fits, advancing the
/// bump cursor. `None` sends the caller hunting for a new chunk.
///
/// # Safety
///
/// `chunk` must be null or a live chunk of this arena.
unsafe fn bump(chunk: *mut Chunk, size: usize, align: usize) -> Option<*mut u8> {
    let found = (unsafe { chunk.as_mut() })?;
    let base = chunk as usize + HEADER;
    let aligned = base
        .checked_add(found.used)
        .and_then(|cursor| cursor.checked_next_multiple_of(align))?;
    let end = aligned.checked_add(size)?;
    if end > base + found.capacity {
        return None;
    }
    found.used = end - base;
    Some(aligned as *mut u8)
}

/// A growable element buffer backed by the arena: the list, map and record
/// storage. Growth allocates a fresh run of the next capacity and copies, and
/// the abandoned run stays in the arena until release — the C route's list
/// growth (`runtime/c/faber.h`) with the same never-free law.
///
/// The element type is `Copy` because the runtime's buffers hold only scalar
/// and pointer cells; nothing here runs destructors.
pub(crate) struct ArenaBuf<T: Copy> {
    items: *mut T,
    length: usize,
    capacity: usize,
}

impl<T: Copy> ArenaBuf<T> {
    /// The empty buffer; the first push allocates.
    pub(crate) fn new() -> Self {
        Self {
            items: std::ptr::NonNull::<T>::dangling().as_ptr(),
            length: 0,
            capacity: 0,
        }
    }

    /// A buffer of `count` copies of `value`.
    pub(crate) fn filled(arena: *const Arena, count: usize, value: T) -> Self {
        let mut buffer = Self::with_capacity(arena, count);
        // SAFETY: `buffer.items` covers `count` initialized elements after the
        // fill.
        unsafe {
            for slot in 0..count {
                buffer.items.add(slot).write(value);
            }
        }
        buffer.length = count;
        buffer
    }

    /// A buffer holding a copy of `items`, sized exactly.
    pub(crate) fn from_slice(arena: *const Arena, items: &[T]) -> Self {
        let mut buffer = Self::with_capacity(arena, items.len());
        // SAFETY: `buffer.items` covers `items.len()` elements.
        unsafe {
            std::ptr::copy_nonoverlapping(items.as_ptr(), buffer.items, items.len());
        }
        buffer.length = items.len();
        buffer
    }

    /// An uninitialized-capacity buffer; `length` is zero.
    pub(crate) fn with_capacity(arena: *const Arena, capacity: usize) -> Self {
        if capacity == 0 {
            return Self::new();
        }
        let bytes = size_of::<T>()
            .checked_mul(capacity)
            .unwrap_or_else(|| out_of_memory());
        let items = arena_ref(arena)
            .alloc_raw(bytes, align_of::<T>())
            .cast::<T>();
        Self {
            items,
            length: 0,
            capacity,
        }
    }

    /// The live elements.
    pub(crate) fn as_slice(&self) -> &[T] {
        // SAFETY: `items` covers `length` initialized elements.
        unsafe { std::slice::from_raw_parts(self.items, self.length) }
    }

    /// The live elements, mutably.
    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: `items` covers `length` initialized elements and no other
        // reference is live.
        unsafe { std::slice::from_raw_parts_mut(self.items, self.length) }
    }

    pub(crate) fn len(&self) -> usize {
        self.length
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// The element capacity of the current run.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn clear(&mut self) {
        self.length = 0;
    }

    /// Element at `index`.
    pub(crate) fn get(&self, index: usize) -> Option<T> {
        self.as_slice().get(index).copied()
    }

    /// Overwrite the element at `index`.
    pub(crate) fn set(&mut self, index: usize, value: T) {
        self.as_mut_slice()[index] = value;
    }

    /// Append one element, growing by the C route's doubling (4 first).
    pub(crate) fn push(&mut self, arena: *const Arena, value: T) {
        if self.length == self.capacity {
            self.grow(arena);
        }
        // SAFETY: `items` covers `length + 1` elements after the growth.
        unsafe { self.items.add(self.length).write(value) };
        self.length += 1;
    }

    /// Remove and return the last element.
    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.length == 0 {
            return None;
        }
        self.length -= 1;
        // SAFETY: `length` was a live element and stays within the buffer.
        Some(unsafe { self.items.add(self.length).read() })
    }

    /// Remove and return the element at `index`, shifting the tail left.
    pub(crate) fn remove(&mut self, index: usize) -> T {
        let slice = self.as_mut_slice();
        let value = slice[index];
        slice.copy_within(index + 1.., index);
        self.length -= 1;
        value
    }

    /// Resize: shorter truncates, longer fills with `value`.
    pub(crate) fn resize(&mut self, arena: *const Arena, length: usize, value: T) {
        if length <= self.length {
            self.length = length;
            return;
        }
        if length > self.capacity {
            self.reserve(arena, length);
        }
        // SAFETY: `items` covers `length` elements; the tail is filled here.
        unsafe {
            for slot in self.length..length {
                self.items.add(slot).write(value);
            }
        }
        self.length = length;
    }

    /// Append every element of `items`.
    pub(crate) fn extend_from_slice(&mut self, arena: *const Arena, items: &[T]) {
        if items.is_empty() {
            return;
        }
        let wanted = self
            .length
            .checked_add(items.len())
            .unwrap_or_else(|| out_of_memory());
        if wanted > self.capacity {
            self.reserve(arena, wanted);
        }
        // SAFETY: the buffer covers `wanted` elements after the reserve.
        unsafe {
            std::ptr::copy_nonoverlapping(items.as_ptr(), self.items.add(self.length), items.len());
        }
        self.length = wanted;
    }

    /// Reverse the elements in place.
    pub(crate) fn reverse(&mut self) {
        self.as_mut_slice().reverse();
    }

    /// The next capacity: the C route's `faber_collection_next_capacity`.
    fn grow(&mut self, arena: *const Arena) {
        let next = match self.capacity {
            0 => 4,
            capacity if capacity > usize::MAX / 2 => out_of_memory(),
            capacity => capacity * 2,
        };
        self.reserve_exact(arena, next);
    }

    /// Ensure room for `wanted` elements: a fresh run of at least `wanted`,
    /// and of twice the current capacity when that is larger, so repeated
    /// small grows need only logarithmically many runs and copy each live
    /// element a bounded number of times. Overflowed doubling falls back to
    /// the exact demand; the run itself is still checked when allocated.
    fn reserve(&mut self, arena: *const Arena, wanted: usize) {
        let doubled = match self.capacity.checked_mul(2) {
            Some(doubled) if doubled > wanted => doubled,
            _ => wanted,
        };
        self.reserve_exact(arena, doubled);
    }

    /// Move the elements into a fresh run of at least `capacity`.
    fn reserve_exact(&mut self, arena: *const Arena, capacity: usize) {
        let mut fresh = Self::with_capacity(arena, capacity);
        // SAFETY: both runs cover `length` live elements and do not alias.
        unsafe {
            std::ptr::copy_nonoverlapping(self.items, fresh.items, self.length);
        }
        fresh.length = self.length;
        *self = fresh;
    }
}

fn arena_ref(arena: *const Arena) -> &'static Arena {
    // SAFETY: all arena pointers are supplied by the owning program entry and
    // remain live until that entry returns.
    unsafe { arena.as_ref() }.expect("clif runtime: null arena pointer")
}

/// Pointer-shaped counterpart of the arena's safe Rust methods for ABI
/// entrypoints, whose hidden context travels as an opaque C pointer.
pub(crate) trait ArenaPointer {
    fn put<T>(self, value: T) -> *mut T;
    fn put_slice<T: Copy>(self, value: &[T]) -> *mut [T];
}

impl ArenaPointer for *const Arena {
    fn put<T>(self, value: T) -> *mut T {
        arena_ref(self).put(value)
    }

    fn put_slice<T: Copy>(self, value: &[T]) -> *mut [T] {
        arena_ref(self).put_slice(value)
    }
}

impl<T: Copy + PartialEq> PartialEq for ArenaBuf<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + PartialEq> PartialEq<Vec<T>> for ArenaBuf<T> {
    fn eq(&self, other: &Vec<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + std::fmt::Debug> std::fmt::Debug for ArenaBuf<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_slice().fmt(formatter)
    }
}

impl<T: Copy> std::ops::Deref for ArenaBuf<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: Copy> std::ops::DerefMut for ArenaBuf<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<'a, T: Copy> IntoIterator for &'a ArenaBuf<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T: Copy> IntoIterator for &'a mut ArenaBuf<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}
