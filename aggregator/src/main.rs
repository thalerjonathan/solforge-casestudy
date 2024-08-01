use std::borrow::BorrowMut;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use log::{error, info, warn};
use mongodb::bson::doc;
use mongodb::options::UpdateModifications;
use mongodb::{bson::Document, Client, Collection};
use shared::SolanaAccount;
use solana_client::{rpc_client::RpcClient, rpc_config::RpcBlockConfig};
use solana_program::pubkey::Pubkey;
use solana_transaction_status::{EncodedTransaction, EncodedTransactionWithStatusMeta, UiMessage};
use tokio::sync::Mutex;
use tokio::time::sleep;

struct TransactionsCollection(Collection<Document>);
struct AccountsCollection(Collection<Document>);

#[tokio::main]
async fn main() {
    // let url = get_from_env_or_panic("RPC_URL");
    // NOTE: unfortunately it seems that helius nodes do not expose the block subscription method, so this wont work (will fail with "Method not found")
    // see https://docs.rs/solana-pubsub-client/2.0.3/solana_pubsub_client/pubsub_client/index.html
    // let (_, receiver) =
    //     PubsubClient::block_subscribe(&url, RpcBlockSubscribeFilter::All, None).unwrap();
    // loop {
    //     match receiver.recv() {
    //         Ok(response) => {
    //             println!("Block subscription response: {:?}", response);
    //         }
    //         Err(e) => {
    //             println!("Block subscription error: {:?}", e);
    //             break;
    //         }
    //     }
    // }

    // NOTE: this didnt work either
    // let api_key = get_from_env_or_panic("HELIUS_API_KEY");
    // // TODO: proper error handling
    // let helius = Helius::new_with_ws(&api_key, Cluster::Devnet)
    //     .await
    //     .expect("Failed to create a Helius client");
    // let ret = helius.connection().get_block(slot);
    // let filter = TransactionSubscribeFilter {
    //     vote: None,
    //     failed: None,
    //     signature: None,
    //     account_include: None,
    //     account_exclude: None,
    //     account_required: None,
    // };
    // let config: RpcTransactionsConfig = RpcTransactionsConfig {
    //     filter,
    //     options: TransactionSubscribeOptions::default(),
    // };
    // info!("helius.ws");
    // if let Some(ws) = helius.ws() {
    //     // TODO: clean error handling
    //     info!("transaction_subscribe");
    //     let (mut stream, _unsub) = ws.transaction_subscribe(config).await.unwrap();
    //     while let Some(event) = stream.next().await {
    //         println!("{:#?}", event);
    //     }
    // }

    // NOTE: this didnt work either because we simply cannot query blocks by just processed slots as they are not final yet
    // see https://docs.rs/solana-pubsub-client/2.0.3/solana_pubsub_client/pubsub_client/index.html
    // https://docs.rs/solana-client/latest/solana_client/pubsub_client/struct.PubsubClient.html
    // NOTE: seems we need to use async version as "blocking" version fails upon receiving
    // in nondeterministic way: sometimes it returns a valid response just to fail with a RecvError
    // and other times it fails straight with RecvError
    // let wss_url = get_from_env_or_panic("WSS_URL");
    // let pubsub_client = PubsubClient::new(&wss_url).await.unwrap();
    // let (mut stream, _closefn) = pubsub_client.slot_subscribe().await.unwrap();
    // loop {
    //     match stream.next().await {
    //         Some(response) => {
    //             info!("Slot subscription response: {:?}", response);
    //             sleep(Duration::from_millis(10000)).await;
    //             // NOTE: unfortunately this doesnt work as the block for the corresponding slots are not finalised yet
    //             let ret = client.get_block(response.slot);
    //             info!("Received block for slot {:?}: {:?}", response.slot, ret);
    //         }
    //         None => {
    //             error!("Finished consuming slots");
    //             break;
    //         }
    //     }
    // }

    env_logger::init();

    let mongo_url = shared::get_from_env_or_panic("MONGO_URL");
    let rpc_url = shared::get_from_env_or_panic("RPC_URL");
    let block_workers: u16 = shared::parse_from_env_or_panic("BLOCK_WORKERS_COUNT");
    let block_poll_interval_millis: u64 =
        shared::parse_from_env_or_panic("BLOCK_POLL_INTERVAL_MILLIS");

    let rpc_client = RpcClient::new(&rpc_url);

    // NOTE: at this point we panic there is nothing we can do to recover from failing to create a MongoDB client
    let mongo_client = Client::with_uri_str(mongo_url)
        .await
        .expect("Failed creating MongoDB client");
    let mongo_database = mongo_client.database("solforge");
    let transactions_collection: Collection<Document> = mongo_database.collection("transactions");
    let accounts_collection: Collection<Document> = mongo_database.collection("accounts");

    let blocks_queue_arc: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

    // NOTE: at this point we panic as there is nothing we can do to recover if we fail to fetch the latest finalised slot
    let mut start_slot = rpc_client
        .get_slot()
        .expect("Failed to fetch latest finalised slot");

    // spawning block workers, that all synchronise on the blocks_queue_arc Mutex. This is a
    // single producer (main thread), multi consumer (block workers) pattern, to be able to
    // process blocks as fast as the network is producing them.
    for i in 0..block_workers {
        tokio::spawn({
            let rpc_client_thread = RpcClient::new(&rpc_url);

            let blocks_queue_thread = blocks_queue_arc.clone();
            let transactions_collection_thread =
                TransactionsCollection(transactions_collection.clone());
            let accounts_collection_thread = AccountsCollection(accounts_collection.clone());

            async move {
                loop {
                    // fetching the next block slot to process - doing within a separate
                    // block to avoid holding the mutex for too long
                    let block_slot_opt = {
                        let mut guard: tokio::sync::MutexGuard<Vec<u64>> =
                            blocks_queue_thread.lock().await;
                        let blocks: &mut Vec<u64> = (*guard).borrow_mut();
                        if !blocks.is_empty() {
                            let block_slot: u64 = blocks.remove(0);
                            Some(block_slot)
                        } else {
                            None
                        }
                    };

                    // if there is some block to be processed in the queue, process it now
                    if let Some(block_slot) = block_slot_opt {
                        let ret = process_block(
                            &rpc_client_thread,
                            &transactions_collection_thread,
                            &accounts_collection_thread,
                            block_slot,
                            i,
                        )
                        .await;
                        // NOTE: at this point we simply log errors, without dealing with them individually
                        // In a production environment you may want to differentiate between errors
                        // and attempt retries e.g. when interacting with RPC, but this beyond
                        // the scope of this exercise
                        if let Err(err) = ret {
                            error!("Failed processing block: {}", err);
                        }
                    }
                }
            }
        });
    }

    // TODO: implement account info fetching (probably via callbacks?)

    loop {
        // NOTE: we simply panic here as there is nothing we can do to recover from this with reasonable effort
        // Yes, we can implement retries and other fallback mechanisms, but this is beyond the scope of this exercise
        let blocks = rpc_client
            .get_blocks(start_slot, None)
            .expect("Failed to get blocks");

        // NOTE: we are not including the last block in fetching as the last slot is the next
        // start_slot. If we include it as well it would be fetched twice
        let slots = &blocks[..blocks.len() - 1];
        info!("Fetching blocks for slots {:?}", slots);

        // NOTE: separate block to make guard get out of scope to unlock Mutex as quickly as possible
        {
            let mut blocks_queue_guard: tokio::sync::MutexGuard<Vec<u64>> =
                blocks_queue_arc.lock().await;
            let blocks_queue: &mut Vec<u64> = (*blocks_queue_guard).borrow_mut();

            for s in slots {
                blocks_queue.push(*s);
            }
        }

        // updating the start slot for the next pull iteration to be the last of the current batch
        // note that the last block is not fetched as it is used as starting point for the next
        // pull iteration after the timeout
        start_slot = *blocks.last().unwrap_or(&start_slot);

        sleep(Duration::from_millis(block_poll_interval_millis)).await;
    }
}

