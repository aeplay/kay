//! `Kay` is a high-performance actor system, suitable for simulating millions of entities.
//!
//! In `Kay`, actors concurrently send and receive asynchronous messages, but are
//! otherwise completely isloated from each other. Actors can only mutate their own state.
//!
//! Have a look at [`ActorSystem`](struct.ActorSystem.html), [`World`](struct.World.html)
//! and [`Swarm`](swarm/struct.Swarm.html) to understand the main abstractions.
//!
//! Current Shortcomings:
//!
//! * Can't deal with messages to dead actors (undefined, often very confusing behaviour)

#![warn(missing_docs)]
#![feature(core_intrinsics)]
#![feature(optin_builtin_traits)]
#![feature(specialization)]
#![feature(box_syntax)]
#![feature(nonzero)]
#![feature(tcpstream_connect_timeout)]
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

mod actor_system;
mod external;
mod id;
mod inbox;
mod messaging;
mod networking;
mod slot_map;
mod swarm;
mod type_registry;

pub use self::actor_system::{Actor, ActorSystem, TraitIDFrom, World};
pub use self::external::External;
pub use self::id::{MachineID, RawID, TypedID};
pub use self::messaging::{Fate, Message, Packet};
pub use self::networking::Networking;
