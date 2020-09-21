use crate::{
    bio::binit,
    console::Console,
    fs::iinit,
    kalloc::kinit,
    plic::{plicinit, plicinithart},
    printf::printfinit,
    println,
    proc::{cpuid, scheduler, PROCSYS},
    trap::{trapinit, trapinithart},
    virtio_disk::virtio_disk_init,
    vm::{kvminit, kvminithart},
};
use core::sync::atomic::{AtomicBool, Ordering};

static STARTED: AtomicBool = AtomicBool::new(false);

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn kernel_main() {
    if cpuid() == 0 {
        Console::init();
        printfinit();

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

        // Inode cache.
        iinit();

        // Emulated hard disk.
        virtio_disk_init();

        // First user process.
        PROCSYS.user_proc_init();
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {}

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