/// Processes a block given its slot number. It fetches the corresponding block as well as its
/// block time, serialises all transactions to bson and writes them into the MongoDb collection
/// Due to the RPC calls taking a few milliseconds this function is intended to be run in
/// parallel where different blocks are distributed to different threads via a single producer
/// multi consumer mechanism. The ordering of blocks in the collection is irrelevant therefore
/// this works.
async fn process_block(
    rpc_client: &RpcClient,
    transactions_collection: &TransactionsCollection,
    accounts_collection: &AccountsCollection,
    block_slot: u64,
    thread_idx: u16,
) -> Result<(), String> {
    info!(
        "Processing of block {} in thread {}...",
        block_slot, thread_idx
    );

    // NOTE: to handle "Transaction version (0) is not supported by the requesting client. Please try the request again with the following configuration parameter: \"maxSupportedTransactionVersion\": 0""
    // we need to use get_block_with_config with the corresponding config, see https://www.quicknode.com/guides/solana-development/transactions/how-to-update-your-solana-client-to-handle-versioned-transactions
    let block_cfg = RpcBlockConfig {
        max_supported_transaction_version: Some(0),
        ..RpcBlockConfig::default()
    };

    // https://solana.com/docs/rpc/http/getblock
    let block = rpc_client
        .get_block_with_config(block_slot, block_cfg)
        .map_err(|err| format!("Failed get_block_with_config with error {}", err))?;

    let timestamp = block
        .block_time
        .and_then(|ts| DateTime::from_timestamp(ts, 0))
        .unwrap_or(chrono::offset::Utc::now());

    info!(
        "Thread {} received block with hash {} for slot {} produced at time {}",
        thread_idx, block.blockhash, block_slot, timestamp
    );

    // iterate over all transactions (there might be none) and encode them to a MongoDB doc and insert into the collection
    if let Some(txs) = block.transactions {
        for tx in txs {
            let account_key_strs = process_transaction(
                &tx,
                block.blockhash.clone(),
                block_slot,
                timestamp,
                transactions_collection,
            )
            .await?;

            for acc in account_key_strs {
                process_account(rpc_client, timestamp, accounts_collection, &acc).await?;
            }
        }
    }

    info!(
        "Processing of block {} finished in thread {}",
        block_slot, thread_idx
    );

    Ok(())
}

