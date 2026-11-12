//! The integration tests of `arkiv-db-trie`, one module per area, in one
//! binary so that `support` is compiled once.

#[path = "../support/mod.rs"]
mod support;

mod adversarial_bounds;
mod bounded_stage;
mod encoding;
mod explore;
mod interval_inspection;
mod interval_model;
mod interval_security;
mod map;
mod proofs;
mod sorted;
mod store;
mod variable_keys;
