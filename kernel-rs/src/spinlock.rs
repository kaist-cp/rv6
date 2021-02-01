use crate::{
    kernel::kernel,
    proc::{Cpu, Waitable},
    riscv::{intr_get, intr_off, intr_on},
};
use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

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

    /// Acquires the lock.
    /// Loops (spins) until the lock is acquired.
    ///
    /// # Safety
    ///
    /// To ensure that all stores done in one critical section are visible in the
    /// next critical section's loads, we use an atomic exchange with `Acquire` ordering
    /// here and pair it with an atomic store with `Release` ordering in `RawSpinlock::release()`.
    /// In this way, we tell the compiler/processor not to move loads that should
    /// originally happen after acquiring the lock to actually happen before acquiring the lock,
    /// since that may make loads read stale values. (Also see `RawSpinlock::release()`.)
    /// Additionally, note that an extra fence is unneccessary due to the pair of Acquire/Release ordering.
    pub fn acquire(&self) {
        // Disable interrupts to avoid deadlock.
        unsafe {
            push_off();
        }
        assert!(!self.holding(), "acquire {}", self.name);

        // RISC-V supports two forms of atomic instructions, 1) load-reserved/store-conditional and 2) atomic fetch-and-op,
        // and we use the former here.
        //
        // 0x80000fdc | lr.d.aq a2,(a0)         (load-reserved, dword, acquire-ordering)
        // 0x80000fe0 | bnez    a2,0x80000fe8   (goto snez)
        // 0x80000fe2 | sc.d    a3,a1,(a0)      (store-conditional, dword)
        // 0x80000fe6 | bnez    a3,0x80000fdc   (go back to start of loop)
        // 0x80000fe8 | snez    a0,a2           (set if not zero)
        while self
            .locked
            .compare_exchange(
                ptr::null_mut(),
                kernel().mycpu(),
                Ordering::Acquire,
                // Okay to use `Relaxed` ordering since we don't enter the critical section anyway
                // if the exchange fails.
                Ordering::Relaxed,
            )
            .is_err()
        {
            spin_loop();
        }
    }

    /// Releases the lock.
    ///
    /// # Safety
    ///
    /// To ensure that all stores done in one critical section are visible in the
    /// next critical section's loads, we use an atomic store with `Release` ordering
    /// here and pair it with an atomic exchange with `Acquire` ordering in `RawSpinlock::acquire()`.
    /// In this way, we tell the compiler/processor not to move stores that should
    /// originally happen before releasing the lock to actually happen after releasing the lock,
    /// since that may make the next critical section's loads read stale values. (Also see `RawSpinlock::acquire()`.)
    /// Additionally, note that an extra fence is unneccessary due to the pair of Acquire/Release ordering.
    pub fn release(&self) {
        assert!(self.holding(), "release {}", self.name);

        // Release the lock by storing ptr::null_mut() in `self.locked`,
        // using an atomic store. This is actually done using a fence in RISC-V.
        //
        // 0x80000f5c | fence   rw,w            (Enforces `Release` memory ordering)
        self.locked.store(ptr::null_mut(), Ordering::Release);
        unsafe {
            pop_off();
        }
    }

    /// Check whether this cpu is holding the lock.
    /// Interrupts must be off.
    pub fn holding(&self) -> bool {
        self.locked.load(Ordering::Relaxed) == kernel().mycpu()
    }
}

pub struct SpinlockGuard<'s, T> {
    lock: &'s Spinlock<T>,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s, T: Sync> Sync for SpinlockGuard<'s, T> {}

pub struct Spinlock<T> {
    lock: RawSpinlock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Spinlock<T> {}

pub struct SpinlockProtectedGuard<'s> {
    lock: &'s RawSpinlock,
    _marker: PhantomData<*const ()>,
}

// Do not implement Send; lock must be unlocked by the CPU that acquired it.
unsafe impl<'s> Sync for SpinlockProtectedGuard<'s> {}

/// Similar to `Spinlock<T>`, but instead of internally owning a `RawSpinlock`,
/// this stores a `'static` reference to an external `RawSpinlock` that was provided by the caller.
/// By making multiple `SpinlockProtected<T>`'s refer to a single `RawSpinlock`,
/// you can make multiple data be protected by a single `RawSpinlock`, and hence,
/// implement global locks.
/// To dereference the inner data, you must use `SpinlockProtected<T>::get_mut`, instead of
/// trying to dereference the `SpinlockProtectedGuard`.
pub struct SpinlockProtected<T> {
    lock: &'static RawSpinlock,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinlockProtected<T> {}

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

