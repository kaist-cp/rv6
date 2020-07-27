use crate::{
    file::inode,
    fs::{ilock, iunlockput, namei, readi},
    log::{begin_op, end_op},
    printf::panic,
    proc::{myproc, proc_0, proc_freepagetable, proc_pagetable},
    string::{safestrcpy, strlen},
    vm::{copyout, uvmalloc, uvmclear, walkaddr},
};
use core::ptr;
pub type pde_t = u64;
pub type pagetable_t = *mut u64;
/// "\x7FELF" in little endian
/// File header
#[derive(Copy, Clone)]
#[repr(C)]
pub struct elfhdr {
    pub magic: u32,
    pub elf: [u8; 12],
    pub type_0: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}
/// Program section header
#[derive(Copy, Clone)]
#[repr(C)]
pub struct proghdr {
    pub type_0: u32,
    pub flags: u32,
    pub off: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}
// maximum number of processes
// maximum number of CPUs
// open files per process
// open files per system
// maximum number of active i-nodes
// maximum major device number
// device number of file system root disk
pub const MAXARG: i32 = 32;
pub const PGSIZE: i32 = 4096;
// Format of an ELF executable file
pub const ELF_MAGIC: u32 = 0x464c457f;
// Values for Proghdr type
pub const ELF_PROG_LOAD: i32 = 1;
// exec.c
#[no_mangle]
pub unsafe extern "C" fn exec(mut path: *mut i8, mut argv: *mut *mut i8) -> i32 {
    let mut oldsz: u64 = 0;
    let mut current_block: u64;
    let mut s: *mut i8 = ptr::null_mut();
    let mut last: *mut i8 = ptr::null_mut();
    let mut i: i32 = 0;
    let mut off: i32 = 0;
    let mut argc: u64 = 0;
    let mut sz: u64 = 0;
    let mut sp: u64 = 0;
    let mut ustack: [u64; 33] = [0; 33];
    let mut stackbase: u64 = 0;
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
        return -(1 as i32);
    }
    ilock(ip);
    // Check ELF header
    if readi(
        ip,
        0 as i32,
        &mut elf as *mut elfhdr as u64,
        0 as i32 as u32,
        ::core::mem::size_of::<elfhdr>() as u64 as u32,
    ) as u64
        == ::core::mem::size_of::<elfhdr>() as u64
        && elf.magic == ELF_MAGIC
    {
        pagetable = proc_pagetable(p);
        if !pagetable.is_null() {
            // Load program into memory.
            sz = 0 as i32 as u64;
            i = 0 as i32;
            off = elf.phoff as i32;
            loop {
                if i >= elf.phnum as i32 {
                    current_block = 15768484401365413375;
                    break;
                }
                if readi(
                    ip,
                    0 as i32,
                    &mut ph as *mut proghdr as u64,
                    off as u32,
                    ::core::mem::size_of::<proghdr>() as u64 as u32,
                ) as u64
                    != ::core::mem::size_of::<proghdr>() as u64
                {
                    current_block = 7080392026674647309;
                    break;
                }
                if ph.type_0 == ELF_PROG_LOAD as u32 {
                    if ph.memsz < ph.filesz {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if ph.vaddr.wrapping_add(ph.memsz) < ph.vaddr {
                        current_block = 7080392026674647309;
                        break;
                    }
                    sz = uvmalloc(pagetable, sz, ph.vaddr.wrapping_add(ph.memsz));
                    if sz == 0 as i32 as u64 {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if ph.vaddr.wrapping_rem(PGSIZE as u64) != 0 as i32 as u64 {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if loadseg(pagetable, ph.vaddr, ip, ph.off as u32, ph.filesz as u32) < 0 as i32
                    {
                        current_block = 7080392026674647309;
                        break;
                    }
                }
                i += 1;
                off = (off as u64).wrapping_add(::core::mem::size_of::<proghdr>() as u64) as i32
                    as i32
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
                    sz = sz.wrapping_add(PGSIZE as u64).wrapping_sub(1 as i32 as u64)
                        & !(PGSIZE - 1 as i32) as u64;
                    sz = uvmalloc(pagetable, sz, sz.wrapping_add((2 as i32 * PGSIZE) as u64));
                    if sz != 0 as i32 as u64 {
                        uvmclear(pagetable, sz.wrapping_sub((2 as i32 * PGSIZE) as u64));
                        sp = sz;
                        stackbase = sp.wrapping_sub(PGSIZE as u64);
                        // Push argument strings, prepare rest of stack in ustack.
                        argc = 0 as i32 as u64; // riscv sp must be 16-byte aligned
                        loop {
                            if (*argv.offset(argc as isize)).is_null() {
                                current_block = 4567019141635105728;
                                break;
                            }
                            if argc >= MAXARG as u64 {
                                current_block = 7080392026674647309;
                                break;
                            }
                            sp = (sp as u64).wrapping_sub(
                                (strlen(*argv.offset(argc as isize)) + 1 as i32) as u64,
                            ) as u64 as u64;
                            sp = (sp as u64).wrapping_sub(sp.wrapping_rem(16 as i32 as u64)) as u64
                                as u64;
                            if sp < stackbase {
                                current_block = 7080392026674647309;
                                break;
                            }
                            if copyout(
                                pagetable,
                                sp,
                                *argv.offset(argc as isize),
                                (strlen(*argv.offset(argc as isize)) + 1 as i32) as u64,
                            ) < 0 as i32
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
                                ustack[argc as usize] = 0 as i32 as u64;
                                // push the array of argv[] pointers.
                                sp = (sp as u64).wrapping_sub(
                                    argc.wrapping_add(1 as i32 as u64)
                                        .wrapping_mul(::core::mem::size_of::<u64>() as u64),
                                ) as u64 as u64;
                                sp = (sp as u64).wrapping_sub(sp.wrapping_rem(16 as i32 as u64))
                                    as u64 as u64;
                                if sp >= stackbase
                                    && copyout(
                                        pagetable,
                                        sp,
                                        ustack.as_mut_ptr() as *mut i8,
                                        argc.wrapping_add(1 as i32 as u64)
                                            .wrapping_mul(::core::mem::size_of::<u64>() as u64),
                                    ) >= 0 as i32
                                {
                                    // arguments to user main(argc, argv)
                                    // argc is returned via the system call return
                                    // value, which goes in a0.
                                    (*(*p).tf).a1 = sp;
                                    // Save program name for debugging.
                                    s = path;
                                    last = s;
                                    while *s != 0 {
                                        if *s as i32 == '/' as i32 {
                                            last = s.offset(1 as i32 as isize)
                                        }
                                        s = s.offset(1)
                                    }
                                    safestrcpy(
                                        (*p).name.as_mut_ptr(),
                                        last,
                                        ::core::mem::size_of::<[i8; 16]>() as u64 as i32,
                                    );
                                    // Commit to the user image.
                                    oldpagetable = (*p).pagetable; // initial program counter = main
                                    (*p).pagetable = pagetable; // initial stack pointer
                                    (*p).sz = sz; // this ends up in a0, the first argument to main(argc, argv)
                                    (*(*p).tf).epc = elf.entry;
                                    (*(*p).tf).sp = sp;
                                    proc_freepagetable(oldpagetable, oldsz);
                                    return argc as i32;
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
    -(1 as i32)
}
/// Load a program segment into pagetable at virtual address va.
/// va must be page-aligned
/// and the pages from va to va+sz must already be mapped.
/// Returns 0 on success, -1 on failure.
unsafe extern "C" fn loadseg(
    mut pagetable: pagetable_t,
    mut va: u64,
    mut ip: *mut inode,
    mut offset: u32,
    mut sz: u32,
) -> i32 {
    let mut i: u32 = 0;
    let mut n: u32 = 0;
    let mut pa: u64 = 0;
    if va.wrapping_rem(PGSIZE as u64) != 0 as i32 as u64 {
        panic(b"loadseg: va must be page aligned\x00" as *const u8 as *mut i8);
    }
    i = 0 as i32 as u32;
    while i < sz {
        pa = walkaddr(pagetable, va.wrapping_add(i as u64));
        if pa == 0 as i32 as u64 {
            panic(b"loadseg: address should exist\x00" as *const u8 as *mut i8);
        }
        if sz.wrapping_sub(i) < PGSIZE as u32 {
            n = sz.wrapping_sub(i)
        } else {
            n = PGSIZE as u32
        }
        if readi(ip, 0 as i32, pa, offset.wrapping_add(i), n) as u32 != n {
            return -(1 as i32);
        }
        i = (i as u32).wrapping_add(PGSIZE as u32) as u32 as u32
    }
    0 as i32
}
