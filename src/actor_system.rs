use crate::actor::{Actor, ActorOrActorTrait};
use crate::class::{Class, ActorVTable};
use crate::id::{MachineID, RawID};
use crate::messaging::{Fate, Message, Packet};
use crate::networking::Networking;
use crate::type_registry::{ShortTypeId, TypeRegistry};
use crate::tuning::Tuning;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

const MAX_RECIPIENT_TYPES: usize = 64;
pub const MAX_MESSAGE_TYPES: usize = 256;

/// Contains the state of a whole actor system
/// and can be used for managing and progressing the actor system
/// as well as for interacting with it from the outside
pub struct ActorSystem {
    /// Did a panic happen inside a message handler?
    /// If so, only messages marked as `Critical` are still sent.
    pub panic_happened: bool,
    actor_registry: TypeRegistry,
    message_registry: TypeRegistry,
    classes: [Option<Class>; MAX_RECIPIENT_TYPES],
    trait_implementors: [Option<Vec<ShortTypeId>>; MAX_RECIPIENT_TYPES],
    message_statistics: [usize; MAX_MESSAGE_TYPES],
    networking: Networking,
    storage: Rc<dyn chunky::ChunkStorage>,
    tuning: Tuning
}

impl ActorSystem {
    /// Create a new actor system that lives in memory only
    pub fn new(networking: Networking, tuning: Tuning) -> ActorSystem {
        Self::new_with_storage(networking, Rc::new(chunky::HeapStorage), tuning)
    }

    /// Create a new actor system that lives in memory and is persisted to disk using Mmapping
    #[cfg(feature = "server")]
    pub fn new_mmap_persisted<P: AsRef<::std::path::Path>>(networking: Networking, directory: &P, tuning: Tuning) -> ActorSystem {
        Self::new_with_storage(networking, Rc::new(chunky::MmapStorage::new(directory.as_ref().to_owned())), tuning)
    }

    /// Create a new actor system backed by any `chunky::ChunkStorage`
    pub fn new_with_storage(networking: Networking, storage: Rc<dyn chunky::ChunkStorage>, tuning: Tuning) -> ActorSystem {
        ActorSystem {
            panic_happened: false,
            trait_implementors: unsafe { make_array!(MAX_RECIPIENT_TYPES, |_| None) },
            actor_registry: TypeRegistry::new(),
            message_registry: TypeRegistry::new(),
            classes: unsafe { make_array!(MAX_RECIPIENT_TYPES, |_| None) },
            message_statistics: [0; MAX_MESSAGE_TYPES],
            networking,
            storage,
            tuning
        }
    }

    /// Register a new actor class with the system (assigning it a type ID)
    pub fn register<A: Actor>(&mut self) {
        // allow use of actor id before it is added
        let actor_id = self.actor_registry.get_or_register::<A>();
        // ...but still make sure it is only added once
        assert!(self.classes[actor_id.as_usize()].is_none());
        // Store pointer to the actor
        let class = Class::new(ActorVTable::new_for_actor_type::<A>(), Rc::clone(&self.storage), &self.tuning);
        self.classes[actor_id.as_usize()] = Some(class);
    }

