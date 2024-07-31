use solana_client::{pubsub_client::PubsubClient, rpc_config::RpcBlockSubscribeFilter};

fn main() {
    // TODO: create application config
    let url = get_from_env_or_panic("RPC_URL");

    // see https://docs.rs/solana-pubsub-client/2.0.3/solana_pubsub_client/pubsub_client/index.html
    // TODO: cleanup unwrap
    // TODO: https://docs.rs/solana-rpc-client-api/2.0.3/solana_rpc_client_api/config/struct.RpcBlockSubscribeConfig.html
    let (_, receiver) =
        PubsubClient::block_subscribe(&url, RpcBlockSubscribeFilter::All, None).unwrap();

    loop {
        match receiver.recv() {
            Ok(response) => {
                println!("Block subscription response: {:?}", response);
            }
            Err(e) => {
                println!("Block subscription error: {:?}", e);
                break;
            }
        }
    }
}

pub fn get_from_env_or_panic(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|err| panic!("Cannot find {} in env: {}", key, err))
}
