//! List based arena.
use core::pin::Pin;
use core::{mem, ops::Deref};

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaGuard, ArenaObject, ArenaRef, Entry, EntryRef, Handle, Rc};
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::pinned_array::IterPinMut,
    util::{
        branded::Branded,
        intrusive_list::{Iter, List, ListEntry, ListNode},
    },
};

unsafe impl<T: Send, const CAPACITY: usize> Sync for MruArena<T, CAPACITY> {}

#[pin_project]
#[repr(C)]
pub struct MruEntry<T> {
    #[pin]
    list_entry: ListEntry,
    data: Entry<T>,
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
            data: Entry::new(data),
        }
    }
}

impl<'id, T, const CAPACITY: usize> ArenaRef<'id, &MruArena<T, CAPACITY>> {
    fn lock(&self) -> ArenaGuard<'id, '_> {
        ArenaGuard(self.0.brand(self.lock.lock()))
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

impl<'id, 's, T: 's> Iterator for EntryIter<'id, 's, T> {
    type Item = EntryRef<'id, 's, T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|inner| EntryRef(self.0.brand(&inner.data)))
    }
}

impl<'id, 's, T: 's> DoubleEndedIterator for EntryIter<'id, 's, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|inner| EntryRef(self.0.brand(&inner.data)))
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
            // If empty is Some(v), v.rc is 0.
            let mut empty: Option<Handle<T>> = None;
            // SAFETY: `self.list` is not modified during iteration.
            for entry in unsafe { arena.entries() } {
                let rc = entry.get_mut_rc(&mut guard);
                if c(&entry) {
                    *rc += 1;
                    return Some(Rc::new(self, entry.into_handle()));
                }
                if *rc == 0 {
                    empty = Some(entry.into_handle());
                }
            }
            empty.map(|handle| {
                // SAFETY: handle.rc is 0.
                n(unsafe { &mut *handle.data_raw() });
                let entry = EntryRef(arena.0.brand(&handle));
                *entry.get_mut_rc(&mut guard) += 1;
                Rc::new(self, handle)
            })
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena: ArenaRef<'_, &MruArena<T, CAPACITY>>| {
            let mut guard = arena.lock();
            // SAFETY: `self.list` is not modified during iteration.
            for entry in unsafe { arena.entries() }.rev() {
                let rc = entry.get_mut_rc(&mut guard);
                if *rc == 0 {
                    // SAFETY: entry.rc is 0.
                    unsafe { *entry.data_raw() = f() };
                    *rc += 1;
                    return Some(Rc::new(self, entry.into_handle()));
                }
            }
            None
        })
    }

    fn dup<'id>(self: ArenaRef<'id, &Self>, handle: &EntryRef<'id, '_, Self::Data>) {
        let mut guard = self.lock();
        let rc = handle.get_mut_rc(&mut guard);
        *rc += 1;
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: EntryRef<'id, '_, Self::Data>) {
        let mut guard = self.lock();
        let rc = handle.get_mut_rc(&mut guard);
        if *rc == 1 {
            // SAFETY: handle.rc will become 0.
            unsafe { (*handle.data_raw()).finalize::<Self>(&mut guard.0) };

            // Move this entry to the back of the list.
            let ptr = (handle.deref() as *const Entry<T> as usize - MruEntry::<T>::DATA_OFFSET)
                as *mut MruEntry<T>;
            // SAFETY: `handle` is an `Entry` inside an `MruEntry`.
            let entry = unsafe { &*ptr };
            self.list.push_back(entry);
        }
        // To prevent `find_or_alloc` and `alloc` from mutating `handle` while finalizing `handle`,
        // `rc` should be decreased after the finalization.
        let rc = handle.get_mut_rc(&mut guard);
        *rc -= 1;
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}
