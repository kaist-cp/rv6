use crate::{
    elf::{ElfHdr, ProgHdr, ELF_MAGIC, ELF_PROG_LOAD},
    file::Inode,
    fs::Path,
    log::{begin_op, end_op},
    ok_or,
    param::MAXARG,
    proc::{myproc, proc_freepagetable, proc_pagetable, Proc},
    riscv::PGSIZE,
    string::{safestrcpy, strlen},
    vm::PageTable,
};

pub unsafe fn exec(path: &Path, argv: *mut *mut u8) -> i32 {
    let sz: usize = 0;
    let mut ustack: [usize; MAXARG + 1] = [0; MAXARG + 1];
    let mut elf: ElfHdr = Default::default();
    let mut ph: ProgHdr = Default::default();
    let mut p: *mut Proc = myproc();

    begin_op();
    let ip = ok_or!(path.namei(), {
        end_op();
        return -1;
    });

    (*ip).lock();

    let mut ip = scopeguard::guard(ip, |ip| {
        (*ip).unlockput();
        end_op();
    });

    // Check ELF header
    if !((**ip).read(
        0,
        &mut elf as *mut ElfHdr as usize,
        0,
        ::core::mem::size_of::<ElfHdr>() as u32,
    ) as usize
        == ::core::mem::size_of::<ElfHdr>()
        && elf.magic == ELF_MAGIC)
    {
        return -1;
    }

    let pt = proc_pagetable(p);
    if pt.is_null() {
        return -1;
    }

    let mut ptable_guard = scopeguard::guard((pt, sz), |(mut pt, sz)| {
        proc_freepagetable(&mut pt, sz);
    });

    let (pt, sz) = &mut *ptable_guard;
    // Load program into memory.
    *sz = 0;
    for i in 0..elf.phnum as usize {
        let off = elf
            .phoff
            .wrapping_add(i * ::core::mem::size_of::<ProgHdr>());

        if (**ip).read(
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
            let sz_op = pt.uvmalloc(*sz, ph.vaddr.wrapping_add(ph.memsz));
            if sz_op.is_err() {
                return -1;
            }
            *sz = sz_op.unwrap();
            if ph.vaddr.wrapping_rem(PGSIZE) != 0 {
                return -1;
            }
            if loadseg(pt, ph.vaddr, *ip, ph.off as u32, ph.filesz as u32).is_err() {
                return -1;
            }
        }
    }
    (**ip).unlockput();
    core::mem::forget(ip);
    end_op();

    p = myproc();
    let oldsz: usize = (*p).sz;

    // Allocate two pages at the next page boundary.
    // Use the second as the user stack.
    *sz = sz.wrapping_add(PGSIZE).wrapping_sub(1) & !PGSIZE.wrapping_sub(1);
    let sz_op = pt.uvmalloc(*sz, sz.wrapping_add(2usize.wrapping_mul(PGSIZE)));

    if sz_op.is_err() {
        return -1;
    }

    *sz = sz_op.unwrap();
    pt.uvmclear(sz.wrapping_sub(2usize.wrapping_mul(PGSIZE)));
    let mut sp: usize = *sz;
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
        if pt
            .copyout(sp, *argv.add(argc), (strlen(*argv.add(argc)) + 1) as usize)
            .is_err()
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
        && pt
            .copyout(
                sp,
                ustack.as_mut_ptr() as *mut u8,
                argc.wrapping_add(1)
                    .wrapping_mul(::core::mem::size_of::<usize>()),
            )
            .is_ok()
    {
        let (pt, sz) = scopeguard::ScopeGuard::into_inner(ptable_guard);
        // arguments to user main(argc, argv)
        // argc is returned via the system call return
        // value, which goes in a0.
        (*(*p).tf).a1 = sp;

        // Save program name for debugging.
        let mut s = path.as_bytes().as_ptr();
        let mut last = s;
        while *s != 0 {
            if *s as i32 == '/' as i32 {
                last = s.offset(1)
            }
            s = s.offset(1)
        }
        safestrcpy(
            (*p).name.as_mut_ptr(),
            last,
            ::core::mem::size_of::<[u8; 16]>() as i32,
        );

        // Commit to the user image.
        let mut oldpagetable = core::mem::replace((*p).pagetable.assume_init_mut(), pt);
        (*p).sz = sz;

        // initial program counter = main
        (*(*p).tf).epc = elf.entry;

        // initial stack pointer
        (*(*p).tf).sp = sp;
        proc_freepagetable(&mut oldpagetable, oldsz);

        // this ends up in a0, the first argument to main(argc, argv)
        return argc as i32;
    }
    -1
}

/// Load a program segment into pagetable at virtual address va.
/// va must be page-aligned
/// and the pages from va to va+sz must already be mapped.
///
/// Returns `Ok(())` on success, `Err(())` on failure.
unsafe fn loadseg(
    pagetable: &mut PageTable,
    va: usize,
    ip: *mut Inode,
    offset: u32,
    sz: u32,
) -> Result<(), ()> {
    if va.wrapping_rem(PGSIZE) != 0 {
        panic!("loadseg: va msut be page aligned");
    }

    for i in num_iter::range_step(0, sz, PGSIZE as _) {
        let pa = pagetable
            .walkaddr(va.wrapping_add(i as usize))
            .expect("loadseg: address should exist");

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
