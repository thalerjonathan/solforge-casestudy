use std::{
    fmt::{self, Display},
    str::FromStr,
    sync::Arc,
};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use futures::TryStreamExt;
use log::error;
use mongodb::{
    bson::{doc, Document},
    Collection,
};

use serde::{de, Deserialize, Deserializer};

/// Represents an application error, where the application failed to handle a response
/// This is used to map such errors to 500 internal server error HTTP codes
#[derive(Debug)]
pub struct AppError {
    error: String,
}

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

// NOTE: need to implement IntoResponse so that axum knows how to return 500 from an AppError
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("Response error: {}", self);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed processing request due to error: {}", self),
        )
            .into_response()
    }
}

impl AppError {
    fn from_error(error: &str) -> Self {
        Self {
            error: error.to_string(),
        }
    }
}

pub struct TransactionsCollection(pub Collection<Document>);
pub struct AccountsCollection(pub Collection<Document>);

/// The server state holdilng the MongoDb collections from which to fetch
pub struct ServerState {
    pub transactions_collection: TransactionsCollection,
    pub accounts_collection: AccountsCollection,
}

/// Representation of the 2 different query params that can be passed to the /transactions endpoint
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

/// /transactions endpoint handler
pub async fn transactions(
    State(state): State<Arc<ServerState>>,
    Query(tx_query): Query<TxQuery>,
) -> Result<Json<Vec<shared::SolanaTransaction>>, AppError> {
    // differentiate between supported query params, constructing different queries
    let doc_query = match (tx_query.id, tx_query.day) {
        // query by Tx id, which in Solana are the signatures
        (Some(id), _) => doc! { "transaction.signatures": id },
        // query by day in format day/month/year
        (None, Some(ref day)) => {
            let from_date =
                NaiveDateTime::parse_from_str(&format!("{} 00:00:00", day), "%d/%m/%Y %H:%M:%S")
                    .map_err(|err| {
                        AppError::from_error(&format!(
                            "Failed parsing day in from date with error: {}",
                            err
                        ))
                    })?;
            let to_date =
                NaiveDateTime::parse_from_str(&format!("{} 23:59:59", day), "%d/%m/%Y %H:%M:%S")
                    .map_err(|err| {
                        AppError::from_error(&format!(
                            "Failed parsing day in to date with error: {}",
                            err
                        ))
                    })?;

            let from_date_str = format!("{}", from_date.format("%Y-%m-%dT%H:%M:%SZ"));
            let to_date_str = format!("{}", to_date.format("%Y-%m-%dT%H:%M:%SZ"));

            doc! { "timestamp" : {"$gte": from_date_str, "$lt": to_date_str}}
        }
        // return ALL Txs - might return a potentially very large set. Probably best to implement
        // some form of paging, but this is beyond the scope of this exercise
        (None, None) => doc! {},
    };

    let ret = state.transactions_collection.0.find(doc_query).await;
    match ret {
        Ok(cursor) => {
            let docs: Vec<Document> = cursor.try_collect().await.map_err(|err| {
                AppError::from_error(&format!(
                    "Failed fetching all transaction documents with error: {}",
                    err
                ))
            })?;

            let txs_res: Result<Vec<shared::SolanaTransaction>, _> =
                docs.into_iter().map(mongodb::bson::from_document).collect();
            let txs = txs_res.map_err(|err| {
                AppError::from_error(&format!(
                    "Failed deserialising transaction bson to json with error: {}",
                    err
                ))
            })?;

            Ok(Json(txs))
        }

        Err(err) => {
            let err_msg = format!(
                "Find query in transaction collection failed with error: {}",
                err
            );
            error!("{}", err_msg);
            Err(AppError::from_error(&err_msg))?
        }
    }
}

/// /account/:id endpoint handler
pub async fn account(
    State(state): State<Arc<ServerState>>,
    Path(account_id): Path<String>,
) -> Result<Json<Option<shared::SolanaAccount>>, AppError> {
    let doc_query = doc! { "_id": account_id };

    let ret = state
        .accounts_collection
        .0
        .find_one(doc_query)
        .await
        .map_err(|err| {
            AppError::from_error(&format!(
                "Find query in accounts collection failed with error: {}",
                err
            ))
        })?;

    match ret {
        Some(account_doc) => {
            let account_res: Result<shared::SolanaAccount, _> =
                mongodb::bson::from_document(account_doc);
            let account = account_res.map_err(|err| {
                AppError::from_error(&format!(
                    "Failed deserialising account bson to json with error: {}",
                    err
                ))
            })?;

            Ok(Json(Some(account)))
        }

        None => Ok(Json(None)),
    }
}
