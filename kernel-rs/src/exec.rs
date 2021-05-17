#![allow(clippy::unit_arg)]

use core::{cmp, mem};

use bitflags::bitflags;
use itertools::*;
use zerocopy::{AsBytes, FromBytes};

use crate::{
    arch::addr::{pgroundup, PAddr, PGSIZE},
    fs::{FileSystem, Path},
    hal::hal,
    page::Page,
    param::MAXARG,
    proc::KernelCtx,
    vm::UserMemory,
};

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
#[derive(AsBytes, FromBytes)]
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
    #[derive(AsBytes, FromBytes)]
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
#[derive(AsBytes, FromBytes)]
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

impl KernelCtx<'_, '_> {
    pub fn exec(&mut self, path: &Path, args: &[Page]) -> Result<usize, ()> {
        if args.len() > MAXARG {
            return Err(());
        }

        // TODO(https://github.com/kaist-cp/rv6/issues/267): remove hal()
        let allocator = &unsafe { hal() }.kmem;

        // TODO(https://github.com/kaist-cp/rv6/issues/290): The method namei can drop inodes. If
        // namei succeeds, its return value, ptr, will be dropped when this method
        // returns. Deallocation of an inode may cause disk write operations, so we must begin a
        // transaction here.
        let tx = self.kernel().fs().begin_tx();
        let tx = scopeguard::guard(tx, |t| t.end(&self));
        let ptr = self.kernel().fs().itable.namei(path, self)?;
        let mut ip = ptr.lock(self);

        // Check ELF header
        let mut elf: ElfHdr = Default::default();
        ip.read_kernel(&mut elf, 0, self)?;
        if !elf.is_valid() {
            return Err(());
        }

        let trap_frame: PAddr = (self.proc().trap_frame() as *const _ as usize).into();
        let mem = UserMemory::new(trap_frame, None, allocator).ok_or(())?;
        let mut mem = scopeguard::guard(mem, |mem| mem.free(allocator));

        // Load program into memory.
        for i in 0..elf.phnum as usize {
            let off = elf.phoff + i * mem::size_of::<ProgHdr>();

            let mut ph: ProgHdr = Default::default();
            ip.read_kernel(&mut ph, off as _, self)?;
            if ph.is_prog_load() {
                if ph.memsz < ph.filesz || ph.vaddr % PGSIZE != 0 {
                    return Err(());
                }
                let _ = mem.alloc(ph.vaddr.checked_add(ph.memsz).ok_or(())?, allocator)?;
                mem.load_file(ph.vaddr.into(), &mut ip, ph.off as _, ph.filesz as _, self)?;
            }
        }
        drop(ip);
        scopeguard::ScopeGuard::into_inner(tx).end(self);

        // Allocate two pages at the next page boundary.
        // Use the second as the user stack.
        let mut sz = pgroundup(mem.size());
        sz = mem.alloc(sz + 2 * PGSIZE, allocator)?;
        mem.clear((sz - 2 * PGSIZE).into());
        let mut sp: usize = sz;
        let stackbase: usize = sp - PGSIZE;

        // Push argument strings, prepare rest of stack in ustack.
        let mut ustack = [0usize; MAXARG + 1];
        for (arg, stack) in izip!(args, &mut ustack) {
            let null_idx = arg
                .iter()
                .position(|c| *c == 0)
                .expect("exec: no null char found");
            let bytes = &arg[..null_idx + 1];
            sp -= bytes.len();

            // riscv sp must be 16-byte aligned
            sp &= !0xf;
            if sp < stackbase {
                return Err(());
            }

            mem.copy_out_bytes(sp.into(), bytes)?;
            *stack = sp;
        }
        let argc: usize = args.len();
        ustack[argc] = 0;

        // push the array of argv[] pointers.
        let argv_size = (argc + 1) * mem::size_of::<usize>();
        sp -= argv_size;
        sp &= !0xf;
        if sp < stackbase {
            return Err(());
        }
        // SAFETY: any byte can be considered as a valid u8.
        let (_, ustack, _) = unsafe { ustack.align_to::<u8>() };
        mem.copy_out_bytes(sp.into(), &ustack[..argv_size])?;

        // Save program name for debugging.
        let path_str = path.as_bytes();
        let name = path_str
            .iter()
            .rposition(|c| *c == b'/')
            .map(|i| &path_str[(i + 1)..])
            .unwrap_or(path_str);
        let proc_name = &mut self.proc_mut().deref_mut_data().name;
        let len = cmp::min(proc_name.len(), name.len());
        proc_name[..len].copy_from_slice(&name[..len]);
        if len < proc_name.len() {
            proc_name[len] = 0;
        }

        // Commit to the user image.
        mem::replace(
            self.proc_mut().memory_mut(),
            scopeguard::ScopeGuard::into_inner(mem),
        )
        .free(allocator);

        // arguments to user main(argc, argv)
        // argc is returned via the system call return
        // value, which goes in a0.
        self.proc_mut().trap_frame_mut().a1 = sp;

        // initial program counter = main
        self.proc_mut().trap_frame_mut().epc = elf.entry;

        // initial stack pointer
        self.proc_mut().trap_frame_mut().sp = sp;

        // this ends up in a0, the first argument to main(argc, argv)
        Ok(argc)
    }
}
