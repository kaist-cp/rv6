//! List based arena.
use core::pin::Pin;
use core::ptr::NonNull;
use core::{mem, ops::Deref};

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRef, Rc};
use crate::util::shared_rc_cell::BrandedRcCell;
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::pinned_array::IterPinMut,
    util::{
        branded::Branded,
        intrusive_list::{Iter, List, ListEntry, ListNode},
        shared_rc_cell::{BrandedRef, RcCell, SharedGuard},
    },
};

unsafe impl<T: Send, const CAPACITY: usize> Sync for MruArena<T, CAPACITY> {}

#[pin_project]
#[repr(C)]
pub struct MruEntry<T> {
    #[pin]
    list_entry: ListEntry,
    data: RcCell<T>,
}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct MruArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [MruEntry<T>; CAPACITY],
    #[pin]
    list: List<MruEntry<T>>,
    lock: Spinlock<()>,
}

impl<T> MruEntry<T> {
    // TODO(https://github.com/kaist-cp/rv6/issues/369)
    // A workarond for https://github.com/Gilnaa/memoffset/issues/49.
    // Assumes `list_entry` is located at the beginning of `MruEntry`
    // and `data` is located at `mem::size_of::<ListEntry>()`.
    const DATA_OFFSET: usize = mem::size_of::<ListEntry>();
    const LIST_ENTRY_OFFSET: usize = 0;

    // const DATA_OFFSET: usize = offset_of!(MruEntry<T>, data);
    // const LIST_ENTRY_OFFSET: usize = offset_of!(MruEntry<T>, list_entry);

    pub const fn new(data: T) -> Self {
        Self {
            list_entry: unsafe { ListEntry::new() },
            data: RcCell::new(data),
        }
    }
}

impl<'id, T, const CAPACITY: usize> ArenaRef<'id, &MruArena<T, CAPACITY>> {
    fn lock(&self) -> SharedGuard<'id, '_> {
        unsafe { SharedGuard::new_unchecked(self.0.brand(self.lock.lock())) }
    }

    /// # Safety
    ///
    /// `self.list` must not be modified during iteration.
    unsafe fn entries(&self) -> EntryIter<'id, '_, T> {
        EntryIter(self.0.brand(unsafe { self.list.iter_unchecked() }))
    }
}

#[repr(transparent)]
struct EntryIter<'id, 's, T>(Branded<'id, Iter<'s, MruEntry<T>>>);

impl<'id: 's, 's, T: 's> Iterator for EntryIter<'id, 's, T> {
    type Item = &'s BrandedRcCell<'id, T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|inner| unsafe { &*(&inner.data as *const _ as *const _) })
    }
}

impl<'id: 's, 's, T: 's> DoubleEndedIterator for EntryIter<'id, 's, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|inner| unsafe { &*(&inner.data as *const _ as *const _) })
    }
}

// SAFETY: `MruEntry` owns a `ListEntry`.
unsafe impl<T> ListNode for MruEntry<T> {
    fn get_list_entry(&self) -> &ListEntry {
        &self.list_entry
    }

    fn from_list_entry(list_entry: *const ListEntry) -> *const Self {
        (list_entry as *const _ as usize - Self::LIST_ENTRY_OFFSET) as *const Self
    }
}

impl<T, const CAPACITY: usize> MruArena<T, CAPACITY> {
    /// Returns an `MruArena` of size `CAPACITY` that is filled with `D`'s const default value.
    /// Note that `D` must `impl const Default`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let mru_arena = MruArena::<D, 100>::new();
    /// ```
    // Note: We cannot use the generic `T` in the following function, since we need to only allow
    // types that `impl const Default`, not just `impl Default`.
    #[allow(clippy::new_ret_no_self)]
    pub const fn new<D: Default>() -> MruArena<D, CAPACITY> {
        MruArena {
            entries: array![_ => MruEntry::new(Default::default()); CAPACITY],
            list: unsafe { List::new() },
            lock: Spinlock::new("MruArena", ()),
        }
    }

    pub fn init(self: Pin<&mut Self>) {
        let mut this = self.project();
        this.list.as_mut().init();
        for mut entry in IterPinMut::from(this.entries) {
            entry.as_mut().project().list_entry.init();
            this.list.push_front(&entry);
        }
    }
}

impl<T: 'static + ArenaObject + Unpin + Send, const CAPACITY: usize> Arena
    for MruArena<T, CAPACITY>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, ()>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena: ArenaRef<'_, &MruArena<T, CAPACITY>>| {
            let mut guard = arena.lock();
            let mut empty = None;
            // SAFETY: `self.list` is not modified during iteration.
            for cell in unsafe { arena.entries() } {
                if let Some(data) = cell.get_data(&mut guard) {
                    if c(data) {
                        return Some(Rc::new(self, cell.make_ref(&mut guard).into_ref()));
                    }
                }
                let rc = cell.get_rc_mut(&mut guard);
                if *rc == 0 {
                    let _ = empty.get_or_insert(NonNull::from(cell));
                }
            }
            empty.map(|cell| {
                let cell = unsafe { cell.as_ref() };
                n(unsafe { cell.get_data_mut_unchecked(&mut guard) });
                Rc::new(self, cell.make_ref(&mut guard).into_ref())
            })
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena: ArenaRef<'_, &MruArena<T, CAPACITY>>| {
            let mut guard = arena.lock();
            // SAFETY: `self.list` is not modified during iteration.
            for cell in unsafe { arena.entries() }.rev() {
                if let Some(data) = cell.get_data_mut(&mut guard) {
                    *data = f();
                    return Some(Rc::new(self, cell.make_ref(&mut guard).into_ref()));
                }
            }
            None
        })
    }

    fn dup<'id>(self: ArenaRef<'id, &Self>, handle: &BrandedRef<'id, Self::Data>) -> Rc<Self> {
        let mut guard = self.lock();
        Rc::new(self.deref(), handle.clone(&mut guard).into_ref())
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: BrandedRef<'id, Self::Data>) {
        let mut guard = self.lock();
        let handle = match handle.into_mut(&mut guard) {
            Ok(mut data) => {
                data.get_data_mut().finalize::<Self>(guard.inner_mut());

                let handle = data.into_ref(&mut guard);
                // Move this entry to the back of the list.
                let ptr = (handle.get_cell() as *const _ as usize - MruEntry::<T>::DATA_OFFSET)
                    as *mut MruEntry<T>;
                // SAFETY: `handle` is an `Entry` inside an `MruEntry`.
                let entry = unsafe { &*ptr };
                self.list.push_back(entry);
                handle
            }
            Err(handle) => handle,
        };
        // To prevent `find_or_alloc` and `alloc` from mutating `handle` while finalizing `handle`,
        // `rc` should be decreased after the finalization.
        handle.free(&mut guard);
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}
