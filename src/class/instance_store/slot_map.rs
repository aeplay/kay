use chunky;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub struct SlotIndices {
    bin: u8,
    version: u8,
    slot: u32,
}

impl SlotIndices {
    pub fn new(bin: usize, slot: usize, version: u8) -> SlotIndices {
        SlotIndices {
            bin: bin as u8,
            version,
            slot: slot as u32,
        }
    }

    pub fn invalid(version: u8) -> SlotIndices {
        SlotIndices {
            bin: u8::max_value(),
            version,
            slot: u32::max_value(),
        }
    }

    pub fn artificial(bin: usize, slot: usize) -> SlotIndices {
        SlotIndices {
            bin: bin as u8,
            version: u8::max_value(),
            slot: slot as u32,
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

pub struct SlotMap {
    entries: chunky::Vector<SlotIndices>,
    free_ids_with_versions: chunky::Vector<(usize, u8)>,
}

impl SlotMap {
    pub fn new(ident: &chunky::Ident, storage: Rc<dyn chunky::ChunkStorage>) -> Self {
        SlotMap {
            entries: chunky::Vector::new(ident.sub("entr"), 64 * 1024, Rc::clone(&storage)),
            free_ids_with_versions: chunky::Vector::new(ident.sub("free"), 1024, storage),
        }
    }

    pub fn allocate_id(&mut self) -> (usize, usize) {
        match self.free_ids_with_versions.pop() {
            None => {
                self.entries.push(SlotIndices::invalid(0));
                (self.entries.len() - 1, 0)
            }
            Some((id, version)) => (id, version as usize),
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
        self.indices_of_no_version_check(id).and_then(|indices|
            if indices.version == version {
                Some(indices)
            } else {
                println!("Version check failed. Slop map: {} given: {}", indices.version, version);
                None
            }
        )
    }

    pub fn indices_of_no_version_check(&self, id: usize) -> Option<SlotIndices> {
        self.entries.at(id).cloned()
    }

    pub fn free(&mut self, id: usize, version: u8) {
        // set to one version higher to mark as invalidated
        self.indices_of(id, version)
            .expect("should have last known version when freeing").version = version + 1;
        self.free_ids_with_versions.push((id, version + 1));
    }
}
