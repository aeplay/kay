use compact::CVec;
use kay::{ActorSystem, World};

#[derive(Compact, Clone)]
pub struct Counter {
    id: CounterID,
    count: u32,
    history: CVec<u32>, 
    listeners: CVec<CounterListenerID>,
}

pub trait CounterListener {
    fn on_count_change(&mut self, new_count: u32, history: &CVec<u32>, world: &mut World);
}

impl Counter {
    pub fn spawn(id: CounterID, initial_count: u32, _: &mut World) -> Counter {
        Counter {
            id,
            count: initial_count,
            history: vec![initial_count].into(),
            listeners: CVec::new(),
        }
    }

    pub fn increment_by(&mut self, increment_amount: u32, world: &mut World) {
        self.count += increment_amount;
        self.history.push(self.count);

        for listener in &self.listeners {
            println!("Notifying {:?}", listener);
            listener.on_count_change(self.count, self.history.clone(), world);
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
    fn on_count_change(&mut self, new_count: u32, history: &CVec<u32>, _: &mut World) {
        println!("Server got new count: {}, history: {:?}!", new_count, history);
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
    fn on_count_change(&mut self, new_count: u32, history: &CVec<u32>, _: &mut World) {
        let history_str = format!("{:?}", history);
        #[cfg(feature = "browser")]
        {
            js!{
                console.log("Browser got new count: ", @ {new_count}, "history", @ {history_str});
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
