use super::super::Inode;
use super::Lfs;
use crate::arena::ArenaObject;

pub struct InodeInner {}

impl ArenaObject for Inode<Lfs> {
    type Ctx<'a, 'id: 'a> = ();

    #[allow(clippy::needless_lifetimes)]
    fn finalize<'a, 'id: 'a>(&mut self, _: ()) {}
}
