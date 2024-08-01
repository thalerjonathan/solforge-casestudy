use std::{fmt::Display, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use solana_transaction_status::{EncodedTransaction, UiTransactionStatusMeta};

/// Represents a transaction on Solana. Due to the fact that the transaction type fetched via get_block
/// from solana-client does not hold the timestamp, nor block info, it is added here
#[derive(Serialize, Deserialize, Debug)]
pub struct SolanaTransaction {
    /// The timestamp of the block this transaction belongs to
    pub timestamp: DateTime<Utc>,
    /// The block hash of the block this transaction belongs to
    pub block_hash: String,
    /// The slot of the block this transaction belongs to
    pub block_slot: u64,
    /// The actual transaction
    pub transaction: EncodedTransaction,
    /// The metadata of the transaction
    pub meta: Option<UiTransactionStatusMeta>,
}

/// Gets a value from the environment and if not found panics.
/// The reason it panics is that fetching from environment is generally done at program
/// startup time, and there is often no way the program can continue when the env variable is
/// missing. In case a non-panicking version is needed, its trivial to add it.
pub fn get_from_env_or_panic(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|err| panic!("Cannot find {} in env: {}", key, err))
}

/// Parses a value from the environment into some type that can be parsed witih FromStr
pub fn parse_from_env_or_panic<F>(key: &str) -> F
where
    F: FromStr,
    <F as FromStr>::Err: Display,
{
    std::env::var(key)
        .unwrap_or_else(|err| panic!("Cannot find {} in env: {}", key, err))
        .parse()
        .unwrap_or_else(|err| panic!("Cannot parse from {}: {}", key, err))
}
