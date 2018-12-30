use std::mem::size_of;
pub trait StorageAware: Sized {
    fn typical_size() -> usize {
        let size = size_of::<Self>();
        if size == 0 {
            1
        } else {
            size
        }
    }
}
impl<T> StorageAware for T {}
