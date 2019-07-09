pub struct Tuning {
    pub instance_chunk_size: usize,
    pub instance_entry_chunk_size: usize,
    pub instance_versions_chunk_size: usize,
    pub instance_free_chunk_size: usize,
    pub inbox_queue_chunk_size: usize
}

impl ::std::default::Default for Tuning {
    fn default() -> Self {
        Tuning {
            instance_chunk_size: 4 * 1024 * 1024,
            instance_entry_chunk_size: 1024 * 1024,
            instance_versions_chunk_size: 512 * 1024,
            instance_free_chunk_size: 8 * 1024,
            inbox_queue_chunk_size: 1024 * 1024
        }
    }
}