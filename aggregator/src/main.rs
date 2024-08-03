use std::collections::HashSet;
use std::str::FromStr;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use mongodb::bson::doc;
use mongodb::options::UpdateModifications;
use mongodb::{bson::Document, Client, Collection};
use shared::SolanaAccount;
use solana_client::{rpc_client::RpcClient, rpc_config::RpcBlockConfig};
use solana_program::pubkey::Pubkey;
use solana_transaction_status::{EncodedTransaction, EncodedTransactionWithStatusMeta, UiMessage};
use tokio::task::JoinHandle;
use tokio::time::sleep;

#[derive(Clone)]
struct TransactionsCollection(Collection<Document>);
#[derive(Clone)]
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
    let block_poll_interval_millis: u128 =
        shared::parse_from_env_or_panic("BLOCK_POLL_INTERVAL_MILLIS");

    let rpc_client_box = Box::new(RpcClient::new(&rpc_url));
    let rpc_client: &'static RpcClient = Box::leak(rpc_client_box);

    // NOTE: at this point we panic there is nothing we can do to recover from failing to create a MongoDB client
    let mongo_client = Client::with_uri_str(mongo_url)
        .await
        .expect("Failed creating MongoDB client");
    let mongo_database = mongo_client.database("solforge");
    let transactions_collection = TransactionsCollection(mongo_database.collection("transactions"));
    let accounts_collection = AccountsCollection(mongo_database.collection("accounts"));

    // NOTE: at this point we panic as there is nothing we can do to recover if we fail to fetch the latest finalised slot
    let mut start_slot = rpc_client
        .get_slot()
        .expect("Failed to fetch latest finalised slot");

    loop {
        let start_time = Instant::now();

        // NOTE: we simply panic here as there is nothing we can do to recover from this with reasonable effort
        // Yes, we can implement retries and other fallback mechanisms, but this is beyond the scope of this exercise
        let blocks = rpc_client
            .get_blocks(start_slot, None)
            .expect("Failed to get blocks");

        // NOTE: we are not including the last block in fetching as the last slot is the next
        // start_slot. If we include it as well it would be fetched twice
        let slots = &blocks[..blocks.len() - 1];

        // processing blocks returns the set of all accounts that were involved in the Txs
        let accounts: HashSet<Pubkey> =
            process_blocks(rpc_client, &transactions_collection, slots).await;
        // processing set of all acounts, using tasks. Given that there are no duplicates due
        // to hashset, we can process them all in parallel without any issue of inconsistent data
        process_accounts(rpc_client, &accounts_collection, accounts).await;

        // updating the start slot for the next pull iteration to be the last of the current batch
        // note that the last block is not fetched as it is used as starting point for the next
        // pull iteration after the timeout
        start_slot = *blocks.last().unwrap_or(&start_slot);

        let delta = Instant::now() - start_time;
        if delta.as_millis() > block_poll_interval_millis {
            warn!("Processing blocks and accounts took {} millisecondes which is longer than the block polling interval of {} milliseconds.\
                This indicates that the Solana Network is producing blocks faster than the aggregator can consume them.", 
                    delta.as_millis() , block_poll_interval_millis);
        } else {
            let waiting_time_millis = block_poll_interval_millis - delta.as_millis();
            sleep(Duration::from_millis(waiting_time_millis as u64)).await;
        }
    }
}

