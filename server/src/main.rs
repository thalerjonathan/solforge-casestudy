use std::sync::Arc;

use axum::{routing::get, Router};
use handlers::{transactions, ServerState};
use mongodb::Client;

mod handlers;

#[tokio::main]
async fn main() {
    env_logger::init();

    let server_host = get_from_env_or_panic("HOST");
    let mongo_url = get_from_env_or_panic("MONGO_URL");

    // NOTE: at this point we panic there is nothing we can do to recover from failing to create a MongoDB client
    let mongo_client = Client::with_uri_str(mongo_url)
        .await
        .expect("Failed creating MongoDB client");
    let mongo_database = mongo_client.database("solforge");
    let transactions_collection = mongo_database.collection("transactions");

    let state = ServerState {
        transactions_collection,
    };
    let state_arc = Arc::new(state);

    let app = Router::new()
        .route("/transactions", get(transactions))
        // TODO: add /accounts route
        .with_state(state_arc);

    let listener = tokio::net::TcpListener::bind(&server_host)
        .await
        .unwrap_or_else(|err| panic!("Failed to bind to {} with error: {}", server_host, err));

    axum::serve(listener, app)
        .await
        .unwrap_or_else(|err| panic!("Failed serving app with error: {}", err));
}

fn get_from_env_or_panic(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|err| panic!("Cannot find {} in env: {}", key, err))
}
