extern crate phoebus;

mod graphiql;
mod resolvers;

use anyhow::Result;
use graphiql::GraphiQLSource;
use phoebus::Executor;
use tracing::info;

use axum::{
    extract::Extension,
    http::StatusCode,
    response::{self, IntoResponse},
    routing::{get, post},
    Json, Router, Server,
};

const SCHEMA: &str = include_str!("schema.graphql");
// const QUERY: &str = include_str!("query.graphql");

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("axum http server starting...");
    let executor = Executor::new(SCHEMA)?;
    let app = Router::new()
        .route("/", get(graphiql) /*.post(graphql_handler)*/)
        .route("/graphql", post(graphql))
        .layer(Extension(executor));

    println!("GraphiQL IDE: http://localhost:8000");

    Server::bind(&"127.0.0.1:8000".parse().unwrap())
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn graphiql() -> impl IntoResponse {
    response::Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

async fn graphql(
    executor: Extension<Executor>,
    Json(graphql_req): Json<http::GraphQLReq>,
) -> (StatusCode, Json<http::GraphQLResp>) {
    match executor
        .run(
            &graphql_req.query,
            resolvers::QueryResolver,
            graphql_req.operation_name,
        )
        .await
        .and_then(|r| r.into_json().map_err(anyhow::Error::new))
    {
        Ok(result) => (
            StatusCode::OK,
            Json(http::GraphQLResp {
                data: result,
                errors: None,
            }),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(http::GraphQLResp::generic_error(err)),
        ),
    }
}

mod http {
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::{collections::HashMap, fmt::Display};

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GraphQLReq {
        pub query: String,
        pub operation_name: Option<String>,
        pub variables: Option<HashMap<String, serde_json::Value>>,
    }

    #[derive(Serialize, Debug, Clone)]
    #[serde(rename_all = "camelCase")]
    pub struct GraphQLResp {
        pub data: serde_json::Value,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub errors: Option<Vec<serde_json::Value>>,
    }

    impl GraphQLResp {
        pub fn generic_error<E: Display>(err: E) -> Self {
            Self {
                data: Default::default(),
                errors: Some(
                    json!([{ "message": format!("{}", err) }])
                        .as_array()
                        .unwrap()
                        .clone(), //TODO fix
                ),
            }
        }
    }
}
