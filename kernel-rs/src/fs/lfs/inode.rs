use super::Inode;
use crate::arena::ArenaObject;

pub struct InodeInner {}

impl ArenaObject for Inode<InodeInner> {
    type Ctx<'a, 'id: 'a> = ();

    #[allow(clippy::needless_lifetimes)]
    fn finalize<'a, 'id: 'a>(&mut self, _: ()) {}
}
