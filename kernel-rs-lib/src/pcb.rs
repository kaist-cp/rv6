use core::cell::UnsafeCell;

pub trait PCB {
    type O;

    fn owned(&self) -> &UnsafeCell<Self::O>;
}

pub unsafe trait Current {
    type P: PCB;

    fn get_pcb(&self) -> &Self::P;

    fn deref_owned(&self) -> &<Self::P as PCB>::O {
        unsafe { &*self.get_pcb().owned().get() }
    }

    fn deref_owned_mut(&mut self) -> &mut <Self::P as PCB>::O {
        unsafe { &mut *self.get_pcb().owned().get() }
    }
}
