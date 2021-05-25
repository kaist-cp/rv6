use core::{cell::UnsafeCell, ops::Deref, ptr::NonNull};

use super::branded::Branded;
use crate::lock::{RawSpinlock, RemoteLock, SpinlockGuard};

const BORROWED_MUT: usize = usize::MAX;

pub struct RcCell<T> {
    data: UnsafeCell<T>,
    rc: RemoteLock<RawSpinlock, (), usize>,
}

#[repr(transparent)]
pub struct BrandedRcCell<'id, T>(Branded<'id, RcCell<T>>);

#[repr(transparent)]
pub struct SharedGuard<'id, 's>(Branded<'id, SpinlockGuard<'s, ()>>);

#[repr(transparent)]
pub struct Ref<T>(NonNull<RcCell<T>>);

#[repr(transparent)]
pub struct BrandedRef<'id, T>(Branded<'id, Ref<T>>);

#[repr(transparent)]
pub struct RefMut<T>(NonNull<RcCell<T>>);

#[repr(transparent)]
pub struct BrandedRefMut<'id, T>(Branded<'id, RefMut<T>>);

impl<T> RcCell<T> {
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            rc: RemoteLock::new(0),
        }
    }
}

impl<'id, T> BrandedRcCell<'id, T> {
    pub fn get_rc_mut<'a: 'b, 'b>(&'a self, guard: &'b mut SharedGuard<'id, '_>) -> &'b mut usize {
        unsafe { self.0.rc.get_mut_unchecked(&mut guard.0) }
    }

    pub fn get_data<'a: 'b, 'b>(&'a self, guard: &'b mut SharedGuard<'id, '_>) -> Option<&'b T> {
        let rc = self.get_rc_mut(guard);
        if *rc != BORROWED_MUT {
            Some(unsafe { &*self.0.data.get() })
        } else {
            None
        }
    }

    pub fn get_data_mut<'a: 'b, 'b>(
        &'a self,
        guard: &'b mut SharedGuard<'id, '_>,
    ) -> Option<&'b mut T> {
        let rc = self.get_rc_mut(guard);
        if *rc == 0 {
            Some(unsafe { self.get_data_mut_unchecked(guard) })
        } else {
            None
        }
    }

    pub unsafe fn get_data_mut_unchecked<'a: 'b, 'b>(
        &'a self,
        _: &'b mut SharedGuard<'id, '_>,
    ) -> &'b mut T {
        unsafe { &mut *self.0.data.get() }
    }

    pub fn make_ref(&self, guard: &mut SharedGuard<'id, '_>) -> BrandedRef<'id, T> {
        let rc = self.get_rc_mut(guard);
        assert!(*rc < usize::MAX - 1);
        *rc += 1;
        BrandedRef(self.0.brand(Ref(NonNull::from(self.0.deref()))))
    }
}

impl<'id, 's> SharedGuard<'id, 's> {
    pub unsafe fn new_unchecked(guard: Branded<'id, SpinlockGuard<'s, ()>>) -> Self {
        Self(guard)
    }

    pub fn inner_mut(&mut self) -> &mut SpinlockGuard<'s, ()> {
        &mut self.0
    }
}

impl<T> Ref<T> {
    fn get_cell(&self) -> &RcCell<T> {
        unsafe { self.0.as_ref() }
    }
}

impl<T> Deref for Ref<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.get_cell().data.get() }
    }
}

impl<T> Drop for Ref<T> {
    fn drop(&mut self) {
        panic!();
    }
}

impl<'id, T> BrandedRef<'id, T> {
    pub unsafe fn new_unchecked(r: Branded<'id, Ref<T>>) -> Self {
        Self(r)
    }

    pub fn clone(&self, guard: &mut SharedGuard<'id, '_>) -> Self {
        let rc = self.get_cell().get_rc_mut(guard);
        assert!(*rc < usize::MAX - 1);
        *rc += 1;
        BrandedRef(self.0.brand(Ref(self.0 .0)))
    }

    pub fn free(self, guard: &mut SharedGuard<'id, '_>) {
        let rc = self.get_cell().get_rc_mut(guard);
        *rc -= 1;
        core::mem::forget(self);
    }

    pub fn into_mut(self, guard: &mut SharedGuard<'id, '_>) -> Result<BrandedRefMut<'id, T>, Self> {
        let rc = self.get_cell().get_rc_mut(guard);
        if *rc == 1 {
            *rc = BORROWED_MUT;
            let r = Ok(BrandedRefMut(self.0.brand(RefMut(self.0 .0))));
            core::mem::forget(self);
            r
        } else {
            Err(self)
        }
    }

    pub fn get_cell(&self) -> &BrandedRcCell<'id, T> {
        unsafe { &*(self.0.get_cell() as *const _ as *const _) }
    }

    pub fn into_ref(self) -> Ref<T> {
        let r = Ref(self.0 .0);
        core::mem::forget(self);
        r
    }
}

impl<T> Drop for BrandedRef<'_, T> {
    fn drop(&mut self) {
        panic!();
    }
}

impl<T> RefMut<T> {
    fn get_cell(&self) -> &RcCell<T> {
        unsafe { self.0.as_ref() }
    }
}

impl<T> Drop for RefMut<T> {
    fn drop(&mut self) {
        panic!();
    }
}

impl<'id, T> BrandedRefMut<'id, T> {
    pub fn get_data_mut(&mut self) -> &mut T {
        unsafe { &mut *self.0.get_cell().data.get() }
    }

    pub fn into_ref(self, guard: &mut SharedGuard<'id, '_>) -> BrandedRef<'id, T> {
        let rc = self.get_cell().get_rc_mut(guard);
        *rc = 1;
        let r = BrandedRef(self.0.brand(Ref(self.0 .0)));
        core::mem::forget(self);
        r
    }

    pub fn get_cell(&self) -> &BrandedRcCell<'id, T> {
        unsafe { &*(self.0.get_cell() as *const _ as *const _) }
    }
}

impl<T> Drop for BrandedRefMut<'_, T> {
    fn drop(&mut self) {
        panic!();
    }
}
