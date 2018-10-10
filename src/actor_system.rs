use super::actor::{Actor, ActorOrActorTrait};
use super::id::{MachineID, RawID, TypedID};
use super::inbox::{DispatchablePacket, Inbox};
use super::instance_store::InstanceStore;
use super::messaging::{Fate, Message, Packet};
use super::networking::Networking;
use super::type_registry::{ShortTypeId, TypeRegistry};

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

struct Dispatcher {
    function: Box<Fn(*const (), &mut World)>,
    critical: bool,
}

const MAX_RECIPIENT_TYPES: usize = 64;
const MAX_MESSAGE_TYPES: usize = 256;

/// The main thing inside of which all the magic happens.
///
/// An `ActorSystem` contains the states of all registered actor instances,
/// message inboxes (queues) for each registered Actor type,
/// and message dispatchers for each registered (`Actor`, `Message`) pair.
///
/// It can be controlled from the outside to do message passing and handling in turns.
pub struct ActorSystem {
    /// Flag that the system is in a panicked state
    pub panic_happened: bool,
    /// Flag that the system is shutting down
    pub shutting_down: bool,
    inboxes: [Option<Inbox>; MAX_RECIPIENT_TYPES],
    implementors: [Option<Vec<ShortTypeId>>; MAX_RECIPIENT_TYPES],
    actor_registry: TypeRegistry,
    instance_stores: [Option<*mut u8>; MAX_RECIPIENT_TYPES],
    message_registry: TypeRegistry,
    dispatchers: [[Option<Dispatcher>; MAX_MESSAGE_TYPES]; MAX_RECIPIENT_TYPES],
    actors_as_countables: Vec<(String, *const InstancesCountable)>,
    message_statistics: [usize; MAX_MESSAGE_TYPES],
    networking: Networking,
}

macro_rules! make_array {
    ($n:expr, $constructor:expr) => {{
        let mut items: [_; $n] = ::std::mem::uninitialized();
        for (i, place) in items.iter_mut().enumerate() {
            ::std::ptr::write(place, $constructor(i));
        }
        items
    }};
}

impl ActorSystem {
    /// Create a new ActorSystem (usually only one per application is needed).
    /// Expects to get a panic callback as a parameter that is called when
    /// an actor panics during message handling and can thus be used to
    /// for example display the panic error message.
    ///
    /// Note that after an actor panicking, the whole `ActorSystem` switches
    /// to a panicked state and only passes messages anymore which have been
    /// marked as *critically receiveable* using `add_handler`.
    pub fn new(networking: Networking) -> ActorSystem {
        ActorSystem {
            panic_happened: false,
            shutting_down: false,
            inboxes: unsafe { make_array!(MAX_RECIPIENT_TYPES, |_| None) },
            implementors: unsafe { make_array!(MAX_RECIPIENT_TYPES, |_| None) },
            actor_registry: TypeRegistry::new(),
            message_registry: TypeRegistry::new(),
            instance_stores: [None; MAX_RECIPIENT_TYPES],
            dispatchers: unsafe {
                make_array!(MAX_RECIPIENT_TYPES, |_| make_array!(
                    MAX_MESSAGE_TYPES,
                    |_| None
                ))
            },
            actors_as_countables: Vec::new(),
            message_statistics: [0; MAX_MESSAGE_TYPES],
            networking,
        }
    }

    /// Register a new Actor type with the system
    pub fn register<A: Actor>(&mut self) {
        // allow use of actor id before it is added
        let actor_id = self.actor_registry.get_or_register::<A>();
        assert!(self.inboxes[actor_id.as_usize()].is_none());
        let actor_name = unsafe { ::std::intrinsics::type_name::<A>() };
        self.inboxes[actor_id.as_usize()] =
            Some(Inbox::new(&::chunky::Ident::from(actor_name).sub("inbox")));
        // ...but still make sure it is only added once
        assert!(self.instance_stores[actor_id.as_usize()].is_none());
        // Store pointer to the actor
        let actor_pointer = Box::into_raw(Box::new(InstanceStore::<A>::new()));
        self.instance_stores[actor_id.as_usize()] = Some(actor_pointer as *mut u8);
        self.actors_as_countables.push((
            self.actor_registry
                .get_name(self.actor_registry.get::<A>())
                .clone(),
            actor_pointer,
        ));
    }

