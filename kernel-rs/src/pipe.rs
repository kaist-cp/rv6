use crate::{ libc, proc, file, spinlock };
use proc::proc_0;
use file::File;
use spinlock::Spinlock;
use core::ptr;
extern "C" {
    // file.c
    #[no_mangle]
    fn filealloc() -> *mut File;
    #[no_mangle]
    fn fileclose(_: *mut File);
    // kalloc.c
    #[no_mangle]
    fn kalloc() -> *mut libc::c_void;
    #[no_mangle]
    fn kfree(_: *mut libc::c_void);
    #[no_mangle]
    fn myproc() -> *mut proc_0;
    #[no_mangle]
    fn sleep(_: *mut libc::c_void, _: *mut Spinlock);
    #[no_mangle]
    fn wakeup(_: *mut libc::c_void);
    // spinlock.c
    #[no_mangle]
    fn acquire(_: *mut Spinlock);
    #[no_mangle]
    fn initlock(_: *mut Spinlock, _: *mut libc::c_char);
    #[no_mangle]
    fn release(_: *mut Spinlock);
    #[no_mangle]
    fn copyout(_: pagetable_t, _: uint64, _: *mut libc::c_char, _: uint64) -> libc::c_int;
    #[no_mangle]
    fn copyin(_: pagetable_t, _: *mut libc::c_char, _: uint64, _: uint64) -> libc::c_int;
}
pub type uint = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pagetable_t = *mut uint64;

pub type procstate = libc::c_uint;
pub const ZOMBIE: procstate = 4;
pub const RUNNING: procstate = 3;
pub const RUNNABLE: procstate = 2;
pub const SLEEPING: procstate = 1;
pub const UNUSED: procstate = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Pipe {
    pub lock: Spinlock,
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
pub unsafe extern "C" fn pipealloc(mut f0: *mut *mut File, mut f1: *mut *mut File) -> libc::c_int {
    let mut pi: *mut Pipe = ptr::null_mut();
    pi = ptr::null_mut();
    *f1 = 0 as *mut File;
    *f0 = *f1;
    *f0 = filealloc();
    if !((*f0).is_null() || {
        *f1 = filealloc();
        (*f1).is_null()
    }) {
        pi = kalloc() as *mut Pipe;
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
pub unsafe extern "C" fn pipeclose(mut pi: *mut Pipe, mut writable: libc::c_int) {
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
    mut pi: *mut Pipe,
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
    mut pi: *mut Pipe,
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
