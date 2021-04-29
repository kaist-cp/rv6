//! List based arena.

use core::convert::TryFrom;
use core::mem;
use core::pin::Pin;

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRef, Handle, HandleRef, Rc};
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::list::{List, ListEntry, ListNode},
    util::pinned_array::IterPinMut,
    util::rc_cell::{RcCell, RefMut},
};

#[pin_project]
#[repr(C)]
pub struct MruEntry<T> {
    #[pin]
    list_entry: ListEntry,
    #[pin]
    data: RcCell<T>,
}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct MruArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [MruEntry<T>; CAPACITY],
    #[pin]
    list: List<MruEntry<T>>,
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

impl<T: 'static + ArenaObject + Unpin, const CAPACITY: usize> Arena
    for Spinlock<MruArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, MruArena<T, CAPACITY>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena| {
            let mut guard = arena.lock();
            let this = guard.get_pin_mut().project();

            let mut empty: Option<*mut RcCell<T>> = None;
            // SAFETY: the whole `MruArena` is protected by a lock.
            for entry in unsafe { this.list.iter_pin_mut_unchecked() } {
                if !entry.data.is_borrowed() {
                    empty = Some(&entry.data as *const _ as *mut _);
                }
                if let Some(r) = entry.data.try_borrow() {
                    if c(&r) {
                        return Some(Rc::new(arena, Handle(arena.0.brand(r))));
                    }
                }
            }

            empty.map(|cell_raw| {
                // SAFETY: `cell` is not referenced or borrowed. Also, it is already pinned.
                let mut cell = unsafe { Pin::new_unchecked(&mut *cell_raw) };
                n(cell.as_mut().get_pin_mut().unwrap().get_mut());
                let handle = Handle(arena.0.brand(cell.borrow()));
                Rc::new(arena, handle)
            })
        })
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<Rc<Self>> {
        ArenaRef::new(self, |arena| {
            let mut guard = arena.lock();
            let this = guard.get_pin_mut().project();

            // SAFETY: the whole `MruArena` is protected by a lock.
            for mut entry in unsafe { this.list.iter_pin_mut_unchecked().rev() } {
                if !entry.data.is_borrowed() {
                    *(entry
                        .as_mut()
                        .project()
                        .data
                        .get_pin_mut()
                        .unwrap()
                        .get_mut()) = f();
                    let handle = Handle(arena.0.brand(entry.data.borrow()));
                    return Some(Rc::new(arena, handle));
                }
            }
            None
        })
    }

    fn dup<'id>(
        self: ArenaRef<'id, &Self>,
        handle: HandleRef<'id, '_, Self::Data>,
    ) -> Handle<'id, Self::Data> {
        let mut _this = self.lock();
        Handle(self.0.brand(handle.0.into_inner().clone()))
    }

    fn dealloc<'id>(self: ArenaRef<'id, &Self>, handle: Handle<'id, Self::Data>) {
        let mut this = self.lock();

        if let Ok(mut rm) = RefMut::<T>::try_from(handle.0.into_inner()) {
            // Finalize the arena object.
            rm.finalize::<Self>(&mut this);

            // Move this entry to the back of the list.
            let ptr = (rm.get_cell() as *const _ as usize - MruEntry::<T>::DATA_OFFSET)
                as *mut MruEntry<T>;
            let entry = unsafe { &*ptr };
            this.list.push_back(entry);
        }
    }

    unsafe fn reacquire_after<'s, 'g: 's, F, R: 's>(guard: &'s mut Self::Guard<'g>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        guard.reacquire_after(f)
    }
}
