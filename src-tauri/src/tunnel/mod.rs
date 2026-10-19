//! The tunnel between a guest's game and a host's world server (ADAPTATION
//! A3, A5, spec 07): iroh connections carrying one bidirectional stream per
//! game connection, admitted by a ticket and the player's login name.

pub mod bridge;
pub mod endpoint;
pub mod frame;
pub mod guest;
pub mod handshake;
pub mod host;

#[cfg(test)]
mod tests;
