use crate::libc;
use core::ptr;
use crate::proc::proc_0;
extern "C" {
    pub type inode;
    pub type file;
    #[no_mangle]
    fn ilock(_: *mut inode);
    #[no_mangle]
    fn iunlockput(_: *mut inode);
    #[no_mangle]
    fn namei(_: *mut libc::c_char) -> *mut inode;
    #[no_mangle]
    fn readi(_: *mut inode, _: libc::c_int, _: uint64, _: uint, _: uint) -> libc::c_int;
    #[no_mangle]
    fn begin_op();
    #[no_mangle]
    fn end_op();
    #[no_mangle]
    fn panic(_: *mut libc::c_char) -> !;
    #[no_mangle]
    fn proc_pagetable(_: *mut proc_0) -> pagetable_t;
    #[no_mangle]
    fn proc_freepagetable(_: pagetable_t, _: uint64);
    #[no_mangle]
    fn myproc() -> *mut proc_0;
    #[no_mangle]
    fn safestrcpy(
        _: *mut libc::c_char,
        _: *const libc::c_char,
        _: libc::c_int,
    ) -> *mut libc::c_char;
    #[no_mangle]
    fn strlen(_: *const libc::c_char) -> libc::c_int;
    #[no_mangle]
    fn uvmalloc(_: pagetable_t, _: uint64, _: uint64) -> uint64;
    #[no_mangle]
    fn uvmclear(_: pagetable_t, _: uint64);
    #[no_mangle]
    fn walkaddr(_: pagetable_t, _: uint64) -> uint64;
    #[no_mangle]
    fn copyout(_: pagetable_t, _: uint64, _: *mut libc::c_char, _: uint64) -> libc::c_int;
}
pub type uint = libc::c_uint;
pub type ushort = libc::c_ushort;
pub type uchar = libc::c_uchar;
pub type uint32 = libc::c_uint;
pub type uint64 = libc::c_ulong;
pub type pde_t = uint64;
pub type pagetable_t = *mut uint64;
// "\x7FELF" in little endian
// File header
#[derive(Copy, Clone)]
#[repr(C)]
pub struct elfhdr {
    pub magic: uint,
    pub elf: [uchar; 12],
    pub type_0: ushort,
    pub machine: ushort,
    pub version: uint,
    pub entry: uint64,
    pub phoff: uint64,
    pub shoff: uint64,
    pub flags: uint,
    pub ehsize: ushort,
    pub phentsize: ushort,
    pub phnum: ushort,
    pub shentsize: ushort,
    pub shnum: ushort,
    pub shstrndx: ushort,
}
// Program section header
#[derive(Copy, Clone)]
#[repr(C)]
pub struct proghdr {
    pub type_0: uint32,
    pub flags: uint32,
    pub off: uint64,
    pub vaddr: uint64,
    pub paddr: uint64,
    pub filesz: uint64,
    pub memsz: uint64,
    pub align: uint64,
}
// maximum number of processes
// maximum number of CPUs
// open files per process
// open files per system
// maximum number of active i-nodes
// maximum major device number
// device number of file system root disk
pub const MAXARG: libc::c_int = 32 as libc::c_int;
pub const PGSIZE: libc::c_int = 4096 as libc::c_int;
// Format of an ELF executable file
pub const ELF_MAGIC: libc::c_uint = 0x464c457f as libc::c_uint;
// Values for Proghdr type
pub const ELF_PROG_LOAD: libc::c_int = 1 as libc::c_int;
// exec.c
#[no_mangle]
pub unsafe extern "C" fn exec(
    mut path: *mut libc::c_char,
    mut argv: *mut *mut libc::c_char,
) -> libc::c_int {
    let mut oldsz: uint64 = 0;
    let mut current_block: u64;
    let mut s: *mut libc::c_char = ptr::null_mut();
    let mut last: *mut libc::c_char = ptr::null_mut();
    let mut i: libc::c_int = 0;
    let mut off: libc::c_int = 0;
    let mut argc: uint64 = 0;
    let mut sz: uint64 = 0;
    let mut sp: uint64 = 0;
    let mut ustack: [uint64; 33] = [0; 33];
    let mut stackbase: uint64 = 0;
    let mut elf: elfhdr = elfhdr {
        magic: 0,
        elf: [0; 12],
        type_0: 0,
        machine: 0,
        version: 0,
        entry: 0,
        phoff: 0,
        shoff: 0,
        flags: 0,
        ehsize: 0,
        phentsize: 0,
        phnum: 0,
        shentsize: 0,
        shnum: 0,
        shstrndx: 0,
    };
    let mut ip: *mut inode = 0 as *mut inode;
    let mut ph: proghdr = proghdr {
        type_0: 0,
        flags: 0,
        off: 0,
        vaddr: 0,
        paddr: 0,
        filesz: 0,
        memsz: 0,
        align: 0,
    };
    let mut pagetable: pagetable_t = 0 as pagetable_t;
    let mut oldpagetable: pagetable_t = ptr::null_mut();
    let mut p: *mut proc_0 = myproc();
    begin_op();
    ip = namei(path);
    if ip.is_null() {
        end_op();
        return -(1 as libc::c_int);
    }
    ilock(ip);
    // Check ELF header
    if readi(
        ip,
        0 as libc::c_int,
        &mut elf as *mut elfhdr as uint64,
        0 as libc::c_int as uint,
        ::core::mem::size_of::<elfhdr>() as libc::c_ulong as uint,
    ) as libc::c_ulong
        == ::core::mem::size_of::<elfhdr>() as libc::c_ulong
        && elf.magic == ELF_MAGIC
    {
        pagetable = proc_pagetable(p);
        if !pagetable.is_null() {
            // Load program into memory.
            sz = 0 as libc::c_int as uint64;
            i = 0 as libc::c_int;
            off = elf.phoff as libc::c_int;
            loop {
                if i >= elf.phnum as libc::c_int {
                    current_block = 15768484401365413375;
                    break;
                }
                if readi(
                    ip,
                    0 as libc::c_int,
                    &mut ph as *mut proghdr as uint64,
                    off as uint,
                    ::core::mem::size_of::<proghdr>() as libc::c_ulong as uint,
                ) as libc::c_ulong
                    != ::core::mem::size_of::<proghdr>() as libc::c_ulong
                {
                    current_block = 7080392026674647309;
                    break;
                }
                if ph.type_0 == ELF_PROG_LOAD as libc::c_uint {
                    if ph.memsz < ph.filesz {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if ph.vaddr.wrapping_add(ph.memsz) < ph.vaddr {
                        current_block = 7080392026674647309;
                        break;
                    }
                    sz = uvmalloc(pagetable, sz, ph.vaddr.wrapping_add(ph.memsz));
                    if sz == 0 as libc::c_int as libc::c_ulong {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if ph.vaddr.wrapping_rem(PGSIZE as libc::c_ulong)
                        != 0 as libc::c_int as libc::c_ulong
                    {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if loadseg(pagetable, ph.vaddr, ip, ph.off as uint, ph.filesz as uint)
                        < 0 as libc::c_int
                    {
                        current_block = 7080392026674647309;
                        break;
                    }
                }
                i += 1;
                off = (off as libc::c_ulong)
                    .wrapping_add(::core::mem::size_of::<proghdr>() as libc::c_ulong)
                    as libc::c_int as libc::c_int
            }
            match current_block {
                7080392026674647309 => {}
                _ => {
                    iunlockput(ip);
                    end_op();
                    ip = 0 as *mut inode;
                    p = myproc();
                    oldsz = (*p).sz;
                    // Allocate two pages at the next page boundary.
                    // Use the second as the user stack.
                    sz = sz
                        .wrapping_add(PGSIZE as libc::c_ulong)
                        .wrapping_sub(1 as libc::c_int as libc::c_ulong)
                        & !(PGSIZE - 1 as libc::c_int) as libc::c_ulong;
                    sz = uvmalloc(
                        pagetable,
                        sz,
                        sz.wrapping_add((2 as libc::c_int * PGSIZE) as libc::c_ulong),
                    );
                    if sz != 0 as libc::c_int as libc::c_ulong {
                        uvmclear(
                            pagetable,
                            sz.wrapping_sub((2 as libc::c_int * PGSIZE) as libc::c_ulong),
                        );
                        sp = sz;
                        stackbase = sp.wrapping_sub(PGSIZE as libc::c_ulong);
                        // Push argument strings, prepare rest of stack in ustack.
                        argc = 0 as libc::c_int as uint64; // riscv sp must be 16-byte aligned
                        loop {
                            if (*argv.offset(argc as isize)).is_null() {
                                current_block = 4567019141635105728;
                                break;
                            }
                            if argc >= MAXARG as libc::c_ulong {
                                current_block = 7080392026674647309;
                                break;
                            }
                            sp = (sp as libc::c_ulong).wrapping_sub(
                                (strlen(*argv.offset(argc as isize)) + 1 as libc::c_int)
                                    as libc::c_ulong,
                            ) as uint64 as uint64;
                            sp = (sp as libc::c_ulong)
                                .wrapping_sub(sp.wrapping_rem(16 as libc::c_int as libc::c_ulong))
                                as uint64 as uint64;
                            if sp < stackbase {
                                current_block = 7080392026674647309;
                                break;
                            }
                            if copyout(
                                pagetable,
                                sp,
                                *argv.offset(argc as isize),
                                (strlen(*argv.offset(argc as isize)) + 1 as libc::c_int) as uint64,
                            ) < 0 as libc::c_int
                            {
                                current_block = 7080392026674647309;
                                break;
                            }
                            ustack[argc as usize] = sp;
                            argc = argc.wrapping_add(1)
                        }
                        match current_block {
                            7080392026674647309 => {}
                            _ => {
                                ustack[argc as usize] = 0 as libc::c_int as uint64;
                                // push the array of argv[] pointers.
                                sp = (sp as libc::c_ulong).wrapping_sub(
                                    argc.wrapping_add(1 as libc::c_int as libc::c_ulong)
                                        .wrapping_mul(
                                            ::core::mem::size_of::<uint64>() as libc::c_ulong
                                        ),
                                ) as uint64 as uint64;
                                sp = (sp as libc::c_ulong).wrapping_sub(
                                    sp.wrapping_rem(16 as libc::c_int as libc::c_ulong),
                                ) as uint64 as uint64;
                                if sp >= stackbase
                                    && copyout(
                                        pagetable,
                                        sp,
                                        ustack.as_mut_ptr() as *mut libc::c_char,
                                        argc.wrapping_add(1 as libc::c_int as libc::c_ulong)
                                            .wrapping_mul(
                                                ::core::mem::size_of::<uint64>() as libc::c_ulong
                                            ),
                                    ) >= 0 as libc::c_int
                                {
                                    // arguments to user main(argc, argv)
                                    // argc is returned via the system call return
                                    // value, which goes in a0.
                                    (*(*p).tf).a1 = sp;
                                    // Save program name for debugging.
                                    s = path;
                                    last = s;
                                    while *s != 0 {
                                        if *s as libc::c_int == '/' as i32 {
                                            last = s.offset(1 as libc::c_int as isize)
                                        }
                                        s = s.offset(1)
                                    }
                                    safestrcpy(
                                        (*p).name.as_mut_ptr(),
                                        last,
                                        ::core::mem::size_of::<[libc::c_char; 16]>()
                                            as libc::c_ulong
                                            as libc::c_int,
                                    );
                                    // Commit to the user image.
                                    oldpagetable = (*p).pagetable; // initial program counter = main
                                    (*p).pagetable = pagetable; // initial stack pointer
                                    (*p).sz = sz; // this ends up in a0, the first argument to main(argc, argv)
                                    (*(*p).tf).epc = elf.entry;
                                    (*(*p).tf).sp = sp;
                                    proc_freepagetable(oldpagetable, oldsz);
                                    return argc as libc::c_int;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if !pagetable.is_null() {
        proc_freepagetable(pagetable, sz);
    }
    if !ip.is_null() {
        iunlockput(ip);
        end_op();
    }
    -(1 as libc::c_int)
}
// Load a program segment into pagetable at virtual address va.
// va must be page-aligned
// and the pages from va to va+sz must already be mapped.
// Returns 0 on success, -1 on failure.
unsafe extern "C" fn loadseg(
    mut pagetable: pagetable_t,
    mut va: uint64,
    mut ip: *mut inode,
    mut offset: uint,
    mut sz: uint,
) -> libc::c_int {
    let mut i: uint = 0;
    let mut n: uint = 0;
    let mut pa: uint64 = 0;
    if va.wrapping_rem(PGSIZE as libc::c_ulong) != 0 as libc::c_int as libc::c_ulong {
        panic(
            b"loadseg: va must be page aligned\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    i = 0 as libc::c_int as uint;
    while i < sz {
        pa = walkaddr(pagetable, va.wrapping_add(i as libc::c_ulong));
        if pa == 0 as libc::c_int as libc::c_ulong {
            panic(
                b"loadseg: address should exist\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        if sz.wrapping_sub(i) < PGSIZE as libc::c_uint {
            n = sz.wrapping_sub(i)
        } else {
            n = PGSIZE as uint
        }
        if readi(ip, 0 as libc::c_int, pa, offset.wrapping_add(i), n) as libc::c_uint != n {
            return -(1 as libc::c_int);
        }
        i = (i as libc::c_uint).wrapping_add(PGSIZE as libc::c_uint) as uint as uint
    }
    0 as libc::c_int
}
