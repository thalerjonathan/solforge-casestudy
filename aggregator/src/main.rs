use std::borrow::BorrowMut;
use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use log::info;
use mongodb::{bson::Document, Client, Collection};
use solana_client::{rpc_client::RpcClient, rpc_config::RpcBlockConfig};
use tokio::sync::Mutex;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    // TODO: create application config
    // let url = get_from_env_or_panic("RPC_URL");
    // NOTE: unfortunately it seems that helius nodes do not expose the block subscription method, so this wont work (will fail with "Method not found")
    // see https://docs.rs/solana-pubsub-client/2.0.3/solana_pubsub_client/pubsub_client/index.html
    // TODO: cleanup unwrap
    // TODO: https://docs.rs/solana-rpc-client-api/2.0.3/solana_rpc_client_api/config/struct.RpcBlockSubscribeConfig.html
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
    // TODO: cleanup unwrap
    // TODO: https://docs.rs/solana-client/latest/solana_client/pubsub_client/struct.PubsubClient.html
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
    // FIXME: handle unwrap
    let client = Client::with_uri_str(mongo_url).await.unwrap();
    let database = client.database("solforge");
    let transactions: Collection<Document> = database.collection("transactions");

    let rpc_url = shared::get_from_env_or_panic("RPC_URL");
    let rpc_client = RpcClient::new(&rpc_url);

    let blocks_queue_arc: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

    // TODO: make configurable
    for i in 0..5 {
        tokio::spawn({
            let blocks_queue_thread = blocks_queue_arc.clone();
            let transactions_thread = transactions.clone();
            let rpc_client_thread = RpcClient::new(&rpc_url);

            async move {
                loop {
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

                    if let Some(block_slot) = block_slot_opt {
                        process_block(&rpc_client_thread, &transactions_thread, block_slot, i)
                            .await;
                    }
                }
            }
        });
    }

    let mut start_slot = rpc_client.get_slot().unwrap();

    loop {
        // TODO: make configurable
        sleep(Duration::from_millis(1000)).await;

        // FIXME: unwrap
        // let end_slot = client.get_slot().unwrap();
        // FIXME: unwrap
        let blocks = rpc_client.get_blocks(start_slot, None).unwrap();

        // NOTE: we are not including the last block in fetching as the last slot is the next
        // start_slot. If we include it as well it would be fetched twice
        let slots = &blocks[..blocks.len() - 1];
        info!("Fetching blocks for slots {:?}", slots);

        {
            let mut blocks_queue_guard: tokio::sync::MutexGuard<Vec<u64>> =
                blocks_queue_arc.lock().await;
            let blocks_queue: &mut Vec<u64> = (*blocks_queue_guard).borrow_mut();

            for s in slots {
                blocks_queue.push(*s);
            }
        }
        start_slot = *blocks.last().unwrap_or(&start_slot);
    }
}

async fn process_block(
    client: &RpcClient,
    coll: &Collection<Document>,
    block_slot: u64,
    thread_idx: u64,
) {
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
    // FIXME: handle unwrap
    let block = client.get_block_with_config(block_slot, block_cfg).unwrap();

    // TODO: maybe simply take DateTime::now instead of querying due to delay
    // FIXME: handle unwrap
    let ts: i64 = client.get_block_time(block_slot).unwrap();
    // FIXME: handle unwrap
    let timestamp = DateTime::from_timestamp(ts, 0).unwrap();

    info!(
        "Thread {} received block with hash {} for slot {} produced at time {}",
        thread_idx, block.blockhash, block_slot, timestamp
    );

    if let Some(txs) = block.transactions {
        for tx_encoded in txs {
            let tx = shared::SolanaTransaction {
                timestamp,
                block_hash: block.blockhash.clone(),
                block_slot,
                transaction: tx_encoded.transaction,
                meta: tx_encoded.meta,
            };

            // FIXME: handle unwrap
            let tx_doc = mongodb::bson::to_document(&tx).unwrap();
            // FIXME: handle unwrap
            coll.insert_one(tx_doc).await.unwrap();
        }
    }

    info!(
        "Processing of block {} finished in thread {}",
        block_slot, thread_idx
    );
}
