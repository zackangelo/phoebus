use anyhow::{anyhow, Result};
use resolver::ObjectResolver;
use std::time::Instant;
use tracing::info;
use value::ConstValue;

use crate::executor::Executor;

mod executor;
mod resolver;
mod value;

const SCHEMA: &str = include_str!("../schema.graphql");
const QUERY: &str = include_str!("../query.graphql");

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    info!("phoebus server starting...");

    let mut executor = Executor::new(SCHEMA)?;

    {
        let query_type = executor.query_type().unwrap();
        let person_type = executor
            .object_type_def("Person")
            .ok_or(anyhow!("person type not found"))?;
        let dog_type = executor
            .object_type_def("Dog")
            .ok_or(anyhow!("dog type not found"))?;
        let cat_type = executor
            .object_type_def("Cat")
            .ok_or(anyhow!("cat type not found"))?;

        let resolvers = executor.resolvers_mut();
        resolvers.register_obj(query_type, QueryResolver);
        resolvers.register_obj(person_type, PersonResolver);
        resolvers.register_obj(dog_type, DogResolver);
        resolvers.register_obj(cat_type, CatResolver);
    }

    let start = Instant::now();
    let result = executor.run(QUERY).await?;

    println!(
        "result = {}\n(took {}μs)",
        result,
        Instant::now().duration_since(start).as_micros()
    );

    Ok(())
}

pub struct QueryResolver;

#[async_trait::async_trait]
impl ObjectResolver for QueryResolver {
    async fn resolve_field_type(&self, _name: &str) -> Result<String> {
        unimplemented!()
    }

    async fn resolve_field(&self, name: &str) -> Result<ConstValue> {
        match name {
            "peopleCount" => Ok(ConstValue::Number(42.into())),
            _ => Err(anyhow!("invalid field: {}", name)),
        }
    }
}

pub struct PersonResolver;

#[async_trait::async_trait]
impl ObjectResolver for PersonResolver {
    async fn resolve_field_type(&self, name: &str) -> Result<String> {
        match name {
            "pet" => Ok("Dog".to_owned()),
            _ => Err(anyhow!("invalid field")),
        }
    }

    async fn resolve_field(&self, name: &str) -> Result<ConstValue> {
        match name {
            "firstName" => Ok(ConstValue::String("Zack".to_owned())),
            "lastName" => Ok(ConstValue::String("Angelo".to_owned())),
            "age" => Ok(ConstValue::Number(39.into())),
            _ => unreachable!(),
        }
    }
}

pub struct DogResolver;

#[async_trait::async_trait]
impl ObjectResolver for DogResolver {
    async fn resolve_field_type(&self, _name: &str) -> Result<String> {
        unimplemented!()
    }

    async fn resolve_field(&self, name: &str) -> Result<ConstValue> {
        match name {
            "name" => Ok(ConstValue::String("Coco".to_owned())),
            _ => unimplemented!(),
        }
    }
}

pub struct CatResolver;

#[async_trait::async_trait]
impl ObjectResolver for CatResolver {
    async fn resolve_field_type(&self, _name: &str) -> Result<String> {
        unimplemented!()
    }

    async fn resolve_field(&self, name: &str) -> Result<ConstValue> {
        match name {
            "name" => Ok(ConstValue::String("Nemo".to_owned())),
            _ => unimplemented!(),
        }
    }
}
