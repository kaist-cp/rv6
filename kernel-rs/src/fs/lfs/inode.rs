use core::ops::Deref;

use super::super::{InodeGuard, InodeInner};
use super::Inode;
use crate::{arena::ArenaObject, proc::KernelCtx};

pub struct I {}

impl InodeInner for I {
    #[inline]
    fn read_internal<
        'id,
        's,
        K: Deref<Target = KernelCtx<'id, 's>>,
        F: FnMut(u32, &[u8], &mut K) -> Result<(), ()>,
    >(
        guard: &mut InodeGuard<'_, Self>,
        off: u32,
        n: u32,
        f: F,
        k: K,
    ) -> Result<usize, ()> {
        todo!()
    }
}

impl ArenaObject for Inode<I> {
    type Ctx<'a, 'id: 'a> = ();

    #[allow(clippy::needless_lifetimes)]
    fn finalize<'a, 'id: 'a>(&mut self, _: ()) {}
}
