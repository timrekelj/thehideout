#![allow(refining_impl_trait_internal, refining_impl_trait_reachable)]

pub mod access;
pub mod accounts;
pub mod admin;
pub mod app;
pub mod auth;
pub mod clientip;
pub mod codes;
pub mod config;
pub mod convert;
pub mod email;
pub mod embeds;
pub mod events;
pub mod layout;
pub mod media;
pub mod migrations;
pub mod passkeys;
pub mod permissions;
pub mod ratelimit;
pub mod scylla_conn;
pub mod server;
pub mod services;
pub mod shutdown;
pub mod snowflake;
pub mod stats;
pub mod storage;
pub mod streams;
pub mod ttl;
pub mod validation;
pub mod versions;
pub mod voice;

pub mod proto {
    connectrpc::include_generated!();
}

pub use app::{AppState, build_app};
