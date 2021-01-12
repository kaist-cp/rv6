#![allow(clippy::unit_arg)]

use crate::{
    fs::{InodeGuard, Path},
    kernel::Kernel,
    ok_or,
    param::MAXARG,
    proc::{myproc, proc_freepagetable, proc_pagetable, Proc},
    riscv::PGSIZE,
    string::{safestrcpy, strlen},
    vm::{KVAddr, PageTable, UVAddr, VAddr},
};
use core::{cmp, mem, slice};

/// "\x7FELF" in little endian
const ELF_MAGIC: u32 = 0x464c457f;

/// Values for Proghdr type
const ELF_PROG_LOAD: u32 = 1;

/// File header
#[derive(Default, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct ElfHdr {
    /// must equal ELF_MAGIC
    magic: u32,
    elf: [u8; 12],
    typ: u16,
    machine: u16,
    version: u32,
    entry: usize,
    phoff: usize,
    shoff: usize,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

bitflags! {
    /// Flag bits for ProgHdr flags
    #[repr(C)]
    struct ProgFlags: u32 {
        const EXEC = 1;
        const WRITE = 2;
        const READ = 4;
    }
}

impl Default for ProgFlags {
    fn default() -> Self {
        Self::from_bits_truncate(0)
    }
}

/// Program section header
#[derive(Default, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
struct ProgHdr {
    typ: u32,
    flags: ProgFlags,
    off: usize,
    vaddr: usize,
    paddr: usize,
    filesz: usize,
    memsz: usize,
    align: usize,
}

impl ElfHdr {
    pub fn is_valid(&self) -> bool {
        self.magic == ELF_MAGIC
    }
}

impl ProgHdr {
    pub fn is_prog_load(&self) -> bool {
        self.typ == ELF_PROG_LOAD
    }
}

