use core::mem::{self, MaybeUninit};
use core::ops::{Deref, DerefMut};
use core::ptr;

struct RcEntry<T> {
    ref_cnt: usize,
    data: MaybeUninit<T>,
}

/// A homogeneous memory allocator equipped with reference counts.
pub struct RcArena<T, const CAPACITY: usize> {
    inner: [RcEntry<T>; CAPACITY],
}

pub struct Rc<T> {
    ptr: *mut RcEntry<T>,
}

impl<T, const CAPACITY: usize> RcArena<T, CAPACITY> {
    pub const fn new() -> Self {
        Self {
            inner: [RcEntry {
                ref_cnt: 0,
                data: MaybeUninit::uninit(),
            }; CAPACITY],
        }
    }

    /// # Safety
    ///
    /// `rc` must be allocated from `self`.
    // TODO: Make a RcArena trait and move this there.
    pub unsafe fn dup(&mut self, rc: &Rc<T>) -> Rc<T> {
        (*rc.ptr).ref_cnt += 1;
        Rc { ptr: rc.ptr }
    }
}

impl<T> Rc<T> {
    pub fn into_raw(self) -> *mut T {
        let result = unsafe { (*self.ptr).data.as_mut_ptr() };
        mem::forget(self);
        result
    }
}

impl<T> Deref for Rc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { (*self.ptr).data.assume_init_ref() }
    }
}

// TODO: This may cause UB; remove after refactoring File::{read, write}.
impl<T> DerefMut for Rc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { (*self.ptr).data.assume_init_mut() }
    }
}

impl<T> Drop for Rc<T> {
    fn drop(&mut self) {
        // HACK(@efenniht): we really need linear type here:
        // https://github.com/rust-lang/rfcs/issues/814
        panic!("Rc must never drop: use RcArena::dealloc instead.");
    }
}

/// A homogeneous memory allocator, equipped with the box type representing an allocation.
pub trait Arena {
    /// The value type of the allocator.
    type Data: 'static;

    /// The box type of the allocator.
    type Handle: 'static;

    /// Find or alloc.
    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(*mut Self::Data)>(
        &mut self,
        c: C,
        n: N,
    ) -> Option<Self::Handle>;

    /// Failable allocation.
    fn alloc<F: FnOnce(*mut Self::Data)>(&mut self, f: F) -> Option<Self::Handle>;

    /// # Safety
    ///
    /// `pbox` must be allocated from the pool.
    unsafe fn dealloc(&mut self, pbox: Self::Handle) -> Option<Self::Data>;
}

impl<T: 'static, const CAPACITY: usize> Arena for RcArena<T, CAPACITY> {
    type Data = T;
    type Handle = Rc<T>;

    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(*mut Self::Data)>(
        &mut self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let mut empty: *mut RcEntry<T> = ptr::null_mut();

        for entry in self.inner.iter_mut() {
            if entry.ref_cnt != 0 {
                if c(unsafe { &*entry.data.as_ptr() }) {
                    entry.ref_cnt += 1;
                    return Some(Rc { ptr: entry });
                }
            } else if empty.is_null() {
                empty = entry;
            }
        }

        if empty.is_null() {
            return None;
        }

        let entry = unsafe { &mut *empty };
        entry.ref_cnt = 1;
        n(entry.data.as_mut_ptr());
        Some(Rc { ptr: entry })
    }

    fn alloc<F: FnOnce(*mut T)>(&mut self, f: F) -> Option<Rc<T>> {
        for entry in self.inner.iter_mut() {
            if entry.ref_cnt == 0 {
                entry.ref_cnt = 1;
                f(entry.data.as_mut_ptr());
                return Some(Rc { ptr: entry });
            }
        }

        None
    }

    /// # Safety
    ///
    /// `rc` must be allocated from `self`.
    unsafe fn dealloc(&mut self, rc: Rc<T>) -> Option<T> {
        let entry = &mut *rc.ptr;
        entry.ref_cnt -= 1;

        let val = if entry.ref_cnt == 0 {
            Some(entry.data.read())
        } else {
            None
        };

        mem::forget(rc);
        val
    }
}

pub trait Tag: Clone + 'static {
    type Target: Arena;
    type Result<'s>: DerefMut<Target = Self::Target>;

    fn arena(&self) -> Self::Result<'_>;
}

pub struct TaggedRc<T: Tag> {
    tag: T,
    inner: mem::MaybeUninit<<<T as Tag>::Target as Arena>::Handle>,
}

impl<A: 'static, T: Tag<Target = RcArena<A, C>>, const C: usize> Deref for TaggedRc<T> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        unsafe { (*self.inner.as_ptr()).deref() }
    }
}

impl<A: 'static, T: Tag<Target = RcArena<A, C>>, const C: usize> DerefMut for TaggedRc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { (*self.inner.as_mut_ptr()).deref_mut() }
    }
}

impl<T: Tag> Drop for TaggedRc<T> {
    fn drop(&mut self) {
        // SAFETY: We can ensure the box is allocated from `P` by the invariant of ArenaRef2.
        unsafe {
            self.tag.arena().dealloc(self.inner.read());
        }
    }
}

impl<T: Tag> TaggedRc<T> {
    pub unsafe fn from_unchecked(tag: T, inner: <<T as Tag>::Target as Arena>::Handle) -> Self {
        Self {
            tag,
            inner: mem::MaybeUninit::new(inner),
        }
    }
}

impl<A: 'static, T: Tag<Target = RcArena<A, C>>, const C: usize> Clone for TaggedRc<T> {
    fn clone(&self) -> Self {
        let inner = mem::MaybeUninit::new(unsafe { self.tag.arena().dup(&*self.inner.as_ptr()) });
        Self {
            tag: self.tag.clone(),
            inner,
        }
    }
}

impl<A: Arena + 'static, T: Tag<Target = A>> Arena for T {
    type Data = A::Data;
    type Handle = TaggedRc<T>;

    /// Find or alloc.
    fn find_or_alloc<C: Fn(&Self::Data) -> bool, N: FnOnce(*mut Self::Data)>(
        &mut self,
        c: C,
        n: N,
    ) -> Option<Self::Handle> {
        let inner = mem::MaybeUninit::new(self.arena().find_or_alloc(c, n)?);
        Some(TaggedRc {
            tag: self.clone(),
            inner,
        })
    }

    /// Failable allocation.
    fn alloc<F: FnOnce(*mut Self::Data)>(&mut self, f: F) -> Option<Self::Handle> {
        let inner = mem::MaybeUninit::new(self.arena().alloc(f)?);
        Some(TaggedRc {
            tag: self.clone(),
            inner,
        })
    }

    /// # Safety
    ///
    /// `pbox` must be allocated from the pool.
    unsafe fn dealloc(&mut self, pbox: Self::Handle) -> Option<Self::Data> {
        self.arena().dealloc(pbox.inner.read())
    }
}
