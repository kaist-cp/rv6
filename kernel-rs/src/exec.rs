#![allow(clippy::unit_arg)]

use crate::{
    fs::{InodeGuard, Path},
    kernel::Kernel,
    page::Page,
    param::MAXARG,
    proc::{myproc, Proc},
    riscv::{pgroundup, PGSIZE},
    vm::{KVAddr, PageTable, UVAddr, VAddr},
};
use core::{cmp, mem};
use cstr_core::CStr;

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
    pub unsafe fn exec(&self, path: &Path, argv: &[Page]) -> Result<usize, ()> {
        if argv.len() > MAXARG {
            return Err(());
        }

        // TODO(rv6)
        // The method namei can drop inodes. If namei succeeds, its return
        // value, ptr, will be dropped when this method returns. Deallocation
        // of an inode may cause disk write operations, so we must begin a
        // transaction here.
        // https://github.com/kaist-cp/rv6/issues/290
        let tx = self.file_system.begin_transaction();
        let ptr = path.namei()?;
        let mut ip = ptr.lock();

        // Check ELF header
        let mut elf: ElfHdr = Default::default();
        let bytes_read = ip.read(
            KVAddr::new(&mut elf as *mut _ as _),
            0,
            mem::size_of::<ElfHdr>() as _,
        )?;
        if !(bytes_read == mem::size_of::<ElfHdr>() && elf.is_valid()) {
            return Err(());
        }

        let p: *mut Proc = myproc();
        let mut data = &mut *(*p).data.get();
        let mut pt = PageTable::<UVAddr>::new(data.trapframe).ok_or(())?;

        // Load program into memory.
        let mut sz = 0;
        for i in 0..elf.phnum as usize {
            let off = elf.phoff + i * mem::size_of::<ProgHdr>();

            let mut ph: ProgHdr = Default::default();
            let bytes_read = ip.read(
                KVAddr::new(&mut ph as *mut _ as _),
                off as _,
                mem::size_of::<ProgHdr>() as _,
            )?;
            if bytes_read != mem::size_of::<ProgHdr>() {
                return Err(());
            }
            if ph.is_prog_load() {
                if ph.memsz < ph.filesz || ph.vaddr % PGSIZE != 0 {
                    return Err(());
                }
                sz = pt.alloc(sz, ph.vaddr.checked_add(ph.memsz).ok_or(())?)?;
                loadseg(
                    &mut pt,
                    UVAddr::new(ph.vaddr),
                    &mut ip,
                    ph.off as _,
                    ph.filesz as _,
                )?;
            }
        }
        drop(ip);
        drop(tx);

        // Allocate two pages at the next page boundary.
        // Use the second as the user stack.
        sz = pgroundup(sz);
        sz = pt.alloc(sz, sz + 2 * PGSIZE)?;
        pt.clear(UVAddr::new(sz - 2 * PGSIZE));
        let mut sp: usize = sz;
        let stackbase: usize = sp - PGSIZE;

        // Push argument strings, prepare rest of stack in ustack.
        let mut ustack = [0usize; MAXARG + 1];
        for (arg, stack) in izip!(argv, &mut ustack) {
            let bytes = CStr::from_ptr(arg.as_ptr()).to_bytes_with_nul();
            sp -= bytes.len();

            // riscv sp must be 16-byte aligned
            sp &= !0xf;
            if sp < stackbase {
                return Err(());
            }

            pt.copy_out(UVAddr::new(sp), bytes)?;
            *stack = sp;
        }
        let argc: usize = argv.len();
        ustack[argc] = 0;

        // push the array of argv[] pointers.
        let argv_size = (argc + 1) * mem::size_of::<usize>();
        sp -= argv_size;
        sp &= !0xf;
        if sp < stackbase {
            return Err(());
        }
        pt.copy_out(UVAddr::new(sp), &ustack.align_to::<u8>().1[..argv_size])?;

        // arguments to user main(argc, argv)
        // argc is returned via the system call return
        // value, which goes in a0.
        (*data.trapframe).a1 = sp;

        // Save program name for debugging.
        let path_str = path.as_bytes();
        let name = path_str
            .iter()
            .rposition(|c| *c == b'/')
            .map(|i| &path_str[(i + 1)..])
            .unwrap_or(path_str);
        let p_name = &mut (*p).name;
        let len = cmp::min(p_name.len(), name.len());
        p_name[..len].copy_from_slice(&name[..len]);
        if len < p_name.len() {
            p_name[len] = 0;
        }

        // Commit to the user image.
        data.pagetable = pt;
        data.sz = sz;

        // initial program counter = main
        (*data.trapframe).epc = elf.entry;

        // initial stack pointer
        (*data.trapframe).sp = sp;

        // this ends up in a0, the first argument to main(argc, argv)
        Ok(argc)
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
            .walk_addr(va + i as usize)
            .expect("loadseg: address should exist")
            .into_usize();

        let n = cmp::min(sz - i, PGSIZE as _);

        let bytes_read = ip.read(KVAddr::new(pa), offset + i, n)?;
        if bytes_read != n as _ {
            return Err(());
        }
    }

    Ok(())
}
