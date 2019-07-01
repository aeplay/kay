use crate::actor::{Actor, ActorOrActorTrait};
use crate::class::{Class, ActorVTable};
use crate::id::{MachineID, RawID};
use crate::messaging::{Fate, Message, Packet};
use crate::networking::Networking;
use crate::type_registry::{ShortTypeId, TypeRegistry};

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

const MAX_RECIPIENT_TYPES: usize = 64;
pub const MAX_MESSAGE_TYPES: usize = 256;

pub struct ActorSystem {
    pub panic_happened: bool,
    pub shutting_down: bool,
    actor_registry: TypeRegistry,
    message_registry: TypeRegistry,
    classes: [Option<Class>; MAX_RECIPIENT_TYPES],
    trait_implementors: [Option<Vec<ShortTypeId>>; MAX_RECIPIENT_TYPES],
    message_statistics: [usize; MAX_MESSAGE_TYPES],
    networking: Networking,
    storage: Rc<dyn chunky::ChunkStorage>
}

impl ActorSystem {
    pub fn new(networking: Networking) -> ActorSystem {
        Self::new_with_storage(networking, Rc::new(chunky::HeapStorage))
    }

    #[cfg(feature = "server")]
    pub fn new_mmap_persisted<P: AsRef<::std::path::Path>>(networking: Networking, directory: &P) -> ActorSystem {
        Self::new_with_storage(networking, Rc::new(chunky::MmapStorage{directory: directory.as_ref().to_owned()}))
    }

    pub fn new_with_storage(networking: Networking, storage: Rc<dyn chunky::ChunkStorage>) -> ActorSystem {
        ActorSystem {
            panic_happened: false,
            shutting_down: false,
            trait_implementors: unsafe { make_array!(MAX_RECIPIENT_TYPES, |_| None) },
            actor_registry: TypeRegistry::new(),
            message_registry: TypeRegistry::new(),
            classes: unsafe { make_array!(MAX_RECIPIENT_TYPES, |_| None) },
            message_statistics: [0; MAX_MESSAGE_TYPES],
            networking,
            storage
        }
    }

    pub fn register<A: Actor>(&mut self) {
        // allow use of actor id before it is added
        let actor_id = self.actor_registry.get_or_register::<A>();
        // ...but still make sure it is only added once
        assert!(self.classes[actor_id.as_usize()].is_none());
        // Store pointer to the actor
        let class = Class::new(ActorVTable::new_for_actor_type::<A>(), Rc::clone(&self.storage));
        self.classes[actor_id.as_usize()] = Some(class);
    }

