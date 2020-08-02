use crate::libc;
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

static mut started: AtomicBool = AtomicBool::new(false);

/// start() jumps here in supervisor mode on all CPUs.
#[export_name = "main"]
pub unsafe fn kernel_main() {
    if cpuid() == 0 as i32 {
        consoleinit();
        printfinit();
        
        printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char);
        printf(
            b"xv6 kernel is booting\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
        );
        printf(b"\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char); 
        
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
        
        // file table
        fileinit();
        
        // emulated hard disk
        virtio_disk_init();
        
        // first user process
        userinit();
        started.store(true, Ordering::Release);
    } else {
        while !started.load(Ordering::Acquire) {}

        printf(
            b"hart %d starting\n\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            cpuid(),
        );

        // turn on paging
        kvminithart(); 

        // install kernel trap vector
        trapinithart(); 

        // ask PLIC for device interrupts
        plicinithart();
    }
    
    scheduler();
}
