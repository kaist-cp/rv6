use crate::libc;
extern "C" {
    // file.c
    #[no_mangle]
    fn filealloc() -> *mut file;
    #[no_mangle]
    fn fileclose(_: *mut file);
    // kalloc.c
    #[no_mangle]
    fn kalloc() -> *mut libc::c_void;
    #[no_mangle]
    fn kfree(_: *mut libc::c_void);
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
    #[no_mangle]
    fn copyout(_: pagetable_t, _: uint64, _: *mut libc::c_char, _: uint64) -> libc::c_int;
    #[no_mangle]
    fn copyin(_: pagetable_t, _: *mut libc::c_char, _: uint64, _: uint64) -> libc::c_int;
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
#[derive(Copy, Clone)]
#[repr(C)]
pub struct file {
    pub type_0: C2RustUnnamed,
    pub ref_0: libc::c_int,
    pub readable: libc::c_char,
    pub writable: libc::c_char,
    pub pipe: *mut pipe,
    pub ip: *mut inode,
    pub off: uint,
    pub major: libc::c_short,
}
// FD_DEVICE
// in-memory copy of an inode
#[derive(Copy, Clone)]
#[repr(C)]
pub struct inode {
    pub dev: uint,
    pub inum: uint,
    pub ref_0: libc::c_int,
    pub lock: sleeplock,
    pub valid: libc::c_int,
    pub type_0: libc::c_short,
    pub major: libc::c_short,
    pub minor: libc::c_short,
    pub nlink: libc::c_short,
    pub size: uint,
    pub addrs: [uint; 13],
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
#[derive(Copy, Clone)]
#[repr(C)]
pub struct pipe {
    pub lock: spinlock,
    pub data: [libc::c_char; 512],
    pub nread: uint,
    pub nwrite: uint,
    pub readopen: libc::c_int,
    pub writeopen: libc::c_int,
}
pub type C2RustUnnamed = libc::c_uint;
pub const FD_DEVICE: C2RustUnnamed = 3;
pub const FD_INODE: C2RustUnnamed = 2;
pub const FD_PIPE: C2RustUnnamed = 1;
pub const FD_NONE: C2RustUnnamed = 0;
pub const PIPESIZE: libc::c_int = 512 as libc::c_int;
// pipe.c
// write fd is still open
#[no_mangle]
pub unsafe extern "C" fn pipealloc(mut f0: *mut *mut file, mut f1: *mut *mut file) -> libc::c_int {
    let mut pi: *mut pipe = 0 as *mut pipe;
    pi = 0 as *mut pipe;
    *f1 = 0 as *mut file;
    *f0 = *f1;
    *f0 = filealloc();
    if !((*f0).is_null() || {
        *f1 = filealloc();
        (*f1).is_null()
    }) {
        pi = kalloc() as *mut pipe;
        if !pi.is_null() {
            (*pi).readopen = 1 as libc::c_int;
            (*pi).writeopen = 1 as libc::c_int;
            (*pi).nwrite = 0 as libc::c_int as uint;
            (*pi).nread = 0 as libc::c_int as uint;
            initlock(
                &mut (*pi).lock,
                b"pipe\x00" as *const u8 as *const libc::c_char as *mut libc::c_char,
            );
            (**f0).type_0 = FD_PIPE;
            (**f0).readable = 1 as libc::c_int as libc::c_char;
            (**f0).writable = 0 as libc::c_int as libc::c_char;
            (**f0).pipe = pi;
            (**f1).type_0 = FD_PIPE;
            (**f1).readable = 0 as libc::c_int as libc::c_char;
            (**f1).writable = 1 as libc::c_int as libc::c_char;
            (**f1).pipe = pi;
            return 0 as libc::c_int;
        }
    }
    if !pi.is_null() {
        kfree(pi as *mut libc::c_char as *mut libc::c_void);
    }
    if !(*f0).is_null() {
        fileclose(*f0);
    }
    if !(*f1).is_null() {
        fileclose(*f1);
    }
    -(1 as libc::c_int)
}
#[no_mangle]
pub unsafe extern "C" fn pipeclose(mut pi: *mut pipe, mut writable: libc::c_int) {
    acquire(&mut (*pi).lock);
    if writable != 0 {
        (*pi).writeopen = 0 as libc::c_int;
        wakeup(&mut (*pi).nread as *mut uint as *mut libc::c_void);
    } else {
        (*pi).readopen = 0 as libc::c_int;
        wakeup(&mut (*pi).nwrite as *mut uint as *mut libc::c_void);
    }
    if (*pi).readopen == 0 as libc::c_int && (*pi).writeopen == 0 as libc::c_int {
        release(&mut (*pi).lock);
        kfree(pi as *mut libc::c_char as *mut libc::c_void);
    } else {
        release(&mut (*pi).lock);
    };
}
#[no_mangle]
pub unsafe extern "C" fn pipewrite(
    mut pi: *mut pipe,
    mut addr: uint64,
    mut n: libc::c_int,
) -> libc::c_int {
    let mut i: libc::c_int = 0;
    let mut ch: libc::c_char = 0;
    let mut pr: *mut proc_0 = myproc();
    acquire(&mut (*pi).lock);
    i = 0 as libc::c_int;
    while i < n {
        while (*pi).nwrite == (*pi).nread.wrapping_add(PIPESIZE as libc::c_uint) {
            //DOC: pipewrite-full
            if (*pi).readopen == 0 as libc::c_int || (*myproc()).killed != 0 {
                release(&mut (*pi).lock);
                return -(1 as libc::c_int);
            }
            wakeup(&mut (*pi).nread as *mut uint as *mut libc::c_void);
            sleep(
                &mut (*pi).nwrite as *mut uint as *mut libc::c_void,
                &mut (*pi).lock,
            );
        }
        if copyin(
            (*pr).pagetable,
            &mut ch,
            addr.wrapping_add(i as libc::c_ulong),
            1 as libc::c_int as uint64,
        ) == -(1 as libc::c_int)
        {
            break;
        }
        let fresh0 = (*pi).nwrite;
        (*pi).nwrite = (*pi).nwrite.wrapping_add(1);
        (*pi).data[fresh0.wrapping_rem(PIPESIZE as libc::c_uint) as usize] = ch;
        i += 1
    }
    wakeup(&mut (*pi).nread as *mut uint as *mut libc::c_void);
    release(&mut (*pi).lock);
    n
}
#[no_mangle]
pub unsafe extern "C" fn piperead(
    mut pi: *mut pipe,
    mut addr: uint64,
    mut n: libc::c_int,
) -> libc::c_int {
    let mut i: libc::c_int = 0;
    let mut pr: *mut proc_0 = myproc();
    let mut ch: libc::c_char = 0;
    acquire(&mut (*pi).lock);
    while (*pi).nread == (*pi).nwrite && (*pi).writeopen != 0 {
        //DOC: pipe-empty
        if (*myproc()).killed != 0 {
            release(&mut (*pi).lock);
            return -(1 as libc::c_int);
        }
        sleep(
            &mut (*pi).nread as *mut uint as *mut libc::c_void,
            &mut (*pi).lock,
        );
        //DOC: piperead-sleep
    }
    i = 0 as libc::c_int;
    while i < n {
        //DOC: piperead-copy
        if (*pi).nread == (*pi).nwrite {
            break; //DOC: piperead-wakeup
        }
        let fresh1 = (*pi).nread;
        (*pi).nread = (*pi).nread.wrapping_add(1);
        ch = (*pi).data[fresh1.wrapping_rem(PIPESIZE as libc::c_uint) as usize];
        if copyout(
            (*pr).pagetable,
            addr.wrapping_add(i as libc::c_ulong),
            &mut ch,
            1 as libc::c_int as uint64,
        ) == -(1 as libc::c_int)
        {
            break;
        }
        i += 1
    }
    wakeup(&mut (*pi).nwrite as *mut uint as *mut libc::c_void);
    release(&mut (*pi).lock);
    i
}
