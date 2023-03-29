extern crate phoebus;

mod resolvers;

use std::time::Instant;

use anyhow::Result;
use phoebus::Executor;
use tracing::info;

const SCHEMA: &str = include_str!("schema.graphql");
const QUERY: &str = include_str!("query.graphql");

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("axum http server starting...");

    // tokio::spawn(async move {
    let executor = Executor::new(SCHEMA).unwrap();
    for _i in 0..1 {
        let start = Instant::now();
        let result = executor.run(QUERY, resolvers::QueryResolver).await.unwrap();
        let duration_us = Instant::now().duration_since(start).as_micros();
        println!(
            "{} (took {}μs)",
            serde_json::to_string_pretty(&result)?,
            duration_us,
        );
    }

    Ok(())
}
