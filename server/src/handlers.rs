use std::{fmt, str::FromStr, sync::Arc};

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::NaiveDateTime;
use log::error;
use mongodb::{
    bson::{doc, Document},
    Collection,
};

use futures::stream::TryStreamExt;
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
    day: Option<String>,
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
    Query(tx_query): Query<TxQuery>,
) -> Json<Vec<shared::SolanaTransaction>> {
    let doc_query = match (tx_query.id, tx_query.day) {
        (Some(id), _) => doc! { "transaction.signatures": id },
        (None, Some(ref day)) => {
            // FIXME: handle unwrap
            let from_date =
                NaiveDateTime::parse_from_str(&format!("{} 00:00:00", day), "%d/%m/%Y %H:%M:%S")
                    .unwrap();
            // FIXME: handle unwrap
            let to_date =
                NaiveDateTime::parse_from_str(&format!("{} 23:59:59", day), "%d/%m/%Y %H:%M:%S")
                    .unwrap();

            let from_date_str = format!("{}", from_date.format("%Y-%m-%dT%H:%M:%SZ"));
            let to_date_str = format!("{}", to_date.format("%Y-%m-%dT%H:%M:%SZ"));

            doc! { "timestamp" : {"$gte": from_date_str, "$lt": to_date_str}}
        }
        (None, None) => doc! {},
    };

    let ret = state.transactions_collection.find(doc_query).await;
    match ret {
        Ok(cursor) => {
            // FIXME: handle unwrap
            let docs: Vec<Document> = cursor.try_collect().await.unwrap();

            let txs: Result<Vec<shared::SolanaTransaction>, _> =
                docs.into_iter().map(mongodb::bson::from_document).collect();

            // FIXME: handle unwrap
            Json(txs.unwrap())
        }

        // FIXME: return clean error
        Err(err) => {
            error!("Error {:?}", err);
            // TODO: 500 server error
            Json(Vec::new())
        }
    }
}