    /// Register a new dummy Actor type with the system - useful if this Actor
    /// is only really implemented on other node kinds, but Actor type IDs need to be
    /// registered in the same order without gaps
    pub fn register_dummy<D: 'static>(&mut self) {
        let _actor_id = self.actor_registry.get_or_register::<D>();
    }

    /// Register a trait to give it a type ID and be able to send broadcast messages
    /// to its implementors
    pub fn register_trait<T: ActorOrActorTrait>(&mut self) {
        let trait_id = self.actor_registry.get_or_register::<T>();
        self.implementors[trait_id.as_usize()].get_or_insert_with(Vec::new);
    }

    /// Register a message type that might first only appear in an actor trait
    /// and that might never have an actual handler implementation on this node kind
    pub fn register_trait_message<M: Message>(&mut self) {
        self.message_registry.get_or_register::<M>();
    }

    /// Register an actor as an implementor of an actor trait, so it can receive broadcast
    /// messages directed at all implementors of the trait
    pub fn register_implementor<A: Actor, T: ActorOrActorTrait>(&mut self) {
        let trait_id = self.actor_registry.get_or_register::<T>();
        let actor_id = self.actor_registry.get::<A>();
        self.implementors[trait_id.as_usize()]
            .get_or_insert_with(Vec::new)
            .push(actor_id);
    }

    /// Register a handler for an Actor type and Message type.
    pub fn add_handler<A: Actor, M: Message, F: Fn(&M, &mut A, &mut World) -> Fate + 'static>(
        &mut self,
        handler: F,
        critical: bool,
    ) {
        let actor_id = self.actor_registry.get::<A>();
        let message_id = self.message_registry.get_or_register::<M>();
        // println!(
        //     "adding to {} inbox for {}",
        //     unsafe { ::std::intrinsics::type_name::<A>() },
        //     unsafe { ::std::intrinsics::type_name::<M>() }
        // );

        #[cfg_attr(feature = "cargo-clippy", allow(cast_ptr_alignment))]
        let instance_store_ptr = self.instance_stores[actor_id.as_usize()]
            .expect("Actor not added yet")
            as *mut InstanceStore<A>;

        self.dispatchers[actor_id.as_usize()][message_id.as_usize()] = Some(Dispatcher {
            function: Box::new(move |packet_ptr: *const (), world: &mut World| unsafe {
                let packet = &*(packet_ptr as *const Packet<M>);

                // println!(
                //     "{} Handling packet with message {}",
                //     ::std::intrinsics::type_name::<A>(),
                //     ::std::intrinsics::type_name::<M>()
                // );

                (*instance_store_ptr).dispatch_packet(packet, &handler, world);

                // TODO: not sure if this is the best place to drop the message
                ::std::ptr::drop_in_place(packet_ptr as *mut Packet<M>);
            }),
            critical,
        });
    }

    /// Register a handler that constructs an instance of an Actor type, given an RawID
    pub fn add_spawner<A: Actor, M: Message, F: Fn(&M, &mut World) -> A + 'static>(
        &mut self,
        constructor: F,
        critical: bool,
    ) {
        let actor_id = self.actor_registry.get::<A>();
        let message_id = self.message_registry.get_or_register::<M>();
        // println!("adding to {} inbox for {}",
        //          unsafe { ::std::intrinsics::type_name::<A>() },
        //          unsafe { ::std::intrinsics::type_name::<M>() });

        #[cfg_attr(feature = "cargo-clippy", allow(cast_ptr_alignment))]
        let instance_store_ptr = self.instance_stores[actor_id.as_usize()]
            .expect("Actor not added yet")
            as *mut InstanceStore<A>;

        self.dispatchers[actor_id.as_usize()][message_id.as_usize()] = Some(Dispatcher {
            function: Box::new(move |packet_ptr: *const (), world: &mut World| unsafe {
                let packet = &*(packet_ptr as *const Packet<M>);

                let mut instance = constructor(&packet.message, world);
                (*instance_store_ptr).add_manually_with_id(&mut instance, instance.id().as_raw());

                ::std::mem::forget(instance);

                // TODO: not sure if this is the best place to drop the message
                ::std::ptr::drop_in_place(packet_ptr as *mut Packet<M>);
            }),
            critical,
        });
    }

    /// Send a message to the actor(s) with a given `RawID`.
    /// This is only used to send messages into the system from outside.
    /// Inside actor message handlers you always have access to a
    /// [`World`](struct.World.html) that allows you to send messages.
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
            if let Some(inbox) = self.inboxes[recipient.type_id.as_usize()].as_mut() {
                inbox.put(packet, &self.message_registry);
                return; // A weird replacement for 'else', helps with mut borrows of self.inboxes
            }

            if let Some(implementors) = self.implementors[recipient.type_id.as_usize()].as_ref() {
                for implementor_type_id in implementors {
                    if let Some(inbox) = self.inboxes[implementor_type_id.as_usize()].as_mut() {
                        inbox.put(packet.clone(), &self.message_registry);
                    } else {
                        panic!(
                            "{}:{} has no inbox for {}",
                            self.actor_registry.get_name(*implementor_type_id),
                            self.actor_registry.get_name(recipient.type_id),
                            self.message_registry
                                .get_name(self.message_registry.get::<M>(),)
                        );
                    }
                }
            } else {
                panic!(
                    "{} has no inbox for {} - or the trait has no implementors",
                    self.actor_registry.get_name(recipient.type_id),
                    self.message_registry
                        .get_name(self.message_registry.get::<M>(),)
                );
            }
        }
    }

    /// Get the base RawID of an Actor type
    pub fn id<A: ActorOrActorTrait>(&mut self) -> RawID {
        RawID::new(self.short_id::<A>(), 0, self.networking.machine_id, 0)
    }

    fn short_id<A: ActorOrActorTrait>(&mut self) -> ShortTypeId {
        self.actor_registry.get_or_register::<A>()
    }

    fn single_message_cycle(&mut self) {
        // TODO: separate inbox reading end from writing end
        //       to be able to use (several) mut refs here
        let mut world = World(self as *const Self as *mut Self);

        for (recipient_type_idx, maybe_inbox) in self.inboxes.iter_mut().enumerate() {
            if let Some(recipient_type) = ShortTypeId::new(recipient_type_idx as u16) {
                if let Some(inbox) = maybe_inbox.as_mut() {
                    for DispatchablePacket {
                        message_type,
                        packet_ptr,
                    } in inbox.drain()
                    {
                        if let Some(handler) = self.dispatchers[recipient_type.as_usize()]
                            [message_type.as_usize()].as_mut()
                        {
                            if handler.critical || !self.panic_happened {
                                (handler.function)(packet_ptr, &mut world);
                                self.message_statistics[message_type.as_usize()] += 1;
                            }
                        } else {
                            panic!(
                                "Dispatcher not found ({} << {})",
                                self.actor_registry.get_name(recipient_type),
                                self.message_registry.get_name(message_type)
                            );
                        }
                    }
                }
            }
        }
    }

    /// Processes all sent messages, and messages which are in turn sent
    /// during the handling of messages, up to a recursion depth of 1000.
    ///
    /// This is typically called in the main loop of an application.
    ///
    /// By sending different "top-level commands" into the system and calling
    /// `process_all_messages` inbetween, different aspects of an application
    /// (for example, UI, simulation, rendering) can be run isolated from each other,
    /// in a fixed order of "turns" during each main-loop iteration.
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

    /// Get a world context directly from the system, typically to send messages from outside
    pub fn world(&mut self) -> World {
        World(self as *mut Self)
    }

    /// Connect to all peers in the network
    pub fn networking_connect(&mut self) {
        self.networking.connect();
    }

    /// Send queued outbound messages and take incoming queued messages
    /// and forward them to their local target recipient(s)
    pub fn networking_send_and_receive(&mut self) {
        self.networking
            .send_and_receive(&mut self.inboxes, &mut self.implementors);
    }

    /// Finish the current networking turn and wait for peers which lag behind
    /// based on their turn number. This is the main backpressure mechanism.
    pub fn networking_finish_turn(&mut self) -> Option<usize> {
        self.networking.finish_turn()
    }

    /// The machine index of this machine within the network of peers
    pub fn networking_machine_id(&self) -> MachineID {
        self.networking.machine_id
    }

    /// The current network turn this machine is in. Used to keep track
    /// if this machine lags behind or runs fast compared to its peers
    pub fn networking_n_turns(&self) -> usize {
        self.networking.n_turns
    }

    /// Return a debug message containing the current local view of
    /// network turn progress of all peers in the network
    pub fn networking_debug_all_n_turns(&self) -> HashMap<MachineID, isize> {
        self.networking.debug_all_n_turns()
    }

    /// Get current instance counts for all actory types
    pub fn get_instance_counts(&self) -> HashMap<String, usize> {
        self.actors_as_countables
            .iter()
            .map(|&(ref actor_name, countable_ptr)| {
                (
                    actor_name.split("::").last().unwrap().replace(">", ""),
                    unsafe { (*countable_ptr).instance_count() },
                )
            }).collect()
    }

    /// Get number of processed messages per message type since last reset
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

    /// Reset count of processed messages
    pub fn reset_message_statistics(&mut self) {
        self.message_statistics = [0; MAX_MESSAGE_TYPES]
    }

    /// Get current inbox queue lengths per actor type
    pub fn get_queue_lengths(&self) -> HashMap<String, usize> {
        #[cfg(feature = "server")]
        let connection_queue_length = None;
        #[cfg(feature = "browser")]
        let connection_queue_length = self
            .networking
            .main_out_connection()
            .map(|connection| ("NETWORK QUEUE".to_owned(), connection.in_queue_len()));

        self.inboxes
            .iter()
            .enumerate()
            .filter_map(|(i, maybe_inbox)| {
                maybe_inbox.as_ref().map(|inbox| {
                    let actor_name = self
                        .actor_registry
                        .get_name(ShortTypeId::new(i as u16).unwrap());
                    (actor_name.to_owned(), inbox.len())
                })
            }).chain(connection_queue_length)
            .collect()
    }
}

