use crate::libc;
use crate::{
    elf::{ElfHdr, ProgHdr, ELF_MAGIC, ELF_PROG_LOAD},
    file::Inode,
    fs::namei,
    log::{begin_op, end_op},
    param::MAXARG,
    printf::panic,
    proc::{myproc, proc_0, proc_freepagetable, proc_pagetable},
    riscv::{pagetable_t, PGSIZE},
    string::{safestrcpy, strlen},
    vm::{copyout, uvmalloc, uvmclear, walkaddr},
};
use core::ptr;

pub unsafe fn exec(mut path: *mut libc::c_char, mut argv: *mut *mut libc::c_char) -> i32 {
    let mut oldsz: usize = 0;
    let mut current_block: usize;
    let mut s: *mut libc::c_char = ptr::null_mut();
    let mut last: *mut libc::c_char = ptr::null_mut();
    let mut i: i32 = 0;
    let mut off: i32 = 0;
    let mut argc: usize = 0;
    let mut sz: usize = 0;
    let mut sp: usize = 0;
    let mut ustack: [usize; MAXARG + 1] = [0; MAXARG + 1];
    let mut stackbase: usize = 0;
    let mut elf: ElfHdr = Default::default();
    let mut ip: *mut Inode = ptr::null_mut();
    let mut ph: ProgHdr = Default::default();
    let mut pagetable: pagetable_t = 0 as pagetable_t;
    let mut oldpagetable: pagetable_t = ptr::null_mut();
    let mut p: *mut proc_0 = myproc();
    begin_op();
    ip = namei(path);
    if ip.is_null() {
        end_op();
        return -1;
    }
    (*ip).lock();

    // Check ELF header
    if (*ip).read(
        0,
        &mut elf as *mut ElfHdr as usize,
        0,
        ::core::mem::size_of::<ElfHdr>() as u32,
    ) as usize
        == ::core::mem::size_of::<ElfHdr>()
        && elf.magic == ELF_MAGIC
    {
        pagetable = proc_pagetable(p);
        if !pagetable.is_null() {
            // Load program into memory.
            sz = 0;
            i = 0;
            off = elf.phoff as i32;
            loop {
                if i >= elf.phnum as i32 {
                    current_block = 15768484401365413375;
                    break;
                }
                if (*ip).read(
                    0,
                    &mut ph as *mut ProgHdr as usize,
                    off as u32,
                    ::core::mem::size_of::<ProgHdr>() as u32,
                ) as usize
                    != ::core::mem::size_of::<ProgHdr>()
                {
                    current_block = 7080392026674647309;
                    break;
                }
                if ph.typ == ELF_PROG_LOAD as u32 {
                    if ph.memsz < ph.filesz {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if ph.vaddr.wrapping_add(ph.memsz) < ph.vaddr {
                        current_block = 7080392026674647309;
                        break;
                    }
                    sz = uvmalloc(pagetable, sz, ph.vaddr.wrapping_add(ph.memsz));
                    if sz == 0 {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if ph.vaddr.wrapping_rem(PGSIZE as usize) != 0 {
                        current_block = 7080392026674647309;
                        break;
                    }
                    if loadseg(pagetable, ph.vaddr, ip, ph.off as u32, ph.filesz as u32) < 0 {
                        current_block = 7080392026674647309;
                        break;
                    }
                }
                i += 1;
                off = (off as usize).wrapping_add(::core::mem::size_of::<ProgHdr>()) as i32
            }
            match current_block {
                7080392026674647309 => {}
                _ => {
                    (*ip).unlockput();
                    end_op();
                    ip = ptr::null_mut();
                    p = myproc();
                    oldsz = (*p).sz;

                    // Allocate two pages at the next page boundary.
                    // Use the second as the user stack.
                    sz = sz.wrapping_add(PGSIZE as usize).wrapping_sub(1) & !(PGSIZE - 1) as usize;
                    sz = uvmalloc(pagetable, sz, sz.wrapping_add((2 * PGSIZE) as usize));
                    if sz != 0 {
                        uvmclear(pagetable, sz.wrapping_sub((2 * PGSIZE) as usize));
                        sp = sz;
                        stackbase = sp.wrapping_sub(PGSIZE as usize);

                        // Push argument strings, prepare rest of stack in ustack.
                        argc = 0;
                        loop {
                            if (*argv.add(argc)).is_null() {
                                current_block = 4567019141635105728;
                                break;
                            }
                            if argc >= MAXARG {
                                current_block = 7080392026674647309;
                                break;
                            }
                            sp = sp.wrapping_sub((strlen(*argv.add(argc)) + 1) as usize);

                            // riscv sp must be 16-byte aligned
                            sp = sp.wrapping_sub(sp.wrapping_rem(16));
                            if sp < stackbase {
                                current_block = 7080392026674647309;
                                break;
                            }
                            if copyout(
                                pagetable,
                                sp,
                                *argv.add(argc),
                                (strlen(*argv.add(argc)) + 1) as usize,
                            ) < 0
                            {
                                current_block = 7080392026674647309;
                                break;
                            }
                            ustack[argc] = sp;
                            argc = argc.wrapping_add(1)
                        }
                        match current_block {
                            7080392026674647309 => {}
                            _ => {
                                ustack[argc] = 0;

                                // push the array of argv[] pointers.
                                sp = sp.wrapping_sub(
                                    argc.wrapping_add(1)
                                        .wrapping_mul(::core::mem::size_of::<usize>()),
                                );
                                sp = sp.wrapping_sub(sp.wrapping_rem(16));
                                if sp >= stackbase
                                    && copyout(
                                        pagetable,
                                        sp,
                                        ustack.as_mut_ptr() as *mut libc::c_char,
                                        argc.wrapping_add(1)
                                            .wrapping_mul(::core::mem::size_of::<usize>()),
                                    ) >= 0
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
                                            last = s.offset(1isize)
                                        }
                                        s = s.offset(1)
                                    }
                                    safestrcpy(
                                        (*p).name.as_mut_ptr(),
                                        last,
                                        ::core::mem::size_of::<[libc::c_char; 16]>() as i32,
                                    );

                                    // Commit to the user image.
                                    oldpagetable = (*p).pagetable;
                                    (*p).pagetable = pagetable;
                                    (*p).sz = sz;

                                    // initial program counter = main
                                    (*(*p).tf).epc = elf.entry;

                                    // initial stack pointer
                                    (*(*p).tf).sp = sp;
                                    proc_freepagetable(oldpagetable, oldsz);

                                    // this ends up in a0, the first argument to main(argc, argv)
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
        (*ip).unlockput();
        end_op();
    }
    -1
}

/// Load a program segment into pagetable at virtual address va.
/// va must be page-aligned
/// and the pages from va to va+sz must already be mapped.
/// Returns 0 on success, -1 on failure.
unsafe fn loadseg(
    mut pagetable: pagetable_t,
    mut va: usize,
    mut ip: *mut Inode,
    mut offset: u32,
    mut sz: u32,
) -> i32 {
    let mut i: u32 = 0;
    if va.wrapping_rem(PGSIZE as usize) != 0 {
        panic(
            b"loadseg: va must be page aligned\x00" as *const u8 as *const libc::c_char
                as *mut libc::c_char,
        );
    }
    while i < sz {
        let pa = walkaddr(pagetable, va.wrapping_add(i as usize));
        if pa == 0 {
            panic(
                b"loadseg: address should exist\x00" as *const u8 as *const libc::c_char
                    as *mut libc::c_char,
            );
        }
        let n = if sz.wrapping_sub(i) < PGSIZE as u32 {
            sz.wrapping_sub(i)
        } else {
            PGSIZE as u32
        };
        if (*ip).read(0, pa, offset.wrapping_add(i), n) as u32 != n {
            return -1;
        }
        i = (i as u32).wrapping_add(PGSIZE as u32) as u32 as u32
    }
    0
}
