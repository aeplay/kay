use compact::CVec;
use kay::{ActorSystem, World};

#[derive(Compact, Clone)]
pub struct Counter {
    id: CounterID,
    count: u32,
    listeners: CVec<CounterListenerID>,
}

pub trait CounterListener {
    fn on_count_change(&mut self, new_count: u32, world: &mut World);
}

impl Counter {
    pub fn spawn(id: CounterID, initial_count: u32, _: &mut World) -> Counter {
        Counter {
            id,
            count: initial_count,
            listeners: CVec::new(),
        }
    }

    pub fn increment_by(&mut self, increment_amount: u32, world: &mut World) {
        self.count += increment_amount;

        for listener in &self.listeners {
            println!("Notifying {:?}", listener);
            listener.on_count_change(self.count, world);
        }
    }

    pub fn add_listener(&mut self, listener: CounterListenerID, _: &mut World) {
        self.listeners.push(listener);
    }
}

#[derive(Compact, Clone)]
pub struct ServerLogger {
    id: ServerLoggerID,
}

impl ServerLogger {
    pub fn spawn(id: ServerLoggerID, counter_id: CounterID, world: &mut World) -> ServerLogger {
        counter_id.add_listener(id.into(), world);
        ServerLogger { id }
    }
}

impl CounterListener for ServerLogger {
    fn on_count_change(&mut self, new_count: u32, _: &mut World) {
        println!("Server got new count: {}!", new_count);
    }
}

#[derive(Compact, Clone)]
pub struct BrowserLogger {
    id: BrowserLoggerID,
}

impl BrowserLogger {
    pub fn spawn(id: BrowserLoggerID, counter_id: CounterID, world: &mut World) -> BrowserLogger {
        counter_id.add_listener(id.into(), world);
        BrowserLogger { id }
    }
}

impl CounterListener for BrowserLogger {
    fn on_count_change(&mut self, new_count: u32, _: &mut World) {
        let new_count = new_count as u32;
        #[cfg(feature = "browser")]
        {
            js!{
                console.log("Browser got new count: ", @ {new_count});
            }
        }
    }
}

pub fn setup(system: &mut ActorSystem) {
    system.register::<Counter>();
    system.register::<ServerLogger>();
    system.register::<BrowserLogger>();
    auto_setup(system);
}

mod kay_auto;
pub use self::kay_auto::*;
