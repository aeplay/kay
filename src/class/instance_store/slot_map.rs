use chunky;
use std::rc::Rc;
use crate::tuning::Tuning;

#[derive(Clone, Copy)]
pub struct SlotIndices {
    bin: u8,
    slot: u32,
}

impl SlotIndices {
    pub fn new(bin: usize, slot: usize) -> SlotIndices {
        SlotIndices {
            bin: bin as u8,
            slot: slot as u32,
        }
    }

    pub fn invalid() -> SlotIndices {
        SlotIndices {
            bin: u8::max_value(),
            slot: u32::max_value(),
        }
    }

    pub fn bin(&self) -> usize {
        self.bin as usize
    }

    pub fn slot(&self) -> usize {
        self.slot as usize
    }
}

impl Into<chunky::MultiArenaIndex> for SlotIndices {
    fn into(self) -> chunky::MultiArenaIndex {
        chunky::MultiArenaIndex(self.bin(), chunky::ArenaIndex(self.slot()))
    }
}

impl From<chunky::MultiArenaIndex> for SlotIndices {
    fn from(source: chunky::MultiArenaIndex) -> Self {
        Self::new(source.0, (source.1).0)
    }
}

pub struct SlotMap {
    entries: chunky::Vector<SlotIndices>,
    last_known_version: chunky::Vector<u8>,
    free_ids_with_versions: chunky::Vector<(usize, usize)>,
}

impl SlotMap {
    pub fn new(ident: &chunky::Ident, storage: Rc<dyn chunky::ChunkStorage>, tuning: &Tuning) -> Self {
        SlotMap {
            entries: chunky::Vector::new(ident.sub("entr"), tuning.instance_entry_chunk_size, Rc::clone(&storage)),
            last_known_version: chunky::Vector::new(ident.sub("vrsns"), tuning.instance_versions_chunk_size, Rc::clone(&storage)),
            free_ids_with_versions: chunky::Vector::new(ident.sub("free"), tuning.instance_free_chunk_size, storage),
        }
    }

    pub fn allocate_id(&mut self) -> (usize, usize) {
        match self.free_ids_with_versions.pop() {
            None => {
                self.entries.push(SlotIndices::invalid());
                self.last_known_version.push(0);
                (self.entries.len() - 1, 0)
            }
            Some((id, version)) => (id, version),
        }
    }

    pub fn associate(&mut self, id: usize, new_entry: SlotIndices) {
        let entry = self
            .entries
            .at_mut(id)
            .expect("Should already have entry allocated when associating");
        entry.clone_from(&new_entry);
    }

    pub fn indices_of(&self, id: usize, version: u8) -> Option<SlotIndices> {
        if let Some(last_known_version) = self.last_known_version.at(id) {
            if *last_known_version == version {
                self.indices_of_no_version_check(id)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn indices_of_no_version_check(&self, id: usize) -> Option<SlotIndices> {
        self.entries.at(id).cloned()
    }

    pub fn free(&mut self, id: usize, version: usize) {
        *self
            .last_known_version
            .at_mut(id)
            .expect("should have last known version when freeing") = (version + 1) as u8;
        self.free_ids_with_versions.push((id, version + 1));
    }
}
