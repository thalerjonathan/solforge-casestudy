use std::{fmt, str::FromStr, sync::Arc};

use axum::{
    extract::{Query, State},
    Json,
};
use log::{error, info};
use mongodb::{
    bson::{doc, Document},
    Collection,
};

use serde::{de, Deserialize, Deserializer};

pub struct ServerState {
    pub transactions_collection: Collection<Document>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct TxQuery {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    id: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    date: Option<String>,
}

// taken from https://github.com/tokio-rs/axum/blob/main/examples/query-params-with-empty-strings/src/main.rs#L37
fn empty_string_as_none<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let opt = Option::<String>::deserialize(de)?;
    match opt.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => FromStr::from_str(s).map_err(de::Error::custom).map(Some),
    }
}

pub async fn transactions(
    State(state): State<Arc<ServerState>>,
    Query(tx_query): Query<TxQuery>, // Query<Map<String, Value>>,
) -> Json<Option<shared::SolanaTransaction>> {
    // "signatures": "3yx6sDyRhJQJ6UM8it2zdMphZWkB2YHpWsPiFJx5BagKomnxEzYey3itZcW6Vsv73drgZdgR36Yer1p4nuiujii8"
    info!("tx_query: {:?}", tx_query);

    let ret = state.transactions_collection.find_one(doc! {}).await;
    match ret {
        Ok(Some(doc)) => {
            let ret: Result<shared::SolanaTransaction, _> = mongodb::bson::from_document(doc);
            info!("Something found: {:?}", ret);
            // FIXME: handle unwrap
            return Json(Some(ret.unwrap()));
        }
        Ok(None) => {
            info!("Nothing found")
        }
        // FIXME: return clean error
        Err(err) => error!("Error {:?}", err),
    }

    Json(None)
}
