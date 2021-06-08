//! List based arena.

use core::mem;
use core::pin::Pin;
use core::ptr::NonNull;

use array_macro::array;
use pin_project::pin_project;

use super::{Arena, ArenaObject, ArenaRc, ArenaRef, Handle};
use crate::util::strong_pin::StrongPin;
use crate::{
    lock::{Spinlock, SpinlockGuard},
    util::intrusive_list::{List, ListEntry, ListNode},
    util::pinned_array::IterPinMut,
    util::{static_arc::StaticArc, strong_pin::StrongPinMut},
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

    #[allow(clippy::needless_lifetimes)]
    fn data<'s>(self: StrongPinMut<'s, Self>) -> StrongPinMut<'s, StaticArc<T>> {
        // SAFETY: the pointer is valid, and it creates a unique `StrongPinMut`.
        unsafe { StrongPinMut::new_unchecked(&raw mut (*self.ptr().as_ptr()).data) }
    }
}

// SAFETY: `MruEntry` owns a `ListEntry`.
unsafe impl<T> ListNode for MruEntry<T> {
    fn get_list_entry(self: Pin<&Self>) -> Pin<&ListEntry> {
        unsafe { Pin::new_unchecked(&self.get_ref().list_entry) }
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
    ///
    /// # Safety
    ///
    /// Must be used only after initializing it with `MruArena::init`.
    // Note: We cannot use the generic `T` in the following function, since we need to only allow
    // types that `impl const Default`, not just `impl Default`.
    #[allow(clippy::new_ret_no_self)]
    pub const unsafe fn new<D: Default>() -> MruArena<D, CAPACITY> {
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
            this.list.as_ref().push_front(entry.as_ref());
        }
    }

    #[allow(clippy::needless_lifetimes)]
    fn list<'s>(self: StrongPinMut<'s, Self>) -> StrongPinMut<'s, List<MruEntry<T>>> {
        // SAFETY: the pointer is valid, and it creates a unique `StrongPinMut`.
        unsafe { StrongPinMut::new_unchecked(&raw mut (*self.ptr().as_ptr()).list) }
    }
}

impl<T: 'static + ArenaObject + Unpin + Send, const CAPACITY: usize> Arena
    for Spinlock<MruArena<T, CAPACITY>>
{
    type Data = T;
    type Guard<'s> = SpinlockGuard<'s, MruArena<T, CAPACITY>>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(&mut Self::Data)>(
        self: StrongPin<'_, Self>,
        c: C,
        n: N,
    ) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, '_, Spinlock<MruArena<T, CAPACITY>>>| {
                let mut guard = arena.strong_pinned_lock();
                let this = guard.get_strong_pinned_mut();

                let mut empty: Option<NonNull<StaticArc<T>>> = None;
                for entry in this.list().iter_shared_mut() {
                    let mut entry = entry.data();

                    if let Some(entry) = entry.as_mut().try_borrow() {
                        // The entry is not under finalization. Check its data.
                        if c(&entry) {
                            let handle = Handle(arena.0.brand(entry));
                            return Some(ArenaRc::new(arena, handle));
                        }
                    }

                    if !entry.as_mut().is_borrowed() {
                        let _ = empty.get_or_insert(entry.ptr());
                    }
                }

                empty.map(|ptr| {
                    // SAFETY: `ptr` is valid, and there's no `StrongPinMut`.
                    let mut entry = unsafe { StrongPinMut::new_unchecked(ptr.as_ptr()) };
                    n(entry.as_mut().get_mut().unwrap());
                    let handle = Handle(arena.0.brand(entry.borrow()));
                    ArenaRc::new(arena, handle)
                })
            },
        )
    }

    fn alloc<F: FnOnce() -> Self::Data>(self: StrongPin<'_, Self>, f: F) -> Option<ArenaRc<Self>> {
        ArenaRef::new(
            self,
            |arena: ArenaRef<'_, '_, Spinlock<MruArena<T, CAPACITY>>>| {
                let mut guard = arena.strong_pinned_lock();
                let this = guard.get_strong_pinned_mut();

                for entry in this.list().iter_shared_mut().rev() {
                    let mut entry = entry.data();
                    if let Some(data) = entry.as_mut().get_mut() {
                        *data = f();
                        let handle = Handle(arena.0.brand(entry.borrow()));
                        return Some(ArenaRc::new(arena, handle));
                    }
                }
                None
            },
        )
    }

    fn dealloc<'id, 'a, 'b>(
        self: ArenaRef<'id, '_, Self>,
        handle: Handle<'id, Self::Data>,
        ctx: <Self::Data as ArenaObject>::Ctx<'a, 'b>,
    ) {
        if let Ok(mut rm) = handle.0.into_inner().into_mut() {
            // Finalize the arena object.
            rm.finalize::<Self>(ctx);

            // Move this entry to the back of the list.
            let ptr: *const MruEntry<Self::Data> =
                (rm.cell() as usize - MruEntry::<T>::DATA_OFFSET) as _;
            // SAFETY:
            // * `rm.cell()` is an `RcCell` inside an `MruEntry`.
            // * The value of `DATA_OFFSET` is proper.
            let ptr = unsafe { Pin::new_unchecked(&*ptr) };

            let mut this = self.strong_pinned_lock();
            let this = this.get_strong_pinned_mut().as_ref().as_pin().get_ref();
            unsafe { Pin::new_unchecked(&this.list) }.push_back(ptr);
        }
    }
}
