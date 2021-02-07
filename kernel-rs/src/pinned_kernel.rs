use core::pin::Pin;

use pin_project::pin_project;

use crate::bio::BcacheInner;

/// A static variable to safely store the kernel's structs
/// that should never be moved.
static mut PINNED_KERNEL: PinnedKernel = PinnedKernel::zero();

/// A struct where we actually store the kernel's structs that are !Unpin.
/// Using this struct, we can safely only provide pinned mutable references
/// of this struct's fields to the outside.
///
/// # Safety
///
/// This struct should never be moved.
#[pin_project]
pub struct PinnedKernel {
    #[pin]
    pub bcache_inner: BcacheInner,
    // TODO: move `KERNEL.procs` to here, and other !Unpin structs
}

impl PinnedKernel {
    const fn zero() -> Self {
        Self {
            bcache_inner: unsafe { BcacheInner::zero() },
        }
    }
}

/// Returns a pinned mutable reference to the static `PinnedKernel`.
/// This is the only way to access the `PinnedKernel` from outside.
pub fn pinned_kernel() -> Pin<&'static mut PinnedKernel> {
    unsafe { Pin::new_unchecked(&mut PINNED_KERNEL) }
}