    pub fn register_dummy<D: 'static>(&mut self) {
        let _actor_id = self.actor_registry.get_or_register::<D>();
    }

    pub fn register_trait<T: ActorOrActorTrait>(&mut self) {
        let trait_id = self.actor_registry.get_or_register::<T>();
        self.trait_implementors[trait_id.as_usize()].get_or_insert_with(Vec::new);
    }

    pub fn register_trait_message<M: Message>(&mut self) {
        self.message_registry.get_or_register::<M>();
    }

    pub fn register_implementor<A: Actor, T: ActorOrActorTrait>(&mut self) {
        let trait_id = self.actor_registry.get_or_register::<T>();
        let actor_id = self.actor_registry.get::<A>();
        self.trait_implementors[trait_id.as_usize()]
            .get_or_insert_with(Vec::new)
            .push(actor_id);
    }

    pub fn add_handler<A: Actor, M: Message, F: Fn(&M, &mut A, &mut World) -> Fate + 'static>(
        &mut self,
        handler: F,
        critical: bool,
    ) {
        let actor_id = self.actor_registry.get::<A>();
        let message_id = self.message_registry.get_or_register::<M>();
        let class = self.classes[actor_id.as_usize()].as_mut().expect("Actor not added yet");
        class.add_handler(message_id, handler, critical);
    }

    pub fn add_spawner<A: Actor, M: Message, F: Fn(&M, &mut World) -> A + 'static>(
        &mut self,
        constructor: F,
        critical: bool,
    ) {
        let actor_id = self.actor_registry.get::<A>();
        let message_id = self.message_registry.get_or_register::<M>();
        let class = self.classes[actor_id.as_usize()].as_mut().expect("Actor not added yet");
        class.add_spawner(message_id, constructor, critical);
    }

    pub fn send<M: Message>(&mut self, recipient: RawID, message: M) {
        let packet = Packet {
            recipient_id: recipient,
            message,
        };

        let to_here = recipient.machine == self.networking.machine_id;
        let global = recipient.is_global_broadcast();

        if !to_here || global {
            self.networking
                .enqueue(self.message_registry.get::<M>(), packet.clone());
        }

        if to_here || global {
            if let Some(class) = self.classes[recipient.type_id.as_usize()].as_mut() {
                class.inbox.put(packet, &self.message_registry);
            } else if let Some(implementors) = self.trait_implementors[recipient.type_id.as_usize()].as_ref() {
                for implementor_type_id in implementors {
                    let class = self.classes[implementor_type_id.as_usize()].as_mut().expect("Implementor should exist");
                    class.inbox.put(packet.clone(), &self.message_registry);
                }
            } else {
                panic!(
                    "Recipient {} doesn't exist, or Trait has no implementors",
                    self.actor_registry.get_name(recipient.type_id),
                );
            }
        }
    }

    pub fn id<A: ActorOrActorTrait>(&mut self) -> RawID {
        RawID::new(self.short_id::<A>(), 0, self.networking.machine_id, 0)
    }

    fn short_id<A: ActorOrActorTrait>(&mut self) -> ShortTypeId {
        self.actor_registry.get_or_register::<A>()
    }

    fn single_message_cycle(&mut self) {
        let mut world = World(self as *const Self as *mut Self);

        for (recipient_type_idx, maybe_class) in self.classes.iter_mut().enumerate() {
            if let Some(recipient_type) = ShortTypeId::new(recipient_type_idx as u16) {
                if let Some(class) = maybe_class.as_mut() {
                    class.handle_messages(&mut self.message_statistics, &mut world);
                }
            }
        }
    }

    pub fn process_all_messages(&mut self) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            for _i in 0..1000 {
                self.single_message_cycle();
            }
        }));

        if result.is_err() {
            self.panic_happened = true;
        }
    }

    pub fn world(&mut self) -> World {
        World(self as *mut Self)
    }

    pub fn networking_connect(&mut self) {
        self.networking.connect();
    }

    pub fn networking_send_and_receive(&mut self) {
        self.networking
            .send_and_receive(&mut self.classes, &mut self.trait_implementors);
    }

    pub fn networking_finish_turn(&mut self) -> Option<usize> {
        self.networking.finish_turn()
    }

    pub fn networking_machine_id(&self) -> MachineID {
        self.networking.machine_id
    }

    pub fn networking_n_turns(&self) -> usize {
        self.networking.n_turns
    }

    pub fn networking_debug_all_n_turns(&self) -> HashMap<MachineID, isize> {
        self.networking.debug_all_n_turns()
    }

    pub fn get_instance_counts(&self) -> HashMap<String, usize> {
        self.classes
            .iter()
            .filter_map(|maybe_class| maybe_class.as_ref())
            .map(|class| {
                (
                    class.v_table.type_name.split("::").last().unwrap().replace(">", ""),
                    *class.instance_store.n_instances,
                )
            }).collect()
    }

    pub fn get_message_statistics(&self) -> HashMap<String, usize> {
        self.message_statistics
            .iter()
            .enumerate()
            .filter_map(|(i, n_sent)| {
                if *n_sent > 0 {
                    let name = self
                        .message_registry
                        .get_name(ShortTypeId::new(i as u16).unwrap());
                    Some((name.to_owned(), *n_sent))
                } else {
                    None
                }
            }).collect()
    }

    pub fn reset_message_statistics(&mut self) {
        self.message_statistics = [0; MAX_MESSAGE_TYPES]
    }

    pub fn get_queue_lengths(&self) -> HashMap<String, usize> {
        #[cfg(feature = "server")]
        let connection_queue_length = None;
        #[cfg(feature = "browser")]
        let connection_queue_length = self
            .networking
            .main_out_connection()
            .map(|connection| ("NETWORK QUEUE".to_owned(), connection.in_queue_len()));

        self.classes
            .iter()
            .enumerate()
            .filter_map(|(i, maybe_class)| {
                maybe_class.as_ref().map(|class| {
                    let actor_name = self
                        .actor_registry
                        .get_name(ShortTypeId::new(i as u16).unwrap());
                    (actor_name.to_owned(), class.inbox.len())
                })
            }).chain(connection_queue_length)
            .collect()
    }

    pub fn get_actor_type_id_to_name_mapping(&self) -> HashMap<u16, String> {
        self.actor_registry.short_ids_to_names.iter().map(|(short_id, name)|
            (short_id.as_u16(), name.clone())
        ).collect()
    }
}

pub struct World(*mut ActorSystem);

// TODO: make this true
unsafe impl Sync for World {}
unsafe impl Send for World {}

impl World {
    pub fn send<M: Message>(&mut self, receiver: RawID, message: M) {
        unsafe { &mut *self.0 }.send(receiver, message);
    }

    pub fn local_first<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>()
    }

    pub fn global_first<A: ActorOrActorTrait>(&mut self) -> RawID {
        let mut id = unsafe { &mut *self.0 }.id::<A>();
        id.machine = MachineID(0);
        id
    }

    pub fn local_broadcast<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>().local_broadcast()
    }

    pub fn global_broadcast<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>().global_broadcast()
    }

    pub fn allocate_instance_id<A: 'static + Actor>(&mut self) -> RawID {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        let class = system.classes[system.actor_registry.get::<A>().as_usize()].as_mut()
                .expect("Subactor type not found.");
        unsafe { class.instance_store.allocate_id(self.local_broadcast::<A>()) }
    }

    pub fn local_machine_id(&mut self) -> MachineID {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.networking.machine_id
    }

    pub fn panic_happened(&self) -> bool {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.panic_happened
    }

    pub fn shutdown(&mut self) {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.shutting_down = true;
    }

    pub fn get_actor_name(&mut self, type_id: ShortTypeId) -> &str {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.actor_registry.get_name(type_id)
    }
}