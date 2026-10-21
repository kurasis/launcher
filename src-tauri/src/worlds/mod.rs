//! Worlds with Friends: worlds hosted by this launcher's core (D-068,
//! docs/worlds-with-friends/).
//!
//! A world is a directory `worlds/<worldId>/` with its `world.json` and the
//! server's working directory. The server is Mojang's own JAR of a version
//! [`tuple::version`] accepts, run in offline mode on 127.0.0.1; who may enter
//! is decided by the tunnel, not by the server (ADAPTATION A2, A3).

pub mod app;
#[cfg(test)]
pub(crate) mod fake_service;
pub mod host;
pub mod hosting;
pub mod identity;
pub mod install;
pub mod joining;
pub mod memory;
pub mod output;
pub mod pack;
pub mod ping;
pub mod players;
pub mod properties;
pub mod protocol;
pub mod rcon;
pub mod receipt;
pub mod safety;
pub mod service;
pub mod tuple;
pub mod update;
pub mod world;
