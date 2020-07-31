/// Format of an ELF executable file

/// "\x7FELF" in little endian
pub const ELF_MAGIC: u32 = 0x464c457f;

/// File header
#[derive(Default, Clone)]
#[repr(C)]
pub struct ElfHdr {
    pub magic: u32,
    pub elf: [u8; 12],
    pub typ: u16,
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
#[derive(Default, Clone)]
#[repr(C)]
pub struct ProgHdr {
    pub typ: u32,
    pub flags: u32,
    pub off: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

/// Values for Proghdr type
pub const ELF_PROG_LOAD: u32 = 1;

bitflags! {
    pub struct ELF_PROG_FLAG: u32 {
        const EXEC = 1;
        const WRITE = 2;
        const READ = 4;
    }
}
