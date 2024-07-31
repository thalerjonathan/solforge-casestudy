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
    // FIXME: handle unwrap
    let mongo_client = Client::with_uri_str(mongo_url).await.unwrap();
    let database = mongo_client.database("solforge");
    let transactions_collection = database.collection("transactions");

    let state = ServerState {
        transactions_collection,
    };
    let state_arc = Arc::new(state);

    let app = Router::new()
        .route("/transactions", get(transactions))
        .with_state(state_arc);

    let listener = tokio::net::TcpListener::bind(&server_host).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

fn get_from_env_or_panic(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|err| panic!("Cannot find {} in env: {}", key, err))
}
