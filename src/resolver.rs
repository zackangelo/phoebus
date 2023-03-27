use crate::value::ConstValue;
use anyhow::Result;

#[async_trait::async_trait]
pub trait ObjectResolver: Send + Sync {
    /// Resolves the concrete type of this if it's a polymorphic type
    async fn resolve_type_name(&self) -> Result<Option<&str>> {
        Ok(None)
    }

    /// Resolves the value of the specified field
    async fn resolve_field(&self, name: &str) -> Result<Resolved>;
}

// use futures::stream::Stream;

pub enum Resolved {
    Value(ConstValue),
    Object(Box<dyn ObjectResolver>),
    Array(Vec<Resolved>),
}

impl Resolved {
    fn null() -> Self {
        Self::Value(ConstValue::Null)
    }

    fn is_null(&self) -> bool {
        match self {
            Self::Value(ConstValue::Null) => true,
            _ => false,
        }
    }
}

impl From<ConstValue> for Resolved {
    fn from(value: ConstValue) -> Self {
        Self::Value(value)
    }
}

impl<R: ObjectResolver + 'static> From<R> for Resolved {
    fn from(value: R) -> Self {
        Self::Object(Box::new(value))
    }
}

impl<R: Into<Resolved>> From<Vec<R>> for Resolved {
    fn from(value: Vec<R>) -> Self {
        let resolved = value.into_iter().map(|r| r.into()).collect::<Vec<_>>();
        Resolved::Array(resolved)
    }
}
