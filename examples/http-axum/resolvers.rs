use anyhow::{anyhow, Result};
use phoebus::{ConstValue, Name, ObjectResolver, Resolved};

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
