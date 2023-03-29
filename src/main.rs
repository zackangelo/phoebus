use anyhow::{anyhow, Result};
use resolver::{ObjectResolver, Resolved};
use std::time::Instant;
use tracing::info;
use tracing_subscriber::EnvFilter;
use value::{ConstValue, Name};

use crate::executor::Executor;

mod executor;
mod introspection;
mod resolver;
mod value;

const SCHEMA: &str = include_str!("../schema.graphql");
const QUERY: &str = include_str!("../introspection.graphql");

#[tokio::main]
async fn main() -> Result<()> {
    // tracing_subscriber::fmt::init();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_thread_ids(true)
        .init();

    info!("phoebus server starting...");

    // tokio::spawn(async move {
    let executor = Executor::new(SCHEMA).unwrap();
    for _i in 0..1000 {
        let start = Instant::now();
        let result = executor.run(QUERY, QueryResolver).await.unwrap();
        let duration_us = Instant::now().duration_since(start).as_micros();
        println!(
            "{} (took {}μs)",
            serde_json::to_string_pretty(&result)?,
            duration_us,
        );
    }
    // });

    Ok(())
}

pub struct QueryResolver;

#[async_trait::async_trait]
impl ObjectResolver for QueryResolver {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        match name {
            "peopleCount" => Ok(ConstValue::Number(42.into()).into()),
            "person" => Ok(PersonResolver.into()),
            _ => Err(anyhow!("invalid field: {}", name)),
        }
    }
}

pub struct PersonResolver;

#[async_trait::async_trait]
impl ObjectResolver for PersonResolver {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        match name {
            "firstName" => Ok(ConstValue::String("Zack".to_owned()).into()),
            "lastName" => Ok(ConstValue::String("Angelo".to_owned()).into()),
            "age" => Ok(ConstValue::Number(39.into()).into()),
            "pets" => {
                let pets: Vec<Resolved> = vec![DogResolver.into(), CatResolver.into()];
                Ok(pets.into())
            }
            _ => Err(anyhow!("invalid field {}", name)),
        }
    }
}

pub struct DogResolver;

#[async_trait::async_trait]
impl ObjectResolver for DogResolver {
    async fn resolve_type_name(&self) -> Result<Option<&str>> {
        Ok(Some("Dog"))
    }

    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        match name {
            "name" => Ok(ConstValue::String("Coco".to_owned()).into()),
            "dogBreed" => Ok(ConstValue::Enum(Name::new("CHIHUAHUA")).into()),
            _ => Err(anyhow!("invalid field {}", name)),
        }
    }
}

pub struct CatResolver;

#[async_trait::async_trait]
impl ObjectResolver for CatResolver {
    async fn resolve_type_name(&self) -> Result<Option<&str>> {
        Ok(Some("Cat"))
    }

    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        match name {
            "name" => Ok(ConstValue::String("Nemo".to_owned()).into()),
            "catBreed" => Ok(ConstValue::Enum(Name::new("TABBY")).into()),
            _ => Err(anyhow!("invalid field {}", name)),
        }
    }
}
