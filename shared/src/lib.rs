use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use solana_transaction_status::{EncodedTransaction, UiTransactionStatusMeta};

#[derive(Serialize, Deserialize, Debug)]
pub struct SolanaTransaction {
    pub timestamp: DateTime<Utc>,
    pub block_hash: String,
    pub block_slot: u64,
    pub transaction: EncodedTransaction,
    pub meta: Option<UiTransactionStatusMeta>,
}

pub fn get_from_env_or_panic(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|err| panic!("Cannot find {} in env: {}", key, err))
}
