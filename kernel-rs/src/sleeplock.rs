use crate::libc;
extern "C" {
    pub type file;
    pub type inode;
    #[no_mangle]
    fn myproc() -> *mut proc_0;
    #[no_mangle]
    fn sleep(_: *mut libc::c_void, _: *mut spinlock);
    #[no_mangle]
    fn wakeup(_: *mut libc::c_void);
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut spinlock);
    #[no_mangle]
    fn initlock(_: *mut spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut spinlock);
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;
// Saved registers for kernel context switches.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct context {
    pub ra: uint64,
    pub sp: uint64,
    pub s0: uint64,
    pub s1: uint64,
    pub s2: uint64,
    pub s3: uint64,
    pub s4: uint64,
    pub s5: uint64,
    pub s6: uint64,
    pub s7: uint64,
    pub s8: uint64,
    pub s9: uint64,
    pub s10: uint64,
    pub s11: uint64,
}
// Per-process state
#[derive(Copy, Clone)]
#[repr(C)]
pub struct proc_0 {
    pub lock: spinlock,
    pub state: procstate,
    pub parent: *mut proc_0,
    pub chan: *mut libc::c_void,
    pub killed: libc::c_int,
    pub xstate: libc::c_int,
    pub pid: libc::c_int,
    pub kstack: uint64,
    pub sz: uint64,
    pub pagetable: pagetable_t,
    pub tf: *mut trapframe,
    pub context: context,
    pub ofile: [*mut file; 16],
    pub cwd: *mut inode,
    pub name: [libc::c_char; 16],
}
// per-process data for the trap handling code in trampoline.S.
// sits in a page by itself just under the trampoline page in the
// user page table. not specially mapped in the kernel page table.
// the sscratch register points here.
// uservec in trampoline.S saves user registers in the trapframe,
// then initializes registers from the trapframe's
// kernel_sp, kernel_hartid, kernel_satp, and jumps to kernel_trap.
// usertrapret() and userret in trampoline.S set up
// the trapframe's kernel_*, restore user registers from the
// trapframe, switch to the user page table, and enter user space.
// the trapframe includes callee-saved user registers like s0-s11 because the
// return-to-user path via usertrapret() doesn't return through
// the entire kernel call stack.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct trapframe {
    pub kernel_satp: uint64,
    pub kernel_sp: uint64,
    pub kernel_trap: uint64,
    pub epc: uint64,
    pub kernel_hartid: uint64,
    pub ra: uint64,
    pub sp: uint64,
    pub gp: uint64,
    pub tp: uint64,
    pub t0: uint64,
    pub t1: uint64,
    pub t2: uint64,
    pub s0: uint64,
    pub s1: uint64,
    pub a0: uint64,
    pub a1: uint64,
    pub a2: uint64,
    pub a3: uint64,
    pub a4: uint64,
    pub a5: uint64,
    pub a6: uint64,
    pub a7: uint64,
    pub s2: uint64,
    pub s3: uint64,
    pub s4: uint64,
    pub s5: uint64,
    pub s6: uint64,
    pub s7: uint64,
    pub s8: uint64,
    pub s9: uint64,
    pub s10: uint64,
    pub s11: uint64,
    pub t3: uint64,
    pub t4: uint64,
    pub t5: uint64,
    pub t6: uint64,
}
pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
// Mutual exclusion lock.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct spinlock {
    pub locked: uint,
    pub name: *mut libc::c_char,
    pub cpu: *mut cpu,
}
// Per-CPU state.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct cpu {
    pub proc_0: *mut proc_0,
    pub scheduler: context,
    pub noff: libc::c_int,
    pub intena: libc::c_int,
}
// Long-term locks for processes
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sleeplock {
    pub locked: uint,
    pub lk: spinlock,
    pub name: *mut libc::c_char,
    pub pid: libc::c_int,
}
// Sleeping locks
#[no_mangle]
pub unsafe extern "C" fn initsleeplock(mut lk: *mut sleeplock, mut name: *mut libc::c_char) {
    initlock(
        &mut (*lk).lk,
        b"sleep lock\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
    );
    (*lk).name = name;
    (*lk).locked = 0 as libc::c_int as uint;
    (*lk).pid = 0 as libc::c_int;
}
// sleeplock.c
#[no_mangle]
pub unsafe extern "C" fn acquiresleep(mut lk: *mut sleeplock) {
    acquire(&mut (*lk).lk);
    while (*lk).locked != 0 {
        sleep(lk as *mut libc::c_void, &mut (*lk).lk);
    }
    (*lk).locked = 1 as libc::c_int as uint;
    (*lk).pid = (*myproc()).pid;
    release(&mut (*lk).lk);
}
#[no_mangle]
pub unsafe extern "C" fn releasesleep(mut lk: *mut sleeplock) {
    acquire(&mut (*lk).lk);
    (*lk).locked = 0 as libc::c_int as uint;
    (*lk).pid = 0 as libc::c_int;
    wakeup(lk as *mut libc::c_void);
    release(&mut (*lk).lk);
}
#[no_mangle]
pub unsafe extern "C" fn holdingsleep(mut lk: *mut sleeplock) -> libc::c_int {
    let mut r: libc::c_int = 0;
    acquire(&mut (*lk).lk);
    r = ((*lk).locked != 0 && (*lk).pid == (*myproc()).pid) as libc::c_int;
    release(&mut (*lk).lk);
    return r;
}
