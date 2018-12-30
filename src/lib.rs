#![warn(missing_docs)]
#![feature(core_intrinsics)]
#![feature(optin_builtin_traits)]
#![feature(specialization)]
#![feature(box_syntax)]
extern crate chunky;
extern crate compact;
#[macro_use]
extern crate compact_macros;
extern crate byteorder;
extern crate core;
#[cfg(feature = "browser")]
#[macro_use]
extern crate stdweb;
#[cfg(feature = "server")]
extern crate tungstenite;
extern crate url;
#[cfg(feature = "serde-serialization")]
#[macro_use]
extern crate serde_derive;
#[cfg(feature = "serde-serialization")]
extern crate serde;

macro_rules! make_array {
    ($n:expr, $constructor:expr) => {{
        let mut items: [_; $n] = ::std::mem::uninitialized();
        for (i, place) in items.iter_mut().enumerate() {
            ::std::ptr::write(place, $constructor(i));
        }
        items
    }};
}

mod actor;
mod actor_system;
mod external;
mod id;
mod class;
mod messaging;
mod networking;
mod storage_aware;
mod type_registry;

pub use self::actor::{Actor, ActorOrActorTrait, TraitIDFrom};
pub use self::actor_system::{ActorSystem, World};
pub use self::external::External;
pub use self::id::{MachineID, RawID, TypedID};
pub use self::messaging::{Fate, Message, Packet};
pub use self::networking::Networking;