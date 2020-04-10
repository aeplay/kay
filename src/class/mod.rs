use crate::messaging::HandlerFnRef;
use crate::messaging::Message;
use crate::actor::Actor;
use crate::type_registry::ShortTypeId;
use crate::actor_system::{World, MAX_MESSAGE_TYPES};
use crate::id::{broadcast_instance_id, RawID, TypedID};
use crate::messaging::{Fate, Packet};
use crate::tuning::Tuning;
use compact::Compact;
use std::rc::Rc;

mod instance_store;
use self::instance_store::InstanceStore;
pub mod inbox;
use self::inbox::{Inbox, DispatchablePacket};

pub struct Class {
    pub instance_store: InstanceStore,
    pub v_table: ActorVTable,
    pub inbox: Inbox
}

pub struct ActorVTable {
    pub message_handlers: [MessageHandler; MAX_MESSAGE_TYPES],
    pub state_v_table: ActorStateVTable,
    pub type_name: &'static str,
}

pub struct ActorStateVTable {
    pub is_still_compact: Box<dyn Fn(*const ()) -> bool>,
    pub total_size_bytes: Box<dyn Fn(*const ()) -> usize>,
    pub compact_behind: Box<dyn Fn(*mut (), *mut ())>,
    pub drop: Box<dyn Fn(*mut ())>,
    pub get_raw_id: Box<dyn Fn(*const ()) -> RawID>,
    pub set_raw_id: Box<dyn Fn(*mut (), RawID)>,
    pub typical_size: usize
}

impl ActorVTable {
    pub fn new_for_actor_type<A: Actor>() -> ActorVTable {
        let actor_name = unsafe { ::std::intrinsics::type_name::<A>() };
        ActorVTable {
            message_handlers: unsafe { make_array!(MAX_MESSAGE_TYPES, |_| MessageHandler::Unassigned) },
            type_name: actor_name,
            state_v_table: ActorStateVTable {
                is_still_compact: Box::new(|act: *const ()| unsafe {(*(act as *const A)).is_still_compact()}),
                total_size_bytes: Box::new(|act: *const ()| unsafe {(*(act as *const A)).total_size_bytes()}),
                compact_behind: Box::new(|source: *mut (), dest: *mut ()| unsafe{Compact::compact_behind(source as *mut A, dest as *mut A)}),
                drop: Box::new(|act: *mut ()| unsafe{::std::ptr::drop_in_place(act as *mut A)}),
                get_raw_id: Box::new(|act: *const ()| unsafe{(*(act as *const A)).id().as_raw()}),
                set_raw_id: Box::new(|act: *mut (), id: RawID| unsafe{(*(act as *mut A)).set_id(id)}),
                typical_size: A::typical_size()
            }
        }
    }
}

pub enum MessageHandler {
    Unassigned,
    OnMessage{handler: Box<HandlerFnRef>, critical: bool},
    OnSpawn{spawner: Box<dyn Fn(*const (), &mut World, &mut InstanceStore, &ActorStateVTable)>, critical: bool}
}

impl Class {
    pub fn new(v_table: ActorVTable, storage: Rc<dyn chunky::ChunkStorage>, tuning: &Tuning) -> Self {
        let ident: chunky::Ident = v_table.type_name.split("<").map(|piece|
            piece.split("::").last().unwrap_or("")
        ).collect::<Vec<_>>().join("<").replace("<", "(").replace(">", ")").into();
        Class {
            instance_store: InstanceStore::new(&ident, v_table.state_v_table.typical_size, Rc::clone(&storage), tuning),
            inbox: Inbox::new(&ident.sub("inbx"), storage, tuning),
            v_table,
        }
    }

    pub fn add_handler<A: Actor, M: Message, F: Fn(&M, &mut A, &mut World) -> Fate + 'static>(
        &mut self,
        message_id: ShortTypeId,
        handler: F,
        critical: bool,
    ) {
        self.v_table.message_handlers[message_id.as_usize()] = MessageHandler::OnMessage {
                handler: Box::new(move |actor_ptr: *mut (), packet_ptr: *const (), world: &mut World| -> Fate {
                    unsafe {
                        let actor = &mut *(actor_ptr as *mut A);
                        let packet = & *(packet_ptr as *const Packet<M>);
                        handler(&packet.message, actor, world)
                    }
                }),
                critical
        };
    }

    pub fn add_spawner<A: Actor, M: Message, F: Fn(&M, &mut World) -> A + 'static>(
        &mut self,
        message_id: ShortTypeId,
        constructor: F,
        critical: bool,
    ) {
        self.v_table.message_handlers[message_id.as_usize()] = MessageHandler::OnSpawn {
            spawner: Box::new(move |packet_ptr: *const (), world: &mut World, store: &mut InstanceStore, intrinsics: &ActorStateVTable| {
                unsafe {
                    let packet = &*(packet_ptr as *const Packet<M>);
                    let mut instance = constructor(&packet.message, world);
                    store.add(&mut instance as *mut A as *mut (), intrinsics, true);
                    ::std::mem::forget(instance);
                }
            }),
            critical
        };
    }

    pub fn handle_messages(&mut self, message_statistics: &mut [usize], world: &mut World) {
        for DispatchablePacket { message_type, packet_ptr} in self.inbox.drain() {
            Self::dispatch_packet(&mut self.instance_store, &self.v_table, message_type, packet_ptr, world);
            message_statistics[message_type.as_usize()] += 1;
        }
    }

    fn dispatch_packet(
        instance_store: &mut InstanceStore,
        v_table: &ActorVTable,
        message_type: ShortTypeId,
        packet_ptr: *const (),
        world: &mut World,
    )
    {
        let handler_kind = &v_table.message_handlers[message_type.as_usize()];

        if let MessageHandler::OnMessage{ref handler, critical} = handler_kind {
            if *critical || !world.panic_happened() {
                let recipient_id = unsafe {(*(packet_ptr as *const Packet<()>)).recipient_id};
                if recipient_id.instance_id == broadcast_instance_id() {
                    instance_store.receive_broadcast(packet_ptr, world, handler, &v_table.state_v_table);
                } else {
                    instance_store.receive_instance(recipient_id, packet_ptr, world, handler,  &v_table.state_v_table);
                }
            }
        } else if let MessageHandler::OnSpawn{spawner, critical} = handler_kind {
            if *critical || !world.panic_happened() {
                spawner(packet_ptr, world, instance_store, &v_table.state_v_table);
            }
        } else {
            if !world.panic_happened() {
                panic!("Handler for message {} not found in {}", message_type.as_usize(), v_table.type_name);
            }
        }
    }
}