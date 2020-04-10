use crate::messaging::HandlerFnRef;
use crate::actor_system::{World};
use crate::tuning::Tuning;
use chunky;
use crate::id::RawID;
use crate::messaging::Fate;
use super::ActorStateVTable;
use compact::Compact;
use ::std::rc::Rc;

mod slot_map;
use self::slot_map::{SlotMap, SlotIndices};

pub struct InstanceStore {
    instances: chunky::MultiArena,
    slot_map: SlotMap,
    pub n_instances: chunky::Value<usize>,
}

impl InstanceStore {
    pub fn new(ident: &chunky::Ident, typical_size: usize, storage: Rc<dyn chunky::ChunkStorage>, tuning: &Tuning) -> InstanceStore {
        InstanceStore {
                instances: chunky::MultiArena::new(
                    ident.sub("inst"),
                    tuning.instance_chunk_size,
                    typical_size,
                    Rc::clone(&storage)
                ),
                n_instances: chunky::Value::load_or_default(ident.sub("n"), 0, Rc::clone(&storage)),
                slot_map: SlotMap::new(&ident.sub("slts"), storage, tuning),
            }
    }

    fn allocate_instance_id(&mut self) -> (usize, usize) {
        self.slot_map.allocate_id()
    }

    fn at_index_mut(&mut self, index: SlotIndices) -> *mut () {
        self.instances.at_mut(index.into()) as *mut ()
    }

    fn at_mut(&mut self, id: usize, version: u8) -> Option<*mut ()> {
        self.slot_map
            .indices_of(id, version)
            .map(move |index| self.at_index_mut(index))
    }

    pub unsafe fn allocate_id(&mut self, base_id: RawID) -> RawID {
        let (instance_id, version) = self.allocate_instance_id();
        RawID::new(
            base_id.type_id,
            instance_id as u32,
            base_id.machine,
            version as u8,
        )
    }

    pub unsafe fn add(&mut self, initial_state: *mut (), state_v_table: &ActorStateVTable, increment_n_instances: bool) {
        let id = (state_v_table.get_raw_id)(initial_state);
        let size = (state_v_table.total_size_bytes)(initial_state);
        let (slot_ptr, index) = self.instances.push(size);

        self.slot_map
            .associate(id.instance_id as usize, index.into());

        if increment_n_instances {*self.n_instances += 1}

        (state_v_table.compact_behind)(initial_state, slot_ptr as *mut ());
    }

    fn swap_remove(&mut self, indices: SlotIndices, state_v_table: &ActorStateVTable) -> bool {
        match self.instances.swap_remove_within_bin(indices.into()) {
            Some(swapped_actor) => {
                self.slot_map
                    .associate((state_v_table.get_raw_id)(swapped_actor as *const ()).instance_id as usize, indices);
                true
            }
            None => false,
        }
    }

    fn remove(&mut self, id: RawID, state_v_table: &ActorStateVTable) {
        let i = self
            .slot_map
            .indices_of_no_version_check(id.instance_id as usize)
            .expect("actor should exist when removing");
        self.remove_at_index(i, id, state_v_table);
    }

    fn remove_at_index(&mut self, i: SlotIndices, id: RawID, state_v_table: &ActorStateVTable) {
        // TODO: not sure if this is the best place to drop actor state
        let old_actor_ptr = self.at_index_mut(i);
        (state_v_table.drop)(old_actor_ptr);
        self.swap_remove(i, state_v_table);
        self.slot_map
            .free(id.instance_id as usize, id.version as usize);
        *self.n_instances -= 1;
    }

    fn resize(&mut self, id: usize, state_v_table: &ActorStateVTable) -> bool {
        let index = self
            .slot_map
            .indices_of_no_version_check(id)
            .expect("actor should exist when resizing");
        self.resize_at_index(index, state_v_table)
    }

    fn resize_at_index(&mut self, old_i: SlotIndices, state_v_table: &ActorStateVTable) -> bool {
        let old_actor_ptr = self.at_index_mut(old_i);
        unsafe { self.add(old_actor_ptr, state_v_table, false) };
        self.swap_remove(old_i, state_v_table)
    }

    pub fn receive_instance(&mut self, recipient_id: RawID, packet_ptr: *const (), world: &mut World, handler: &Box<HandlerFnRef>, state_v_table: &ActorStateVTable) {
        if let Some(actor) = self.at_mut(
            recipient_id.instance_id as usize,
            recipient_id.version,
        ) {
            let fate = handler(actor, packet_ptr, world);
            let is_still_compact = (state_v_table.is_still_compact)(actor);

            match fate {
                Fate::Live => {
                    if !is_still_compact {
                        self.resize(recipient_id.instance_id as usize, &state_v_table);
                    }
                }
                Fate::Die => self.remove(recipient_id, &state_v_table),
            }
        } else {
            eprintln!("Could not find actor {}", recipient_id.format(world));
        }
    }

    pub fn receive_broadcast(&mut self, packet_ptr: *const (), world: &mut World, handler: &Box<HandlerFnRef>, state_v_table: &ActorStateVTable) {
    // this function has to deal with the fact that during the iteration,
    // receivers of the broadcast can be resized
    // and thus removed from a bin, swapping in either
    //    - other receivers that didn't receive the broadcast yet
    //    - resized and added receivers that alredy received the broadcast
    //    - sub actors that were created during one of the broadcast receive handlers,
    //      that shouldn't receive this broadcast
    // the only assumption is that no sub actors are immediately completely deleted
    let bin_indices_recipients_todo: Vec<_> =
        self.instances.populated_bin_indices_and_lens().collect();

    for (bin_index, recipients_todo) in bin_indices_recipients_todo {
        let mut slot = 0;
        let mut index_after_last_recipient = recipients_todo;

        for _ in 0..recipients_todo {
            let index = SlotIndices::new(bin_index, slot);
            let (fate, is_still_compact, id) = {
                let actor = self.at_index_mut(index);
                let fate = handler(actor, packet_ptr, world);
                (fate, actor.is_still_compact(), (state_v_table.get_raw_id)(actor))
            };

            let repeat_slot = match fate {
                Fate::Live => {
                    if is_still_compact {
                        false
                    } else {
                        self.resize_at_index(index, state_v_table);
                        // this should also work in the case where the "resized" actor
                        // itself is added to the same bin again
                        let swapped_in_another_receiver =
                            self.instances.bin_len(bin_index) < index_after_last_recipient;
                        if swapped_in_another_receiver {
                            index_after_last_recipient -= 1;
                            true
                        } else {
                            false
                        }
                    }
                }
                Fate::Die => {
                    self.remove_at_index(index, id, state_v_table);
                    // this should also work in the case where the "resized" actor
                    // itself is added to the same bin again
                    let swapped_in_another_receiver =
                        self.instances.bin_len(bin_index) < index_after_last_recipient;
                    if swapped_in_another_receiver {
                        index_after_last_recipient -= 1;
                        true
                    } else {
                        false
                    }
                }
            };

            if !repeat_slot {
                slot += 1;
            }
        }
    }
}
}