    /// Register a dummy actor class without allocating any resources or dispatchers.
    /// This can be used to get consistent type ID assignment between different interacting
    /// versions of an actor system, where some actor classes might only ever exist in some versions.
    pub fn register_dummy<D: 'static>(&mut self) {
        let _actor_id = self.actor_registry.get_or_register::<D>();
    }

    /// Register a new actor trait with the system
    pub fn register_trait<T: ActorOrActorTrait>(&mut self) {
        let trait_id = self.actor_registry.get_or_register::<T>();
        self.trait_implementors[trait_id.as_usize()].get_or_insert_with(Vec::new);
    }

    /// Register a message that an actor trait handles
    pub fn register_trait_message<M: Message>(&mut self) {
        self.message_registry.get_or_register::<M>();
    }

    /// Register an actor class as an implementor of an actor trait,
    /// makes it receive messages that are broadcast to the actor trait ID.
    pub fn register_implementor<A: Actor, T: ActorOrActorTrait>(&mut self) {
        let trait_id = self.actor_registry.get_or_register::<T>();
        let actor_id = self.actor_registry.get::<A>();
        self.trait_implementors[trait_id.as_usize()]
            .get_or_insert_with(Vec::new)
            .push(actor_id);
    }

    /// Add a message handler to a registered actor class
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

    /// Add an actor spawner to a registered actor class
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

    /// Manually send a message
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

    /// Get a base RawID for an actor or actor trait
    pub fn id<A: ActorOrActorTrait>(&mut self) -> RawID {
        RawID::new(self.short_id::<A>(), 0, self.networking.machine_id, 0)
    }

    fn short_id<A: ActorOrActorTrait>(&mut self) -> ShortTypeId {
        self.actor_registry.get_or_register::<A>()
    }

    fn single_message_cycle(&mut self) {
        let mut world = World(self as *const Self as *mut Self);

        for maybe_class in self.classes.iter_mut() {
            if let Some(class) = maybe_class.as_mut() {
                class.handle_messages(&mut self.message_statistics, &mut world);
            }
        }
    }

    /// Process and handle all enqueued messages in the system
    /// and the resulting messages, up to a recursion depth of 1000
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

    /// Get a `World` handle for the system.
    pub fn world(&mut self) -> World {
        World(self as *mut Self)
    }

    /// Connect to peers in the networking topology.
    pub fn networking_connect(&mut self) {
        self.networking.connect();
    }

    /// Send and receive messages from peers in the networking topology.
    pub fn networking_send_and_receive(&mut self) {
        self.networking
            .send_and_receive(&mut self.classes, &mut self.trait_implementors);
    }

    /// Mark the local "networking turn" as finished. Networking turns are
    /// used to track and manage time drift between peers in the networking topology.
    pub fn networking_finish_turn(&mut self) -> Option<usize> {
        self.networking.finish_turn()
    }

    /// Get the machine ID of this system in the network
    pub fn networking_machine_id(&self) -> MachineID {
        self.networking.machine_id
    }

    /// Get the local number of networking turns
    pub fn networking_n_turns(&self) -> usize {
        self.networking.n_turns
    }

    /// Get a summary of the **local view** of the networking turn state of all connected peers.
    pub fn networking_debug_all_n_turns(&self) -> HashMap<MachineID, isize> {
        self.networking.debug_all_n_turns()
    }

    /// Get local instance counts of each actor class
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

    /// Get statistics of sent messages per type
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

    /// Reset the counter for message statistics
    pub fn reset_message_statistics(&mut self) {
        self.message_statistics = [0; MAX_MESSAGE_TYPES]
    }

    /// Get the current length of all actor message queues
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

    /// Get a mapping from actor type IDs to full names, for debugging.
    pub fn get_actor_type_id_to_name_mapping(&self) -> HashMap<u16, String> {
        self.actor_registry.short_ids_to_names.iter().map(|(short_id, name)|
            (short_id.as_u16(), name.clone())
        ).collect()
    }
}

/// A handle representing an `ActorSystem` that exposes a safe subset
/// of functionality to be used within actor message handlers - for
/// communication with other actors.
pub struct World(*mut ActorSystem);

// TODO: make this true
unsafe impl Sync for World {}
unsafe impl Send for World {}

impl World {
    /// Send a message to a RawID
    pub fn send<M: Message>(&mut self, receiver: RawID, message: M) {
        unsafe { &mut *self.0 }.send(receiver, message);
    }

    /// Get the RawID of the first local actor of a certain type
    /// (Note: no such actor might exist)
    pub fn local_first<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>()
    }

    /// Get the RawID of the first global actor (among all network peers)
    /// of a certain type (Note: no such actor might exist)
    pub fn global_first<A: ActorOrActorTrait>(&mut self) -> RawID {
        let mut id = unsafe { &mut *self.0 }.id::<A>();
        id.machine = MachineID(0);
        id
    }

    /// Get a RawID for a broadcast to all local actors of a certain type
    pub fn local_broadcast<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>().local_broadcast()
    }

    /// Get a RawID for a broadcast to all global actors
    /// (across all network peers) of a certain type
    pub fn global_broadcast<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>().global_broadcast()
    }

    /// Allocate a new instance id to be used by a to-be-spawned actor
    pub fn allocate_instance_id<A: 'static + Actor>(&mut self) -> RawID {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        let class = system.classes[system.actor_registry.get::<A>().as_usize()].as_mut()
                .expect("Subactor type not found.");
        unsafe { class.instance_store.allocate_id(self.local_broadcast::<A>()) }
    }

    /// Get the machine ID of this system in the network
    pub fn local_machine_id(&mut self) -> MachineID {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.networking.machine_id
    }

    /// Returns whether the system is in a panicked state
    pub fn panic_happened(&self) -> bool {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.panic_happened
    }

    /// Get the name of an actor class by type ID
    pub fn get_actor_name(&mut self, type_id: ShortTypeId) -> &str {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.actor_registry.get_name(type_id)
    }
}