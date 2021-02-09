use core::ops::Index;
use core::pin::Pin;
use core::slice;

/// An array that holds pinned data, and hence, should also be pinned.
/// From `&PinnedArray<T, N>`, you can get `&T` such as by `pinned_array[index]` or iterating.
/// From `Pin<&mut Array<T, N>>`, you can get `Pin<&mut T>` such as by `pinned_array.index_mut(index)` or iterating.
/// However, there is no way to get an `&mut T` to the inner data.
#[derive(Debug)]
pub struct PinnedArray<T, const N: usize> {
    arr: [T; N],
}

impl<T, const N: usize> PinnedArray<T, N> {
    pub const fn new(arr: [T; N]) -> Self {
        Self { arr }
    }

    pub fn index_mut(self: Pin<&mut Self>, index: usize) -> Pin<&mut T> {
        // Safe since we're just projecting from `Pin<&mut PinnedArray<T, N>>` to one of its elements `Pin<&mut T>`.
        unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().arr[index]) }
    }

    pub fn len(&self) -> usize {
        self.arr.len()
    }
}

impl<T, const N: usize> Index<usize> for PinnedArray<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.arr[index]
    }
}

impl<'s, T, const N: usize> IntoIterator for &'s PinnedArray<T, N> {
    type IntoIter = slice::Iter<'s, T>;
    type Item = &'s T;

    fn into_iter(self) -> slice::Iter<'s, T> {
        self.arr.iter()
    }
}

impl<'s, T, const N: usize> IntoIterator for Pin<&'s mut PinnedArray<T, N>> {
    type IntoIter = IterMut<'s, T>;
    type Item = Pin<&'s mut T>;

    fn into_iter(self) -> IterMut<'s, T> {
        IterMut {
            iter: unsafe { self.get_unchecked_mut() }.arr.iter_mut(),
        }
    }
}

/// An iterator for `PinnedArray` that gives pinned mutable references.
#[derive(Debug)]
pub struct IterMut<'s, T> {
    iter: slice::IterMut<'s, T>,
}

impl<'s, T> Iterator for IterMut<'s, T> {
    type Item = Pin<&'s mut T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|p| unsafe { Pin::new_unchecked(p) })
    }
}
