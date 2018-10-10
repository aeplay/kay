#![doc = r" This is all auto-generated. Do not touch."]
#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use kay::{Actor, ActorSystem, Fate, RawID, TraitIDFrom, TypedID};
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct CounterListenerID {
    _raw_id: RawID,
}
impl TypedID for CounterListenerID {
    unsafe fn from_raw(id: RawID) -> Self {
        CounterListenerID { _raw_id: id }
    }
    fn as_raw(&self) -> RawID {
        self._raw_id
    }
}
impl<A: Actor + CounterListener> TraitIDFrom<A> for CounterListenerID {}
impl CounterListenerID {
    pub fn on_count_change(&self, new_count: u32, history: CVec<u32>, world: &mut World) {
        world.send(
            self.as_raw(),
            MSG_CounterListener_on_count_change(new_count, history),
        );
    }
    pub fn register_handlers<A: Actor + CounterListener>(system: &mut ActorSystem) {
        system.add_handler::<A, _, _>(
            |&MSG_CounterListener_on_count_change(new_count, ref history), instance, world| {
                instance.on_count_change(new_count, history, world);
                Fate::Live
            },
            false,
        );
    }
}
#[allow(non_camel_case_types)]
#[derive(Compact, Clone)]
#[repr(C)]
struct MSG_CounterListener_on_count_change(pub u32, pub CVec<u32>);
impl Actor for Counter {
    type ID = CounterID;
    fn id(&self) -> Self::ID {
        self.id
    }
    unsafe fn set_id(&mut self, id: RawID) {
        self.id = Self::ID::from_raw(id);
    }
}
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct CounterID {
    _raw_id: RawID,
}
impl TypedID for CounterID {
    unsafe fn from_raw(id: RawID) -> Self {
        CounterID { _raw_id: id }
    }
    fn as_raw(&self) -> RawID {
        self._raw_id
    }
}
impl Actor for ServerLogger {
    type ID = ServerLoggerID;
    fn id(&self) -> Self::ID {
        self.id
    }
    unsafe fn set_id(&mut self, id: RawID) {
        self.id = Self::ID::from_raw(id);
    }
}
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ServerLoggerID {
    _raw_id: RawID,
}
impl TypedID for ServerLoggerID {
    unsafe fn from_raw(id: RawID) -> Self {
        ServerLoggerID { _raw_id: id }
    }
    fn as_raw(&self) -> RawID {
        self._raw_id
    }
}
impl Actor for BrowserLogger {
    type ID = BrowserLoggerID;
    fn id(&self) -> Self::ID {
        self.id
    }
    unsafe fn set_id(&mut self, id: RawID) {
        self.id = Self::ID::from_raw(id);
    }
}
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BrowserLoggerID {
    _raw_id: RawID,
}
impl TypedID for BrowserLoggerID {
    unsafe fn from_raw(id: RawID) -> Self {
        BrowserLoggerID { _raw_id: id }
    }
    fn as_raw(&self) -> RawID {
        self._raw_id
    }
}
impl CounterID {
    pub fn increment_by(&self, increment_amount: u32, world: &mut World) {
        world.send(self.as_raw(), MSG_Counter_increment_by(increment_amount));
    }
    pub fn add_listener(&self, listener: CounterListenerID, world: &mut World) {
        world.send(self.as_raw(), MSG_Counter_add_listener(listener));
    }
    pub fn spawn(initial_count: u32, world: &mut World) -> Self {
        let id = unsafe { CounterID::from_raw(world.allocate_instance_id::<Counter>()) };
        let instance_store = world.local_broadcast::<Counter>();
        world.send(instance_store, MSG_Counter_spawn(id, initial_count));
        id
    }
}
#[allow(non_camel_case_types)]
#[derive(Compact, Clone)]
struct MSG_Counter_increment_by(pub u32);
#[allow(non_camel_case_types)]
#[derive(Compact, Clone)]
struct MSG_Counter_add_listener(pub CounterListenerID);
#[allow(non_camel_case_types)]
#[derive(Compact, Clone)]
struct MSG_Counter_spawn(pub CounterID, pub u32);
impl ServerLoggerID {
    pub fn spawn(counter_id: CounterID, world: &mut World) -> Self {
        let id = unsafe { ServerLoggerID::from_raw(world.allocate_instance_id::<ServerLogger>()) };
        let instance_store = world.local_broadcast::<ServerLogger>();
        world.send(instance_store, MSG_ServerLogger_spawn(id, counter_id));
        id
    }
}
#[allow(non_camel_case_types)]
#[derive(Compact, Clone)]
struct MSG_ServerLogger_spawn(pub ServerLoggerID, pub CounterID);
impl BrowserLoggerID {
    pub fn spawn(counter_id: CounterID, world: &mut World) -> Self {
        let id =
            unsafe { BrowserLoggerID::from_raw(world.allocate_instance_id::<BrowserLogger>()) };
        let instance_store = world.local_broadcast::<BrowserLogger>();
        world.send(instance_store, MSG_BrowserLogger_spawn(id, counter_id));
        id
    }
}
#[allow(non_camel_case_types)]
#[derive(Compact, Clone)]
struct MSG_BrowserLogger_spawn(pub BrowserLoggerID, pub CounterID);
impl Into<CounterListenerID> for ServerLoggerID {
    fn into(self) -> CounterListenerID {
        unsafe { CounterListenerID::from_raw(self.as_raw()) }
    }
}
impl Into<CounterListenerID> for BrowserLoggerID {
    fn into(self) -> CounterListenerID {
        unsafe { CounterListenerID::from_raw(self.as_raw()) }
    }
}
#[allow(unused_variables)]
#[allow(unused_mut)]
pub fn auto_setup(system: &mut ActorSystem) {
    system.add_handler::<Counter, _, _>(
        |&MSG_Counter_increment_by(increment_amount), instance, world| {
            instance.increment_by(increment_amount, world);
            Fate::Live
        },
        false,
    );
    system.add_handler::<Counter, _, _>(
        |&MSG_Counter_add_listener(listener), instance, world| {
            instance.add_listener(listener, world);
            Fate::Live
        },
        false,
    );
    system.add_spawner::<Counter, _, _>(
        |&MSG_Counter_spawn(id, initial_count), world| Counter::spawn(id, initial_count, world),
        false,
    );
    CounterListenerID::register_handlers::<ServerLogger>(system);
    system.add_spawner::<ServerLogger, _, _>(
        |&MSG_ServerLogger_spawn(id, counter_id), world| ServerLogger::spawn(id, counter_id, world),
        false,
    );
    CounterListenerID::register_handlers::<BrowserLogger>(system);
    system.add_spawner::<BrowserLogger, _, _>(
        |&MSG_BrowserLogger_spawn(id, counter_id), world| {
            BrowserLogger::spawn(id, counter_id, world)
        },
        false,
    );
}