impl Kernel {
    pub unsafe fn exec(&self, path: &Path, argv: &[*mut u8]) -> Result<usize, ()> {
        let sz: usize = 0;
        let mut ustack = [0usize; MAXARG + 1];
        let mut elf: ElfHdr = Default::default();
        let mut ph: ProgHdr = Default::default();
        let mut p: *mut Proc = myproc();
        let mut data = &mut *(*p).data.get();

        let tx = self.file_system.begin_transaction();
        let ptr = ok_or!(path.namei(&tx), {
            return Err(());
        });
        let mut ip = ptr.lock(&tx);

        // Check ELF header
        let bytes_read = ip.read(
            KVAddr::new(&mut elf as *mut _ as _),
            0,
            mem::size_of::<ElfHdr>() as _,
        )?;
        if !(bytes_read == mem::size_of::<ElfHdr>() && elf.is_valid()) {
            return Err(());
        }

        let pt = proc_pagetable(p)?;

        let mut ptable_guard = scopeguard::guard((pt, sz), |(mut pt, sz)| {
            proc_freepagetable(&mut pt, sz);
        });

        let (pt, sz) = &mut *ptable_guard;
        // Load program into memory.
        *sz = 0;
        for i in 0..elf.phnum as usize {
            let off = elf.phoff.wrapping_add(i * mem::size_of::<ProgHdr>());

            let bytes_read = ip.read(
                KVAddr::new(&mut ph as *mut ProgHdr as usize),
                off as u32,
                mem::size_of::<ProgHdr>() as u32,
            )?;
            if bytes_read != mem::size_of::<ProgHdr>() {
                return Err(());
            }
            if ph.is_prog_load() {
                if ph.memsz < ph.filesz {
                    return Err(());
                }
                if ph.vaddr.wrapping_add(ph.memsz) < ph.vaddr {
                    return Err(());
                }
                let sz1 = pt.uvmalloc(*sz, ph.vaddr.wrapping_add(ph.memsz))?;
                *sz = sz1;
                if ph.vaddr.wrapping_rem(PGSIZE) != 0 {
                    return Err(());
                }
                loadseg(
                    pt,
                    UVAddr::new(ph.vaddr),
                    &mut ip,
                    ph.off as u32,
                    ph.filesz as u32,
                )?;
            }
        }
        drop(ip);

        p = myproc();
        let oldsz: usize = data.sz;

        // Allocate two pages at the next page boundary.
        // Use the second as the user stack.
        *sz = sz.wrapping_add(PGSIZE).wrapping_sub(1) & !PGSIZE.wrapping_sub(1);

        let sz1 = pt.uvmalloc(*sz, sz.wrapping_add(2usize.wrapping_mul(PGSIZE)))?;
        *sz = sz1;
        pt.uvmclear(UVAddr::new(sz.wrapping_sub(2usize.wrapping_mul(PGSIZE))));
        let mut sp: usize = *sz;
        let stackbase: usize = sp.wrapping_sub(PGSIZE);

        // Push argument strings, prepare rest of stack in ustack.
        let mut argc: usize = 0;
        loop {
            if argv[argc].is_null() {
                break;
            }
            if argc >= MAXARG {
                return Err(());
            }
            sp = sp.wrapping_sub((strlen(argv[argc]) + 1) as usize);

            // riscv sp must be 16-byte aligned
            sp = sp.wrapping_sub(sp.wrapping_rem(16));
            if sp < stackbase {
                return Err(());
            }
            pt.copyout(
                UVAddr::new(sp),
                slice::from_raw_parts_mut(argv[argc], (strlen(argv[argc]) + 1) as usize),
            )?;
            ustack[argc] = sp;
            argc = argc.wrapping_add(1)
        }
        ustack[argc] = 0;

        // push the array of argv[] pointers.
        sp = sp.wrapping_sub(argc.wrapping_add(1).wrapping_mul(mem::size_of::<usize>()));
        sp = sp.wrapping_sub(sp.wrapping_rem(16));

        if sp >= stackbase
            && pt
                .copyout(
                    UVAddr::new(sp),
                    slice::from_raw_parts_mut(
                        ustack.as_mut_ptr() as *mut u8,
                        argc.wrapping_add(1).wrapping_mul(mem::size_of::<usize>()),
                    ),
                )
                .is_ok()
        {
            let (pt, sz) = scopeguard::ScopeGuard::into_inner(ptable_guard);
            // arguments to user main(argc, argv)
            // argc is returned via the system call return
            // value, which goes in a0.
            (*data.trapframe).a1 = sp;

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
                mem::size_of::<[u8; 16]>() as i32,
            );

            // Commit to the user image.
            let mut oldpagetable = mem::replace(&mut data.pagetable, pt);
            data.sz = sz;

            // initial program counter = main
            (*data.trapframe).epc = elf.entry;

            // initial stack pointer
            (*data.trapframe).sp = sp;
            proc_freepagetable(&mut oldpagetable, oldsz);

            // this ends up in a0, the first argument to main(argc, argv)
            return Ok(argc);
        }
        Err(())
    }
}

/// Load a program segment into pagetable at virtual address va.
/// va must be page-aligned
/// and the pages from va to va+sz must already be mapped.
///
/// Returns `Ok(())` on success, `Err(())` on failure.
unsafe fn loadseg(
    pagetable: &mut PageTable<UVAddr>,
    va: UVAddr,
    ip: &mut InodeGuard<'_>,
    offset: u32,
    sz: u32,
) -> Result<(), ()> {
    assert!(va.is_page_aligned(), "loadseg: va must be page aligned");

    for i in num_iter::range_step(0, sz, PGSIZE as _) {
        let pa = pagetable
            .walkaddr(va + i as usize)
            .expect("loadseg: address should exist")
            .into_usize();

        let n = cmp::min(sz - i, PGSIZE as u32);

        let bytes_read = ip.read(KVAddr::new(pa), offset.wrapping_add(i), n)?;
        if bytes_read as u32 != n {
            return Err(());
        }
    }

    Ok(())
}
