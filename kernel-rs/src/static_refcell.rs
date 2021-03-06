use core::cell::{Cell, UnsafeCell};
use core::marker::PhantomPinned;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

const BORROWED_MUT: usize = usize::MAX;

/// Similar to `RefCell<T>`, but does not use lifetimes.
struct StaticRefCell<T> {
    data: UnsafeCell<T>,
    ref_cnt: Cell<usize>,
    _pin: PhantomPinned,
}

struct Ref<T> {
    ptr: *const StaticRefCell<T>,
}

struct RefMut<T> {
    ptr: *mut StaticRefCell<T>,
}

impl<T> StaticRefCell<T> {
    pub fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            ref_cnt: Cell::new(0),
            _pin: PhantomPinned,
        }
    }

    fn is_borrowed(&self) -> bool {
        self.ref_cnt.get() != 0 && self.ref_cnt.get() != BORROWED_MUT
    }

    fn is_borrowed_mut(&self) -> bool {
        self.ref_cnt.get() == BORROWED_MUT
    }

    pub fn try_borrow(&self) -> Option<Ref<T>> {
        match self.is_borrowed_mut() {
            true => None,
            false => {
                self.ref_cnt.set(self.ref_cnt.get() + 1);
                Some(Ref { ptr: self })
            }
        }
    }

    pub fn try_borrow_mut(&self) -> Option<RefMut<T>> {
        match self.is_borrowed() {
            true => None,
            false => {
                self.ref_cnt.set(BORROWED_MUT);
                Some(RefMut {
                    ptr: self as *const _ as *mut _,
                }) //TODO: okay?
            }
        }
    }

    pub fn borrow(&self) -> Ref<T> {
        self.try_borrow().expect("already mutably borrowed")
    }

    pub fn borrow_mut(&self) -> RefMut<T> {
        self.try_borrow_mut().expect("already borrowed")
    }
}

impl<T> Drop for StaticRefCell<T> {
    fn drop(&mut self) {
        if self.is_borrowed() || self.is_borrowed_mut() {
            panic!("already borrowed");
        }
    }
}

impl<T> Deref for Ref<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(*self.ptr).data.get() }
    }
}

impl<T> Drop for Ref<T> {
    fn drop(&mut self) {
        let ref_cnt = unsafe { &(*self.ptr).ref_cnt };
        ref_cnt.set(ref_cnt.get() - 1);
    }
}

impl<T> RefMut<T> {
    fn get_pin_mut(&mut self) -> Pin<&mut T> {
        unsafe { Pin::new_unchecked(&mut *(*self.ptr).data.get()) }
    }
}

impl<T> Deref for RefMut<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(*self.ptr).data.get() }
    }
}

impl<T: Unpin> DerefMut for RefMut<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_pin_mut().get_mut()
    }
}

impl<T> Drop for RefMut<T> {
    fn drop(&mut self) {
        unsafe {
            (*self.ptr).ref_cnt.set(0);
        }
    }
}

// fn main() {
//     let blah = StaticRefCell::new(10);
//     let r = blah.borrow();
//     assert!(*r == 10);
//     drop(r); // if not included, panics
//     let mut r2 = blah.borrow_mut();
//     *r2 = 5;
//     assert!(*r2 == 5);
//     drop(r2); // if not included, panics
//     let r3 = blah.borrow();
//     assert!(*r3 == 5);
//     drop(r3);
//     let mut r4 = blah.borrow_mut();
//     *r4 = 10;
//     assert!(*r4 == 10);
// }
