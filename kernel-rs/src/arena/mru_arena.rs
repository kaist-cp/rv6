//! List based arena.

use core::mem;
use core::pin::Pin;
use core::ptr::NonNull;

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRc, ArenaRef, Handle};
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::intrusive_list::{List, ListEntry, ListNode},
    util::pinned_array::IterPinMut,
    util::{shared_mut::SharedMut, static_arc::StaticArc},
};

#[pin_project]
#[repr(C)]
pub struct MruEntry<T> {
    #[pin]
    list_entry: ListEntry,
    #[pin]
    data: StaticArc<T>,
}

/// A homogeneous memory allocator equipped with reference counts.
#[pin_project]
pub struct MruArena<T, const CAPACITY: usize> {
    #[pin]
    entries: [MruEntry<T>; CAPACITY],
    #[pin]
    list: List<MruEntry<T>>,
}

// SAFETY: `MruArena` never exposes its internal lists and entries.
unsafe impl<T: Send, const CAPACITY: usize> Send for MruArena<T, CAPACITY> {}

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
            data: StaticArc::new(data),
        }
    }

    fn data(this: SharedMut<'_, Self>) -> SharedMut<'_, StaticArc<T>> {
        // SAFETY: the pointer is valid, and it creates a unique `SharedMut`.
        unsafe { SharedMut::new_unchecked(&raw mut (*this.ptr().as_ptr()).data) }
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

    fn list(this: SharedMut<'_, Self>) -> SharedMut<'_, List<MruEntry<T>>> {
        // SAFETY: the pointer is valid, and it creates a unique `SharedMut`.
        unsafe { SharedMut::new_unchecked(&raw mut (*this.ptr().as_ptr()).list) }
    }
}

impl<T: 'static + ArenaObject + Unpin + Send, const CAPACITY: usize> Arena
    for Spinlock<MruArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, MruArena<T, CAPACITY>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        &self,
        c: C,
        n: N,
    ) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, &Spinlock<MruArena<T, CAPACITY>>>| {
                let mut guard = arena.pinned_lock_unchecked();
                let this = guard.get_shared_mut();

                let mut empty: Option<NonNull<StaticArc<T>>> = None;
                for entry in List::iter_shared_mut(MruArena::list(this)) {
                    let mut entry = MruEntry::data(entry);

                    if let Some(entry) = StaticArc::try_borrow(entry.as_shared_mut()) {
                        // The entry is not under finalization. Check its data.
                        if c(&entry) {
                            let handle = Handle(arena.0.brand(entry));
                            return Some(ArenaRc::new(arena, handle));
                        }
                    }

                    if !StaticArc::is_borrowed(entry.as_shared_mut()) {
                        let _ = empty.get_or_insert(entry.ptr());
                    }
                }

                empty.map(|ptr| {
                    // SAFETY: `ptr` is valid, and there's no `SharedMut`.
                    let mut entry = unsafe { SharedMut::new_unchecked(ptr.as_ptr()) };
                    n(StaticArc::get_mut(entry.as_shared_mut()).unwrap());
                    let handle = Handle(arena.0.brand(StaticArc::borrow(entry)));
                    ArenaRc::new(arena, handle)
                })
            },
        )
    }

    fn alloc<F: FnOnce() -> Self::Data>(&self, f: F) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, &Spinlock<MruArena<T, CAPACITY>>>| {
                let mut guard = arena.pinned_lock_unchecked();
                let this = guard.get_shared_mut();

                for entry in List::iter_shared_mut(MruArena::list(this)).rev() {
                    let mut entry = MruEntry::data(entry);
                    if let Some(data) = StaticArc::get_mut(entry.as_shared_mut()) {
                        *data = f();
                        let handle = Handle(arena.0.brand(StaticArc::borrow(entry)));
                        return Some(ArenaRc::new(arena, handle));
                    }
                }
                None
            },
        )
    }

    fn dealloc<'id, 'a, 'b>(
        self: ArenaRef<'id, &Self>,
        handle: Handle<'id, Self::Data>,
        ctx: <Self::Data as ArenaObject>::Ctx<'a, 'b>,
    ) {
        if let Ok(mut rm) = handle.0.into_inner().into_mut() {
            // Finalize the arena object.
            rm.finalize::<Self>(ctx);

            // Move this entry to the back of the list.
            let this = self.pinned_lock_unchecked();
            let ptr = (rm.cell() as usize - MruEntry::<T>::DATA_OFFSET) as *mut _;
            // SAFETY:
            // * `rm.cell()` is an `RcCell` inside an `MruEntry`.
            // * The value of `DATA_OFFSET` is proper.
            this.list.push_back(unsafe { &*ptr });
        }
    }
}
