/// Format of an ELF executable file

/// "\x7FELF" in little endian
pub const ELF_MAGIC: u32 = 0x464c457f;

/// File header
#[derive(Default, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct ElfHdr {
    /// must equal ELF_MAGIC
    pub magic: u32,
    elf: [u8; 12],
    typ: u16,
    machine: u16,
    version: u32,
    pub entry: usize,
    pub phoff: usize,
    shoff: usize,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    pub phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

/// Program section header
#[derive(Default, Clone)]
// It needs repr(C) because it's struct for in-disk representation
// which should follow C(=machine) representation
// https://github.com/kaist-cp/rv6/issues/52
#[repr(C)]
pub struct ProgHdr {
    pub typ: u32,
    flags: u32,
    pub off: usize,
    pub vaddr: usize,
    paddr: usize,
    pub filesz: usize,
    pub memsz: usize,
    align: usize,
}

/// Values for Proghdr type
pub const ELF_PROG_LOAD: u32 = 1;

bitflags! {
    /// Flag bits for ProgHdr flags
    pub struct ELF_PROG_FLAG: u32 {
        const EXEC = 1;
        const WRITE = 2;
        const READ = 4;
    }
}
