use core::cell::UnsafeCell;
use core::pin::Pin;

use crate::branded::Branded;
use crate::lock::{Guard, Lock, RawLock};

pub trait LockOwner {
    type R: RawLock;

    fn get_lock(&self) -> &Lock<Self::R, ()>;
}

pub trait DataOwner {
    type L: LockOwner;
    type T;

    fn get_data(&self) -> &UnsafeCell<Self::T>;
}

impl<'id, 'a, L: LockOwner> Branded<'id, &'a L> {
    pub fn lock(self) -> Branded<'id, Guard<'a, L::R, ()>> {
        self.brand(self.get_lock().lock())
    }
}

impl<'id, 'a, L: LockOwner> Branded<'id, Pin<&'a L>> {
    pub fn lock(self) -> Branded<'id, Guard<'a, L::R, ()>> {
        self.brand(self.get_ref().get_lock().lock())
    }
}

impl<'id, 'a, D: DataOwner> Branded<'id, &'a D> {
    pub fn get_mut<'b>(
        self,
        _guard: &'b mut Branded<'id, Guard<'_, <D::L as LockOwner>::R, ()>>,
    ) -> &'b mut D::T
    where
        'a: 'b,
    {
        unsafe { &mut *self.get_data().get() }
    }
}

impl<'id, 'a, D: DataOwner> Branded<'id, Pin<&'a D>> {
    pub fn get_mut<'b>(
        self,
        _guard: &'b mut Branded<'id, Guard<'_, <D::L as LockOwner>::R, ()>>,
    ) -> &'b mut D::T
    where
        'a: 'b,
    {
        unsafe { &mut *self.get_data().get() }
    }
}
