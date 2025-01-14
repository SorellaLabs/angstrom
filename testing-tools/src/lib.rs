#![allow(clippy::too_many_arguments)]
#![feature(associated_type_defaults)]
#![allow(clippy::type_complexity)]

/// External tools that are initialized on-top of the testnet
/// for example a order generator that pushes orders to the nodes rpc
/// and then checks for fills
pub mod agents;
/// mocks utils for different modules
pub mod mocks;
/// Tools for testing network setup
pub mod network;
/// utils for generating orders based on exchange prices.
pub mod order_generator;
/// Tools for testing order_pool functionality
// pub mod order_pool;
/// Tools for generating different types of orders
pub mod type_generator;
/// Tools for validation module. Helps with db overrides and other
/// nuanced needs
pub mod validation;

pub mod providers;

/// Tools for contract deployment and testing
pub mod contracts;

pub mod controllers;
pub mod types;
pub mod utils;
