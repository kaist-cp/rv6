bitflags! {
    pub struct Flags: i32 {
        const O_RDONLY = 0;
        const O_WRONLY = 0x1;
        const O_RDWR = 0x2;
        const O_CREATE = 0x200;
        const O_MODE = 0;
    }
}

impl Flags {
    pub fn setbits(&mut self, n: i32) -> &mut Flags {
        self.bits = n;
        self
    }
}
