use crate::{
    elf::{ElfHdr, ProgHdr, ELF_MAGIC, ELF_PROG_LOAD},
    file::Inode,
    fs::namei,
    log::{begin_op, end_op},
    param::MAXARG,
    printf::panic,
    proc::{myproc, proc_freepagetable, proc_pagetable, Proc},
    riscv::{PagetableT, PGSIZE},
    string::{safestrcpy, strlen},
    vm::{copyout, uvmalloc, uvmclear, walkaddr},
};
use core::ptr;

pub unsafe fn exec(path: *mut u8, argv: *mut *mut u8) -> i32 {
    let mut sz: usize = 0;
    let mut ustack: [usize; MAXARG + 1] = [0; MAXARG + 1];
    let mut elf: ElfHdr = Default::default();
    let mut ph: ProgHdr = Default::default();
    let mut pagetable: PagetableT = 0 as PagetableT;
    let mut p: *mut Proc = myproc();

    begin_op();
    let mut ip: *mut Inode = namei(path);
    if ip.is_null() {
        end_op();
        return -1;
    }
    (*ip).lock();

    let _op = scopeguard::guard((pagetable, sz, ip), |(pagetable, sz, ip)| {
        if !pagetable.is_null() {
            proc_freepagetable(pagetable, sz);
        }
        if !ip.is_null() {
            (*ip).unlockput();
            end_op();
        }
    });

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
            for i in 0..elf.phnum as usize {
                let off = elf
                    .phoff
                    .wrapping_add(i * ::core::mem::size_of::<ProgHdr>());

                if (*ip).read(
                    0,
                    &mut ph as *mut ProgHdr as usize,
                    off as u32,
                    ::core::mem::size_of::<ProgHdr>() as u32,
                ) as usize
                    != ::core::mem::size_of::<ProgHdr>()
                {
                    return -1;
                }
                if ph.typ == ELF_PROG_LOAD {
                    if ph.memsz < ph.filesz {
                        return -1;
                    }
                    if ph.vaddr.wrapping_add(ph.memsz) < ph.vaddr {
                        return -1;
                    }
                    sz = uvmalloc(pagetable, sz, ph.vaddr.wrapping_add(ph.memsz));
                    if sz == 0 {
                        return -1;
                    }
                    if ph.vaddr.wrapping_rem(PGSIZE) != 0 {
                        return -1;
                    }
                    if loadseg(pagetable, ph.vaddr, ip, ph.off as u32, ph.filesz as u32).is_err() {
                        return -1;
                    }
                }
            }
            (*ip).unlockput();
            core::mem::forget(_op);
            end_op();
            ip = ptr::null_mut();

            p = myproc();
            let oldsz: usize = (*p).sz;

            // Allocate two pages at the next page boundary.
            // Use the second as the user stack.
            sz = sz.wrapping_add(PGSIZE).wrapping_sub(1) & !PGSIZE.wrapping_sub(1);
            sz = uvmalloc(pagetable, sz, sz.wrapping_add(2usize.wrapping_mul(PGSIZE)));
            let _op = scopeguard::guard((pagetable, sz, ip), |(pagetable, sz, ip)| {
                if !pagetable.is_null() {
                    proc_freepagetable(pagetable, sz);
                }
                if !ip.is_null() {
                    (*ip).unlockput();
                    end_op();
                }
            });
            if sz != 0 {
                uvmclear(pagetable, sz.wrapping_sub(2usize.wrapping_mul(PGSIZE)));
                let mut sp: usize = sz;
                let stackbase: usize = sp.wrapping_sub(PGSIZE);

                // Push argument strings, prepare rest of stack in ustack.
                let mut argc: usize = 0;
                loop {
                    if (*argv.add(argc)).is_null() {
                        break;
                    }
                    if argc >= MAXARG {
                        return -1;
                    }
                    sp = sp.wrapping_sub((strlen(*argv.add(argc)) + 1) as usize);

                    // riscv sp must be 16-byte aligned
                    sp = sp.wrapping_sub(sp.wrapping_rem(16));
                    if sp < stackbase {
                        return -1;
                    }
                    if copyout(
                        pagetable,
                        sp,
                        *argv.add(argc),
                        (strlen(*argv.add(argc)) + 1) as usize,
                    ) < 0
                    {
                        return -1;
                    }
                    ustack[argc] = sp;
                    argc = argc.wrapping_add(1)
                }
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
                        ustack.as_mut_ptr() as *mut u8,
                        argc.wrapping_add(1)
                            .wrapping_mul(::core::mem::size_of::<usize>()),
                    ) >= 0
                {
                    core::mem::forget(_op);
                    // arguments to user main(argc, argv)
                    // argc is returned via the system call return
                    // value, which goes in a0.
                    (*(*p).tf).a1 = sp;

                    // Save program name for debugging.
                    let mut s: *mut u8 = path;
                    let mut last: *mut u8 = s;
                    while *s != 0 {
                        if *s as i32 == '/' as i32 {
                            last = s.offset(1isize)
                        }
                        s = s.offset(1)
                    }
                    safestrcpy(
                        (*p).name.as_mut_ptr(),
                        last,
                        ::core::mem::size_of::<[u8; 16]>() as i32,
                    );

                    // Commit to the user image.
                    let oldpagetable: PagetableT = (*p).pagetable;
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
    -1
}

/// Load a program segment into pagetable at virtual address va.
/// va must be page-aligned
/// and the pages from va to va+sz must already be mapped.
///
/// Returns `Ok(())` on success, `Err(())` on failure.
unsafe fn loadseg(
    pagetable: PagetableT,
    va: usize,
    ip: *mut Inode,
    offset: u32,
    sz: u32,
) -> Result<(), ()> {
    if va.wrapping_rem(PGSIZE) != 0 {
        panic(b"loadseg: va must be page aligned\x00" as *const u8 as *mut u8);
    }

    for i in num_iter::range_step(0, sz, PGSIZE as _) {
        let pa = walkaddr(pagetable, va.wrapping_add(i as usize));
        if pa == 0 {
            panic(b"loadseg: address should exist\x00" as *const u8 as *mut u8);
        }

        let n = if sz.wrapping_sub(i) < PGSIZE as u32 {
            sz.wrapping_sub(i)
        } else {
            PGSIZE as u32
        };

        if (*ip).read(0, pa, offset.wrapping_add(i), n) as u32 != n {
            return Err(());
        }
    }

    Ok(())
}