    pub fn lock(&self) -> SpinlockGuard<'_, T> {
        self.lock.acquire();

        SpinlockGuard {
            lock: self,
            _marker: PhantomData,
        }
    }

    pub unsafe fn unlock(&self) {
        self.lock.release();
    }

    /// Check whether this cpu is holding the lock.
    pub fn holding(&self) -> bool {
        self.lock.holding()
    }

    /// # Safety
    ///
    /// `self` must not be shared by other threads. Use this function only in the middle of
    /// refactoring.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get_mut_unchecked(&self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }

    pub fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }

    pub fn raw(&self) -> *const RawSpinlock {
        &self.lock as *const _
    }
}

impl<T> SpinlockGuard<'_, T> {
    pub fn reacquire_after<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.lock.lock.release();
        let result = f();
        self.lock.lock.acquire();
        result
    }
}

impl<T> Waitable for SpinlockGuard<'_, T> {
    unsafe fn raw_release(&mut self) {
        self.lock.lock.release();
    }
    unsafe fn raw_acquire(&mut self) {
        self.lock.lock.acquire();
    }
}

impl<T> Drop for SpinlockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.release();
    }
}

impl<T> Deref for SpinlockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for SpinlockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> SpinlockProtected<T> {
    pub const fn new(raw_lock: &'static RawSpinlock, data: T) -> Self {
        Self {
            lock: raw_lock,
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinlockProtectedGuard<'_> {
        self.lock.acquire();

        SpinlockProtectedGuard {
            lock: self.lock,
            _marker: PhantomData,
        }
    }

    /// Returns a mutable reference to the inner data, provided that the given
    /// `guard: SpinlockProtectedGuard` was obtained from a `SpinlockProtected`
    /// that refers to the same `RawSpinlock` with this `SpinlockProtected`.
    ///
    /// # Note
    ///
    /// In order to prevent references from leaking, the returned reference
    /// cannot outlive the given `guard`.
    ///
    /// This method adds some small runtime cost, since we need to check that the given
    /// `SpinlockProtectedGuard` was truely originated from a `SpinlockProtected`
    /// that refers to the same `RawSpinlock`.
    /// TODO(https://github.com/kaist-cp/rv6/issues/375)
    /// This runtime cost can be removed by using a trait, such as `pub trait SpinlockID {}`.
    pub fn get_mut<'a: 'b, 'b>(&'a self, guard: &'b mut SpinlockProtectedGuard<'a>) -> &'b mut T {
        assert!(ptr::eq(self.lock, guard.lock));
        unsafe { &mut *self.data.get() }
    }

    /// Check whether this cpu is holding the lock.
    pub fn holding(&self) -> bool {
        self.lock.holding()
    }
}

impl Waitable for SpinlockProtectedGuard<'_> {
    unsafe fn raw_release(&mut self) {
        self.lock.release();
    }
    unsafe fn raw_acquire(&mut self) {
        self.lock.acquire();
    }
}

impl Drop for SpinlockProtectedGuard<'_> {
    fn drop(&mut self) {
        self.lock.release();
    }
}

/// push_off/pop_off are like intr_off()/intr_on() except that they are matched:
/// it takes two pop_off()s to undo two push_off()s.  Also, if interrupts
/// are initially off, then push_off, pop_off leaves them off.
pub unsafe fn push_off() {
    let old = unsafe { intr_get() };
    unsafe { intr_off() };
    if unsafe { (*(kernel().mycpu())).noff } == 0 {
        unsafe { (*(kernel().mycpu())).interrupt_enabled = old }
    }
    unsafe { (*(kernel().mycpu())).noff += 1 };
}

pub unsafe fn pop_off() {
    let mut c: *mut Cpu = kernel().mycpu();
    assert!(!unsafe { intr_get() }, "pop_off - interruptible");
    assert!(unsafe { (*c).noff } >= 1, "pop_off");

    unsafe { (*c).noff -= 1 };

    if unsafe { (*c).noff == 0 } && unsafe { (*c).interrupt_enabled } {
        unsafe { intr_on() };
    }
}
