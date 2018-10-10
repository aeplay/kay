use std::mem::size_of;

/// Trait that allows dynamically sized `Actor` instances to provide
/// a "typical size" hint to optimize their storage in a `InstanceStore`
pub trait StorageAware: Sized {
    /// The default implementation just returns the static size of the implementing type
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
