use core::ops::Index;
use core::pin::Pin;

/// An array that holds pinned data, and hence, should also be pinned.
/// From `&PinnedArray<T, N>`, you can get `&T` such as by `pinned_array[index]`.
/// From `Pin<&mut Array<T, N>>`, you can get `Pin<&mut T>` such as by `pinned_array.index_mut(index)`.
/// However, there is no way to get an `&mut T` to the inner data.
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
}

impl<T, const N: usize> Index<usize> for PinnedArray<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.arr[index]
    }
}
