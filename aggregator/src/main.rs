use std::time::Duration;

use log::info;
use solana_client::{rpc_client::RpcClient, rpc_config::RpcBlockConfig};
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    env_logger::init();

    // TODO: create application config
    let rpc_url = get_from_env_or_panic("RPC_URL");

    let client = RpcClient::new(&rpc_url);

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

    let mut start_slot = client.get_slot().unwrap();

    loop {
        // TODO: make configurable
        sleep(Duration::from_millis(1000)).await;

        // FIXME: unwrap
        // let end_slot = client.get_slot().unwrap();
        // FIXME: unwrap
        let blocks = client.get_blocks(start_slot, None).unwrap();

        // NOTE: we are not including the last block in fetching as the last slot is the next
        // start_slot. If we include it as well it would be fetched twice
        fetch_blocks_for_slots(&client, &blocks[..blocks.len() - 1]);

        start_slot = *blocks.last().unwrap_or(&start_slot);
    }
}

fn fetch_blocks_for_slots(client: &RpcClient, slots: &[u64]) {
    info!("fetching blocks for slots {:?}", slots);

    // NOTE: to handle "Transaction version (0) is not supported by the requesting client. Please try the request again with the following configuration parameter: \"maxSupportedTransactionVersion\": 0""
    // we need to use get_block_with_config with the corresponding config, see https://www.quicknode.com/guides/solana-development/transactions/how-to-update-your-solana-client-to-handle-versioned-transactions
    let block_cfg = RpcBlockConfig {
        max_supported_transaction_version: Some(0),
        ..RpcBlockConfig::default()
    };

    for s in slots {
        // https://solana.com/docs/rpc/http/getblock
        // FIXME: handle unwrap
        let block = client.get_block_with_config(*s, block_cfg).unwrap();
        // FIXME: handle unwrap
        let block_json = serde_json::to_string(&block).unwrap();

        info!(
            "Received block with hash for slot {:?}: {:?} resulting JSON: {:?}",
            s, block.blockhash, block_json
        );
    }
}

fn get_from_env_or_panic(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|err| panic!("Cannot find {} in env: {}", key, err))
}
