use std::{fmt::Display, sync::Arc};

use crate::value::{ConstValue, Name};
use anyhow::{anyhow, Result};
use apollo_compiler::hir;
use async_trait::async_trait;

/// Resolver context
pub struct Ctx {
    pub(crate) field: Arc<hir::Field>,
}

impl Ctx {
    pub fn try_arg<T: TryFrom<CtxArg>>(&self, name: &str) -> Result<T>
    where
        T::Error: Display,
    {
        let arg = self
            .field
            .arguments()
            .into_iter()
            .find(|a| a.name() == name)
            .ok_or_else(|| anyhow!("argument not found: {}", name))?;

        let arg = arg.clone(); //TODO remove find/clone by passing in a map
        T::try_from(CtxArg(arg)).map_err(|err| anyhow!("argument conversion error: {}", err))
    }

    pub fn arg<T: TryFrom<CtxArg>>(&self, name: &str) -> Option<T> {
        let arg = self
            .field
            .arguments()
            .into_iter()
            .find(|a| a.name() == name);

        let arg = arg.cloned(); //TODO remove find/clone by passing in a map

        match arg {
            Some(arg) => T::try_from(CtxArg(arg)).ok(),
            None => None,
        }
    }
}

#[repr(transparent)]
pub struct CtxArg(hir::Argument);

impl TryFrom<CtxArg> for String {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0.value() {
            hir::Value::String { value: s, .. } => Ok(s.clone()),
            _ => Err(anyhow!("invalid argument type")),
        }
    }
}

impl TryFrom<CtxArg> for i32 {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0.value() {
            hir::Value::Int { value: f, .. } => {
                f.to_i32_checked().ok_or(anyhow!("int conversion error"))
            }
            _ => Err(anyhow!("invalid argument type")),
        }
    }
}

impl TryFrom<CtxArg> for f64 {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0.value() {
            hir::Value::Float { value: f, .. } => Ok(f.get()),
            _ => Err(anyhow!("invalid argument type")),
        }
    }
}

impl TryFrom<CtxArg> for bool {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0.value() {
            hir::Value::Boolean { value: f, .. } => Ok(*f),
            _ => Err(anyhow!("invalid argument type")),
        }
    }
}

#[async_trait::async_trait]
pub trait ObjectResolver: Send + Sync {
    /// Resolves the concrete type of this if it's a polymorphic type
    async fn resolve_type_name(&self) -> Result<Option<&str>> {
        Ok(None)
    }

    /// Resolves the value of the specified field
    async fn resolve_field(&self, ctx: &Ctx, name: &str) -> Result<Resolved>;
}

pub enum Resolved {
    Value(ConstValue),
    Object(Box<dyn ObjectResolver>),
    Array(Vec<Resolved>),
}

impl Resolved {
    #[inline]
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

// impl<V: AsRef<str>> From<V> for Resolved {
//     fn from(value: V) -> Self {
//         Self::Value(value.into())
//     }
// }

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

#[async_trait]
impl<T: ObjectResolver> ObjectResolver for Arc<T> {
    async fn resolve_field(&self, ctx: &Ctx, name: &str) -> Result<Resolved> {
        T::resolve_field(&self, ctx, name).await
    }
}
