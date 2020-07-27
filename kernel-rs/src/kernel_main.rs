use crate::{
    bio::binit,
    console::consoleinit,
    file::fileinit,
    fs::iinit,
    kalloc::kinit,
    plic::{plicinit, plicinithart},
    printf::{printf, printfinit},
    proc::{cpuid, procinit, scheduler, userinit},
    trap::{trapinit, trapinithart},
    virtio_disk::virtio_disk_init,
    vm::{kvminit, kvminithart},
};
use core::sync::atomic::{AtomicBool, Ordering};
/// start() jumps here in supervisor mode on all CPUs.
#[export_name = "main"]
pub unsafe extern "C" fn main_0() {
    let started: AtomicBool = AtomicBool::new(false);
    // physical page allocator
    if cpuid() == 0 as i32 {
        consoleinit(); // create kernel page table
        printfinit(); // turn on paging
        printf(b"\n\x00" as *const u8 as *mut i8); // process table
        printf(b"xv6 kernel is booting\n\x00" as *const u8 as *mut i8); // trap vectors
        printf(b"\n\x00" as *const u8 as *mut i8); // install kernel trap vector
        kinit(); // set up interrupt controller
        kvminit(); // ask PLIC for device interrupts
        kvminithart(); // buffer cache
        procinit(); // inode cache
        trapinit(); // file table
        trapinithart(); // emulated hard disk
        plicinit(); // first user process
        plicinithart();
        binit();
        iinit();
        fileinit();
        virtio_disk_init();
        userinit();
        started.store(true, Ordering::Release);
    } else {
        while !started.load(Ordering::Acquire) {}
        printf(b"hart %d starting\n\x00" as *const u8 as *mut i8, cpuid());
        // ask PLIC for device interrupts
        kvminithart(); // turn on paging
        trapinithart(); // install kernel trap vector
        plicinithart();
    }
    scheduler();
}
