//! Microsoft OAuth, Xbox and Minecraft service exchange, entitlement decision and
//! secret storage (docs/spec/03-AUTHENTICATION.md, docs/AUTH-CONTRACT.md).

pub mod client_id;
pub mod contract;
pub mod decision;
pub mod flow;
pub mod http;
pub mod loopback;
pub mod pkce;
pub mod protocol;
pub mod service;
#[cfg(test)]
pub mod test_server;
pub mod vault;
