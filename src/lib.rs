//Zei: Confidential Payments for Accounts
//#![feature(try_from)]
#![feature(try_trait)]
//#![feature(custom_attribute)]

extern crate blake2;
extern crate bulletproofs;
extern crate curve25519_dalek;
extern crate ed25519_dalek;
extern crate merlin;
extern crate num_bigint;
extern crate num_traits;
extern crate rand;
extern crate serde;
#[macro_use] extern crate serde_derive;
extern crate serde_json;
extern crate sodiumoxide;

mod constants;
mod encryption;
mod errors;
mod setup;
mod utils;


pub mod account;
pub mod address;
pub mod keys;
pub mod proofs;
pub mod serialization;
pub mod utxo_transaction;

// TODO(jackson): Real C bindings for zei
use self::account::Account;
use rand::rngs::OsRng;

#[no_mangle]
pub extern fn test_function() -> f32 {
    let mut csprng: OsRng = OsRng::new().unwrap();
    Account::new(&mut csprng);
    42.0
}

