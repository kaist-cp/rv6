use core::fmt::{self, Write};
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};

use pin_project::pin_project;

/// Handles panic by freezing other CPUs.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    let kernel = kernel_builder();
    kernel.panic();
    kernel.write_fmt(format_args!("{}\n", info));

    spin_loop()
}

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn main() -> ! {
    static INITED: AtomicBool = AtomicBool::new(false);

    if cpuid() == 0 {
        unsafe {
            hal_init();
        }
        unsafe {
            kernel_builder_unchecked_pin().init(allocator());
        }
        INITED.store(true, Ordering::Release);
    } else {
        while !INITED.load(Ordering::Acquire) {
            ::core::hint::spin_loop();
        }
        unsafe {
            kernel_ref(|kctx| kctx.inithart());
        }
    }

    unsafe { kernel_ref(|kctx| kctx.scheduler()) }
}
