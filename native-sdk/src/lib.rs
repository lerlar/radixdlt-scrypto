#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(any(feature = "std", feature = "alloc")))]
compile_error!("Either feature `std` or `alloc` must be enabled for this crate.");
#[cfg(all(feature = "std", feature = "alloc"))]
compile_error!("Feature `std` and `alloc` can't be enabled at the same time.");

pub mod access_rules;
pub mod component;
pub mod metadata;
pub mod resource;
pub mod runtime;

// Export macros
mod macros;
pub use macros::*;

// Re-export radix engine derives
pub extern crate radix_engine_derive;
pub use radix_engine_derive::{
    LegacyDescribe, ScryptoCategorize, ScryptoDecode, ScryptoDescribe, ScryptoEncode, ScryptoSbor,
};

pub extern crate radix_engine_interface;
