//! wtf's core (spec §1 "Architecture"): a trace and what is shown of it, and every operation on
//! that, with no user interface. The terminal UI and the MCP server are interfaces to it.

pub mod command;
pub mod computed;
pub mod decode;
pub mod protocol;
pub mod query;
pub mod radix;
pub mod rows;
pub mod search;
pub mod session;
pub mod stream;
pub mod time;
pub mod trace;
pub mod view;
pub mod workspace;
