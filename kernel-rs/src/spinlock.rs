use crate::{
    proc::{mycpu, Cpu},
    riscv::{intr_get, intr_off, intr_on},
};
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{spin_loop_hint, AtomicPtr, Ordering};

/// Mutual exclusion lock.
pub struct RawSpinlock {
    /// Name of lock.
    name: &'static str,

    /// If the lock is held, contains the pointer of `Cpu`.
    /// Otherwise, contains null.
    ///
    /// Records info about lock acquisition for holding() and debugging.
    locked: AtomicPtr<Cpu>,
}

impl RawSpinlock {
    /// Mutual exclusion spin locks.
    pub const fn new(name: &'static str) -> Self {
        Self {
            locked: AtomicPtr::new(ptr::null_mut()),
            name,
        }
    }

    /// Acquire the lock.
    /// Loops (spins) until the lock is acquired.
    pub fn acquire(&self) {
        // disable interrupts to avoid deadlock.
        unsafe {
            push_off();
        }
        if self.holding() {
            panic!("acquire {}", self.name);
        }

        // On RISC-V, sync_lock_test_and_set turns into an atomic swap:
        //   a5 = 1
        //   s1 = &self->locked
        //   amoswap.w.aq a5, a5, (s1)
        while self
            .locked
            .compare_exchange(
                ptr::null_mut(),
                mycpu(),
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            spin_loop_hint();
        }

        // Tell the C compiler and the processor to not move loads or stores
        // past this point, to ensure that the critical section's memory
        // references happen after the lock is acquired.
        //
        // TODO(@jeehoon): it's unnecessary.
        //
        // ::core::intrinsics::atomic_fence();
    }

    /// Release the lock.
    pub fn release(&self) {
        if !self.holding() {
            panic!("release {}", self.name);
        }

        // Tell the C compiler and the CPU to not move loads or stores
        // past this point, to ensure that all the stores in the critical
        // section are visible to other CPUs before the lock is released.
        // On RISC-V, this turns into a fence instruction.
        //
        // TODO(@jeehoon): it's unnecessary.
        //
        // ::core::intrinsics::atomic_fence();

        // Release the lock, equivalent to lk->locked = 0.
        // This code doesn't use a C assignment, since the C standard
        // implies that an assignment might be implemented with
        // multiple store instructions.
        // On RISC-V, sync_lock_release turns into an atomic swap:
        //   s1 = &lk->locked
        //   amoswap.w zero, zero, (s1)
        self.locked.store(ptr::null_mut(), Ordering::Release);
        unsafe {
            pop_off();
        }
    }

    /// Check whether this cpu is holding the lock.
    pub fn holding(&self) -> bool {
        unsafe {
            push_off();
            let ret = self.locked.load(Ordering::Relaxed) == mycpu();
            pop_off();
            ret
        }
    }
}

pub struct SpinLockGuard<'s, T> {
    lock: &'s Spinlock<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SpinLockGuard<'s, T> {}

pub struct Spinlock<T> {
    lock: RawSpinlock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            lock: RawSpinlock::new(name),
            data: UnsafeCell::new(data),
        }
    }

    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        self.lock.acquire();

        SpinLockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    /// # Safety
    ///
    /// `self` must not be shared by other threads. Use this function only in the middle of
    /// refactoring.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get_mut_unchecked(&self) -> &mut T {
        &mut *self.data.get()
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }
}

impl<T> SpinLockGuard<'_, T> {
    pub fn raw(&self) -> usize {
        self.lock as *const _ as usize
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.release();
    }
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

/// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
/// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
/// are initially off, then push_off, pop_off leaves them off.
pub unsafe fn push_off() {
    let old = intr_get();
    intr_off();
    if (*(mycpu())).noff == 0 {
        (*(mycpu())).interrupt_enabled = old
    }
    (*(mycpu())).noff += 1;
}

pub unsafe fn pop_off() {
    let mut c: *mut Cpu = mycpu();
    if intr_get() {
        panic!("pop_off - interruptible");
    }
    (*c).noff -= 1;
    if (*c).noff < 0 {
        panic!("pop_off");
    }
    if (*c).noff == 0 && (*c).interrupt_enabled {
        intr_on();
    }
}