async fn process_blocks(
    rpc_client: &'static RpcClient,
    transactions_collection: &TransactionsCollection,
    slots: &[u64],
) -> HashSet<Pubkey> {
    info!("Fetching blocks for slots {:?}", slots);

    let block_hdls: Vec<JoinHandle<Result<Vec<Pubkey>, String>>> = slots
        .iter()
        .map(|block_slot| {
            tokio::spawn({
                // let rpc_client_thread = RpcClient::new(rpc_url);
                let transactions_collection_thread = transactions_collection.clone();
                let block_slot_thread = *block_slot;
                async move {
                    process_block(
                        rpc_client,
                        &transactions_collection_thread,
                        block_slot_thread,
                    )
                    .await
                }
            })
        })
        .collect();

    let mut accounts: HashSet<Pubkey> = HashSet::new();

    // NOTE: we are collecting all accounts into a HashSet so we avoid duplicate work
    // as well as inconsistencies due to concurrent updates
    for hdl in block_hdls {
        // NOTE: at this point we simply panic, in a production environment we problably
        // need to deal with it more gracefully, such as retrying, or other cleanup
        let ret = hdl
            .await
            .expect("Awaiting block processing thread handle failed");

        match ret {
            // NOTE: at this point we simply log errors, without dealing with them individually
            // In a production environment you may want to differentiate between errors
            // and attempt retries e.g. when interacting with RPC, but this beyond
            // the scope of this exercise
            Err(err) => error!("Failed processing block: {}", err),
            Ok(pks) => pks.iter().for_each(|pk| {
                accounts.insert(*pk);
            }),
        }
    }

    accounts
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
    block_slot: u64,
) -> Result<Vec<Pubkey>, String> {
    info!("Processing of block {}...", block_slot);

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
        "Received block with hash {} for slot {} produced at time {}",
        block.blockhash, block_slot, timestamp
    );

    let mut account_pubkeys = Vec::new();

    // iterate over all transactions (there might be none) and encode them to a MongoDB doc and insert into the collection
    if let Some(txs) = block.transactions {
        for tx in txs {
            let account_pubkey_strs = process_transaction(
                &tx,
                block.blockhash.clone(),
                block_slot,
                timestamp,
                transactions_collection,
            )
            .await?;

            for account_pubkey_str in account_pubkey_strs {
                match Pubkey::from_str(&account_pubkey_str) {
                    Ok(pk) => account_pubkeys.push(pk),
                    Err(err) => warn!(
                        "Failed to parse Pubkey for account key {} with error: {}",
                        account_pubkey_str, err
                    ),
                }
            }
        }
    }

    info!("Processing of block {} finished", block_slot);

    Ok(account_pubkeys)
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

/// This processes all accounts in the HashSet, by fetching the account info for each in a
/// separate task. Due to the fact that we are using a HashSet we have the guarantee that
/// no account will be fetched twice (or more), therefore we can parallelise this perfectly
/// without running into consistency issues due to out-of-sequence updates.
/// NOTE: this runs into Helius rate limitations when there are too many accounts: HTTP status client error (429 Too Many Requests
async fn process_accounts(
    rpc_client: &'static RpcClient,
    accounts_collection: &AccountsCollection,
    accounts: HashSet<Pubkey>,
) {
    let accounts_count = accounts.len();
    info!("Updating {} accounts ...", accounts_count);

    let start = Instant::now();

    let account_processing_hdls: Vec<JoinHandle<Result<(), String>>> = accounts
        .into_iter()
        .map(|account_pkh| {
            tokio::spawn({
                let accounts_collection_thread = accounts_collection.clone();
                async move {
                    process_account(rpc_client, &accounts_collection_thread, account_pkh).await
                }
            })
        })
        .collect();

    for hdl in account_processing_hdls {
        // NOTE: at this point we simply panic, in a production environment we problably
        // need to deal with it more gracefully, such as retrying, or other cleanup
        let ret = hdl
            .await
            .expect("Awaiting account processing thread handle failed");

        if let Err(err) = ret {
            // NOTE: at this point we simply log errors, without dealing with them individually
            // In a production environment you may want to differentiate between errors
            // and attempt retries e.g. when interacting with RPC, but this beyond
            // the scope of this exercise
            error!("Failed processing account: {}", err);
        }
    }

    let now = Instant::now();

    info!(
        "Updating {} accounts finished and took {:?} seconds",
        accounts_count,
        (now - start)
    );
}

async fn process_account(
    rpc_client: &RpcClient,
    accounts_collection: &AccountsCollection,
    account_pkh: Pubkey,
) -> Result<(), String> {
    // NOTE: in case we can't fetch the account we emit a warning and ignore updating this account
    match rpc_client.get_account(&account_pkh) {
        Err(err) => {
            warn!(
                "Failed get_account for key {} with error: {}",
                account_pkh, err
            );
            Ok(())
        }
        Ok(account) => {
            debug!("Loaded account for key {}: {:?}", account_pkh, account);

            let account = SolanaAccount {
                _id: account_pkh.to_string(),
                lamports: account.lamports,
                data: account.data,
                owner: account.owner,
                executable: account.executable,
            };

            let update_filter = doc! { "_id": account_pkh.to_string()};
            let account_doc = mongodb::bson::to_document(&account)
                .map_err(|err| format!("Failed serialise account to bscon: {}", err))?;
            let update = doc! { "$set": &account_doc };

            accounts_collection
                .0
                .update_one(update_filter, UpdateModifications::Document(update))
                .upsert(true)
                .await
                .map_err(|err| format!("Failed updating account document: {}", err))?;

            Ok(())
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
