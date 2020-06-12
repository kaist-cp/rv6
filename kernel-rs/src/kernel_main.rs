use crate::libc;
extern "C" {
    // bio.c
    #[no_mangle]
    fn binit();
    // console.c
    #[no_mangle]
    fn consoleinit();
    #[no_mangle]
    fn fileinit();
    #[no_mangle]
    fn iinit();
    #[no_mangle]
    fn kinit();
    // printf.c
    #[no_mangle]
    fn printf(_: *mut libc::c_char, _: ...);
    #[no_mangle]
    fn printfinit();
    // proc.c
    #[no_mangle]
    fn cpuid() -> libc::c_int;
    #[no_mangle]
    fn procinit();
    #[no_mangle]
    fn scheduler() -> !;
    #[no_mangle]
    fn userinit();
    #[no_mangle]
    fn trapinit();
    #[no_mangle]
    fn trapinithart();
    // vm.c
    #[no_mangle]
    fn kvminit();
    #[no_mangle]
    fn kvminithart();
    // plic.c
    #[no_mangle]
    fn plicinit();
    #[no_mangle]
    fn plicinithart();
    // virtio_disk.c
    #[no_mangle]
    fn virtio_disk_init();
    // TODO: remove later
    #[no_mangle]
    fn hello_rs();
}
static mut started: libc::c_int = 0 as libc::c_int;
// start() jumps here in supervisor mode on all CPUs.
#[export_name = "main"]
pub unsafe extern "C" fn main_0() {
    // TODO: remove later
    hello_rs(); // physical page allocator
    if cpuid() == 0 as libc::c_int {
        consoleinit(); // create kernel page table
        printfinit(); // turn on paging
        printf(b"\n\x00" as *const u8 as *const libc::c_char as
                   *mut libc::c_char); // process table
        printf(b"xv6 kernel is booting\n\x00" as *const u8 as
                   *const libc::c_char as *mut libc::c_char); // trap vectors
        printf(b"\n\x00" as *const u8 as *const libc::c_char as
                   *mut libc::c_char); // install kernel trap vector
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
        ::core::intrinsics::atomic_fence();
        ::core::ptr::write_volatile(&mut started as *mut libc::c_int,
                                    1 as libc::c_int)
    } else {
        while started == 0 as libc::c_int { }
        ::core::intrinsics::atomic_fence();
        printf(b"hart %d starting\n\x00" as *const u8 as *const libc::c_char
                   as *mut libc::c_char, cpuid());
        // ask PLIC for device interrupts
        kvminithart(); // turn on paging
        trapinithart(); // install kernel trap vector
        plicinithart();
    }
    scheduler();
}
