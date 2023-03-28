use crate::value::{ConstValue, Name};
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

pub enum Resolved {
    Value(ConstValue),
    Object(Box<dyn ObjectResolver>),
    Array(Vec<Resolved>),
}

impl Resolved {
    pub fn null() -> Self {
        Self::Value(ConstValue::Null)
    }

    pub fn is_null(&self) -> bool {
        match self {
            Self::Value(ConstValue::Null) => true,
            _ => false,
        }
    }

    pub fn object<R: ObjectResolver + 'static>(resolver: R) -> Self {
        Self::Object(Box::new(resolver))
    }

    pub fn enum_value<S: AsRef<str>>(v: S) -> Self {
        Self::Value(ConstValue::Enum(Name::new(v)))
    }

    pub fn string<S: AsRef<str>>(v: S) -> Self {
        Self::Value(ConstValue::String(v.as_ref().to_owned()))
    }

    pub fn string_opt<S: AsRef<str>>(v: Option<S>) -> Self {
        match v {
            Some(v) => Self::string(v),
            None => Self::null(),
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
