use core::mem;
use core::sync::atomic::{spin_loop_hint, AtomicBool, Ordering};

use crate::{
    bio::binit,
    console::{consoleinit, Console},
    kalloc::kinit,
    plic::{plicinit, plicinithart},
    println,
    proc::{cpuid, scheduler, PROCSYS},
    sleepablelock::Sleepablelock,
    trap::{trapinit, trapinithart},
    uart::Uart,
    virtio_disk::virtio_disk_init,
    vm::{kvminit, kvminithart},
};

/// The kernel.
pub static mut KERNEL: mem::MaybeUninit<Kernel> = mem::MaybeUninit::uninit();

/// The kernel can be mutably accessed only during the initialization.
pub unsafe fn kernel_mut() -> &'static mut Kernel {
    &mut *KERNEL.as_mut_ptr()
}

/// After intialized, the kernel is safe to immutably access.
pub fn kernel() -> &'static Kernel {
    unsafe { &*KERNEL.as_ptr() }
}

pub struct Kernel {
    panicked: AtomicBool,

    /// Sleeps waiting for there are some input in console buffer.
    pub console: Sleepablelock<Console>,
}

impl Kernel {
    fn panic(&self) {
        self.panicked.store(true, Ordering::Release);
    }

    pub fn is_panicked(&self) -> bool {
        self.panicked.load(Ordering::Acquire)
    }
}

/// Handles panic.
#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo<'_>) -> ! {
    // Freeze other CPUs.
    kernel().panic();
    println!("{}", info);

    crate::utils::spin_loop()
}

static STARTED: AtomicBool = AtomicBool::new(false);

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn kernel_main() {
    if cpuid() == 0 {
        let uart = Uart::new();
        kernel_mut().console = Sleepablelock::new("CONS", Console::new(uart));

        println!();
        println!("rv6 kernel is booting");
        println!();

        // Physical page allocator.
        kinit();

        // Create kernel page table.
        kvminit();

        // Turn on paging.
        kvminithart();

        // Process system.
        PROCSYS.init();

        // Trap vectors.
        trapinit();

        // Install kernel trap vector.
        trapinithart();

        // Set up interrupt controller.
        plicinit();

        // Ask PLIC for device interrupts.
        plicinithart();

        // Buffer cache.
        binit();

        // Emulated hard disk.
        virtio_disk_init();

        consoleinit();

        // First user process.
        PROCSYS.user_proc_init();
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {
            spin_loop_hint();
        }

        println!("hart {} starting", cpuid());

        // Turn on paging.
        kvminithart();

        // Install kernel trap vector.
        trapinithart();

        // Ask PLIC for device interrupts.
        plicinithart();
    }

    scheduler();
}