/// Gives limited access to an [`ActorSystem`](struct.ActorSystem.html) (typically
/// from inside, in a message handler) to identify other actors and send messages to them.
pub struct World(*mut ActorSystem);

// TODO: make this true
unsafe impl Sync for World {}
unsafe impl Send for World {}

impl World {
    /// Send a message to a (sub-)actor with the given RawID.
    ///
    /// ```
    /// world.send(child_id, Update { dt: 1.0 });
    /// ```
    pub fn send<M: Message>(&mut self, receiver: RawID, message: M) {
        unsafe { &mut *self.0 }.send(receiver, message);
    }

    /// Get the RawID of the first machine-local instance of an actor.
    pub fn local_first<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>()
    }

    /// Get the RawID of the first instance of an actor on machine 0
    pub fn global_first<A: ActorOrActorTrait>(&mut self) -> RawID {
        let mut id = unsafe { &mut *self.0 }.id::<A>();
        id.machine = MachineID(0);
        id
    }

    /// Get the RawID for a broadcast to all machine-local instances of an actor.
    pub fn local_broadcast<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>().local_broadcast()
    }

    /// Get the RawID for a global broadcast to all instances of an actor on all machines.
    pub fn global_broadcast<A: ActorOrActorTrait>(&mut self) -> RawID {
        unsafe { &mut *self.0 }.id::<A>().global_broadcast()
    }

    /// Synchronously allocate a instance id for a instance
    /// that will later manually be added to a InstanceStore
    pub fn allocate_instance_id<A: 'static + Actor>(&mut self) -> RawID {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        let instance_store = unsafe {
            #[cfg_attr(feature = "cargo-clippy", allow(cast_ptr_alignment))]
            &mut *(system.instance_stores[system.actor_registry.get::<A>().as_usize()]
                .expect("Subactor type not found.") as *mut InstanceStore<A>)
        };
        unsafe { instance_store.allocate_id(self.local_broadcast::<A>()) }
    }

    /// Get the id of the machine that we're currently in
    pub fn local_machine_id(&mut self) -> MachineID {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.networking.machine_id
    }

    /// Signal intent to shutdown the actor system
    pub fn shutdown(&mut self) {
        let system: &mut ActorSystem = unsafe { &mut *self.0 };
        system.shutting_down = true;
    }
}

pub trait InstancesCountable {
    fn instance_count(&self) -> usize;
}

impl<T> InstancesCountable for T {
    default fn instance_count(&self) -> usize {
        1
    }
}
