use crate::{
    bio::binit,
    console::consoleinit,
    fs::iinit,
    kalloc::kinit,
    plic::{plicinit, plicinithart},
    printf::printfinit,
    println,
    proc::{cpuid, procinit, scheduler, userinit},
    trap::{trapinit, trapinithart},
    virtio_disk::virtio_disk_init,
    vm::{kvminit, kvminithart},
};
use core::sync::atomic::{AtomicBool, Ordering};

static STARTED: AtomicBool = AtomicBool::new(false);

/// start() jumps here in supervisor mode on all CPUs.
pub unsafe fn kernel_main() {
    if cpuid() == 0 {
        consoleinit();
        printfinit();

        println!();
        println!("rv6 kernel is booting");
        println!();

        // physical page allocator
        kinit();

        // create kernel page table
        kvminit();

        // turn on paging
        kvminithart();

        // process table
        procinit();

        // trap vectors
        trapinit();

        // install kernel trap vector
        trapinithart();

        // set up interrupt controller
        plicinit();

        // ask PLIC for device interrupts
        plicinithart();

        // buffer cache
        binit();

        // inode cache
        iinit();

        // emulated hard disk
        virtio_disk_init();

        // first user process
        userinit();
        STARTED.store(true, Ordering::Release);
    } else {
        while !STARTED.load(Ordering::Acquire) {}

        println!("hart {} starting", cpuid());

        // turn on paging
        kvminithart();

        // install kernel trap vector
        trapinithart();

        // ask PLIC for device interrupts
        plicinithart();
    }

    scheduler();
}
