use anyhow::{anyhow, Result};
use phoebus::{ConstValue, Ctx, Name, ObjectResolver, Resolved};

pub struct QueryResolver;

#[async_trait::async_trait]
impl ObjectResolver for QueryResolver {
    async fn resolve_field(&self, ctx: &Ctx, name: &str) -> Result<Resolved> {
        match name {
            "peopleCount" => Ok(ConstValue::Number(42.into()).into()),
            "person" => Ok(PersonResolver {
                str_arg_value: ctx.arg("testStringArg"),
                int_arg_value: ctx.arg("testIntArg"),
                float_arg_value: ctx.arg("testFloatArg"),
                bool_arg_value: ctx.arg("testBoolArg"),
            }
            .into()),
            _ => Err(anyhow!("invalid field: {}", name)),
        }
    }
}

pub struct PersonResolver {
    str_arg_value: Option<String>,
    int_arg_value: Option<i32>,
    float_arg_value: Option<f64>,
    bool_arg_value: Option<bool>,
}

impl PersonResolver {
    fn av<C: Into<ConstValue> + Clone>(&self, maybe: &Option<C>) -> Result<Resolved> {
        match maybe.clone() {
            Some(c) => Ok(Resolved::Value(c.into())),
            None => Ok(Resolved::null()),
        }
    }
}
#[async_trait::async_trait]
impl ObjectResolver for PersonResolver {
    async fn resolve_field(&self, _: &Ctx, name: &str) -> Result<Resolved> {
        match name {
            "firstName" => Ok(ConstValue::String("Zack".to_owned()).into()),
            "lastName" => Ok(ConstValue::String("Angelo".to_owned()).into()),
            "age" => Ok(ConstValue::Number(39.into()).into()),
            "stringArgVal" => self.av(&self.str_arg_value),
            "intArgVal" => self.av(&self.int_arg_value),
            "floatArgVal" => self.av(&self.float_arg_value),
            "boolArgVal" => self.av(&self.bool_arg_value),
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

    async fn resolve_field(&self, _ctx: &Ctx, name: &str) -> Result<Resolved> {
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

    async fn resolve_field(&self, _ctx: &Ctx, name: &str) -> Result<Resolved> {
        match name {
            "name" => Ok(ConstValue::String("Nemo".to_owned()).into()),
            "catBreed" => Ok(ConstValue::Enum(Name::new("TABBY")).into()),
            _ => Err(anyhow!("invalid field {}", name)),
        }
    }
}