async fn process_transaction(
    tx_encoded: &EncodedTransactionWithStatusMeta,
    block_hash: String,
    block_slot: u64,
    timestamp: DateTime<Utc>,
    transactions_collection: &TransactionsCollection,
) -> Result<Vec<String>, String> {
    // NOTE: for each account involved in the Tx, we fetch it because PubSub to account only works when one wants to listen to a specific account
    let account_keys_str = extract_accounts_from_tx(&tx_encoded.transaction);

    let tx = shared::SolanaTransaction {
        timestamp,
        block_hash,
        block_slot,
        transaction: tx_encoded.transaction.clone(),
        meta: tx_encoded.meta.clone(),
    };

    let tx_doc = mongodb::bson::to_document(&tx)
        .map_err(|err| format!("Failed serialise transaction to bscon: {}", err))?;
    transactions_collection
        .0
        .insert_one(tx_doc)
        .await
        .map_err(|err| format!("Failed inserting transaction document: {}", err))?;

    Ok(account_keys_str)
}

async fn process_account(
    rpc_client: &RpcClient,
    block_timestamp: DateTime<Utc>,
    accounts_collection: &AccountsCollection,
    account_pubkey_str: &str,
) -> Result<(), String> {
    // NOTE: in case we can't parse the pub key we emit a warning and ignore updating this account
    match Pubkey::from_str(account_pubkey_str) {
        Err(err) => {
            warn!(
                "Failed to parse Pubkey for account key {} with error: {}",
                account_pubkey_str, err
            );
            Ok(())
        }
        Ok(pubkey) => {
            // NOTE: in case we can't fetch the account we emit a warning and ignore updating this account
            match rpc_client.get_account(&pubkey) {
                Err(err) => {
                    warn!("Failed get_account for key {} with error: {}", pubkey, err);
                    Ok(())
                }
                Ok(account) => {
                    info!("Loaded account for key {}: {:?}", pubkey, account);

                    let account = SolanaAccount {
                        _id: account_pubkey_str.to_string(),
                        lastchanged: block_timestamp,
                        lamports: account.lamports,
                        data: account.data,
                        owner: account.owner,
                        executable: account.executable,
                    };

                    // NOTE: we update (override) the account only if this account is newer than the one
                    // already stored, which is needed because we are processing blocks concurrently
                    let block_timestamp_str =
                        format!("{}", block_timestamp.format("%Y-%m-%dT%H:%M:%SZ"));
                    let query = doc! { "lastchanged" : {"$gt": block_timestamp_str}};

                    let account_doc = mongodb::bson::to_document(&account)
                        .map_err(|err| format!("Failed serialise account to bscon: {}", err))?;
                    let update = doc! { "$set": account_doc.clone() };

                    // TODO: get update working

                    accounts_collection
                        .0
                        .insert_one(account_doc)
                        //.replace_one(query, account_doc)
                        // .update_one(query, UpdateModifications::Document(update))
                        .await
                        .map_err(|err| format!("Failed inserting account document: {}", err))?;

                    Ok(())
                }
            }
        }
    }
}

fn extract_accounts_from_tx(tx: &EncodedTransaction) -> Vec<String> {
    match tx {
        EncodedTransaction::Json(ui_tx) => match ui_tx.message {
            UiMessage::Parsed(ref ui_parsed_msg) => ui_parsed_msg
                .account_keys
                .iter()
                .map(|a| a.pubkey.clone())
                .collect(),
            UiMessage::Raw(ref ui_raw_msg) => ui_raw_msg.account_keys.clone(),
        },
        EncodedTransaction::Accounts(ui_accounts_list) => ui_accounts_list
            .account_keys
            .iter()
            .map(|a| a.pubkey.clone())
            .collect(),
        _ => Vec::new(),
    }
}
