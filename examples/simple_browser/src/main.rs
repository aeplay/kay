#![feature(proc_macro)]

extern crate kay;
extern crate kay_simple_example_common;

#[macro_use]
extern crate stdweb;

use kay::{Actor, ActorSystem, Networking};
use kay_simple_example_common::counter;

use std::cell::RefCell;
use std::rc::Rc;

const STEP_SPEED_MS: u32 = 100;

fn main() {
    stdweb::initialize();

    js! {
        console.log("Starting actor system...");
    }

    let mut system = ActorSystem::new(Networking::new(1, vec!["localhost:9999","wsclient"], 50_000, 30, 10));
    counter::setup(&mut system);

    js! {
        console.log("Connecting network...")
    }

    system.networking_connect();

    let world = &mut system.world();

    counter::BrowserLoggerID::spawn(counter::Counter::global_broadcast(world), world);

    system.process_all_messages();

    let looper = Rc::new(RefCell::new(Looper { system }));

    looper.borrow_mut().turn(looper.clone());
}

struct Looper {
    system: ActorSystem,
}

impl Looper {
    fn turn(&mut self, rc: Rc<RefCell<Self>>) {
        let system = &mut self.system;
        let world = &mut system.world();

        system.networking_send_and_receive();

        counter::Counter::global_broadcast(world).increment_by(13, world);

        system.process_all_messages();

        system.networking_finish_turn();
        system.networking_send_and_receive();

        stdweb::web::set_timeout(
            move || {
                let next_rc = rc.clone();
                rc.borrow_mut().turn(next_rc);
            },
            STEP_SPEED_MS,
        );
    }
}
