//! Worlds with Friends: worlds hosted by this launcher's core (D-068,
//! docs/worlds-with-friends/).
//!
//! A world is a directory `worlds/<worldId>/` with its `world.json` and the
//! server's working directory. The server is Mojang's own JAR for one of the
//! [`tuple::TUPLES`], run in offline mode on 127.0.0.1; who may enter is decided
//! by the tunnel, not by the server (ADAPTATION A2, A3).

pub mod identity;
pub mod install;
pub mod memory;
pub mod properties;
pub mod tuple;
pub mod world;
