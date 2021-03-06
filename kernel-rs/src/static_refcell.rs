use core::cell::{Cell, UnsafeCell};
use core::convert::TryFrom;
use core::marker::PhantomPinned;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;

const BORROWED_MUT: usize = usize::MAX;

/// Similar to `RefCell<T>`, but does not use lifetimes.
pub struct StaticRefCell<T> {
    data: UnsafeCell<T>,
    refcnt: Cell<usize>,
    _pin: PhantomPinned,
}

pub struct Ref<T> {
    ptr: *const StaticRefCell<T>,
}

pub struct RefMut<T> {
    ptr: *mut StaticRefCell<T>,
}

impl<T> StaticRefCell<T> {
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            refcnt: Cell::new(0),
            _pin: PhantomPinned,
        }
    }

    fn is_borrowed(&self) -> bool {
        self.refcnt.get() != 0 && self.refcnt.get() != BORROWED_MUT
    }

    fn is_borrowed_mut(&self) -> bool {
        self.refcnt.get() == BORROWED_MUT
    }

    pub fn try_borrow(&self) -> Option<Ref<T>> {
        match self.is_borrowed_mut() {
            true => None,
            false => {
                self.refcnt.set(self.refcnt.get() + 1);
                Some(Ref { ptr: self })
            }
        }
    }

    pub fn try_borrow_mut(&self) -> Option<RefMut<T>> {
        match self.is_borrowed() || self.is_borrowed_mut() {
            true => None,
            false => {
                self.refcnt.set(BORROWED_MUT);
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

impl<T> From<RefMut<T>> for Ref<T> {
    fn from(r: RefMut<T>) -> Self {
        let ptr = r.ptr;
        drop(r);
        unsafe {
            (*ptr).refcnt.set(1);
        }
        Self { ptr }
    }
}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        let refcnt = unsafe { &(*self.ptr).refcnt };
        refcnt.set(refcnt.get() + 1);
        Self { ptr: self.ptr }
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
        let refcnt = unsafe { &(*self.ptr).refcnt };
        refcnt.set(refcnt.get() - 1);
    }
}

impl<T> RefMut<T> {
    pub fn get_pin_mut(&mut self) -> Pin<&mut T> {
        unsafe { Pin::new_unchecked(&mut *(*self.ptr).data.get()) }
    }
}

impl<T> TryFrom<Ref<T>> for RefMut<T> {
    type Error = ();

    fn try_from(r: Ref<T>) -> Result<Self, Self::Error> {
        let refcnt = unsafe { &(*r.ptr).refcnt };
        if refcnt.get() == 1 {
            let ptr = r.ptr;
            drop(r);
            refcnt.set(BORROWED_MUT);
            Ok(RefMut { ptr: ptr as *mut _ })
        } else {
            Err(())
        }
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
            (*self.ptr).refcnt.set(0);
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
