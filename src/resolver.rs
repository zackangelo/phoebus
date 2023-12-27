use std::{collections::HashMap, fmt::Display, sync::Arc};

use crate::value::{ConstValue, Name};
use anyhow::{anyhow, Result};
use apollo_compiler::hir::{self, Value};
use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::Number;

/// Resolver context
pub struct Ctx {
    pub(crate) variables: Arc<HashMap<String, ConstValue>>,
    pub(crate) field: Arc<hir::Field>,
}

impl Ctx {
    //FIXME this is probably wrong and also would probably be easier to do
    // in an upstream phase that eagerly resolves all the variables first
    fn resolve_vars(&self, arg_value: &Value) -> Result<CtxArg> {
        let const_v: ConstValue = match arg_value {
            Value::Variable(var) => self
                .variables
                .get(var.name())
                .ok_or_else(|| anyhow!("undefined variable: {}", var.name()))?
                .clone(),
            Value::Object { value, .. } => {
                let fields: IndexMap<Name, ConstValue> = value
                    .iter()
                    .map(|(k, v)| {
                        (
                            Name::new(k.clone().src().to_owned()),
                            self.resolve_vars(v).unwrap().0, //FIXME unwrap
                        )
                    })
                    .collect::<IndexMap<_, _>>();
                ConstValue::Object(fields)
            }
            Value::List { value, .. } => {
                let values = value
                    .iter()
                    .map(|v| self.resolve_vars(v).unwrap().0) //FIXME unwrap()
                    .collect::<Vec<ConstValue>>();

                ConstValue::List(values)
            }
            Value::Boolean { value, .. } => ConstValue::Boolean(*value),
            Value::String { value, .. } => ConstValue::String(value.clone()),
            Value::Int { value, .. } => {
                ConstValue::Number(Number::from(value.to_i32_checked().unwrap()))
            }
            Value::Float { value, .. } => {
                ConstValue::Number(Number::from_f64(value.get()).unwrap())
            }
            Value::Enum { value, .. } => ConstValue::Enum(Name::new(value.src())),
            Value::Null { .. } => ConstValue::Null,
        };

        Ok(CtxArg(const_v.clone()))
    }

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

        let arg_const_v = self.resolve_vars(arg.value())?;

        T::try_from(arg_const_v).map_err(|err| anyhow!("argument conversion error: {}", err))
    }

    pub fn arg<T: TryFrom<CtxArg>>(&self, name: &str) -> Option<T>
    where
        T::Error: Display,
    {
        match self.try_arg(name) {
            Ok(v) => Some(v),
            Err(err) => {
                tracing::error!("argument error: {}", err);
                None
            } // _ => None,
        }
    }
}

#[repr(transparent)]
pub struct CtxArg(ConstValue);

impl TryFrom<CtxArg> for String {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0 {
            ConstValue::String(s) => Ok(s),
            _ => Err(anyhow!("invalid argument type, expected string")),
        }
    }
}

impl TryFrom<CtxArg> for i32 {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0 {
            ConstValue::Number(num) if num.is_i64() => {
                let inum = num.as_i64().unwrap();
                Ok(inum.try_into()?)
            }
            _ => Err(anyhow!("invalid argument type, expected integer")),
        }
    }
}

impl TryFrom<CtxArg> for f64 {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0 {
            ConstValue::Number(n) if n.is_f64() => {
                let num_f = n.as_f64().unwrap();
                Ok(num_f)
            }
            _ => Err(anyhow!("invalid argument type, expected float")),
        }
    }
}

impl TryFrom<CtxArg> for bool {
    type Error = anyhow::Error;

    fn try_from(value: CtxArg) -> std::result::Result<Self, Self::Error> {
        match value.0 {
            ConstValue::Boolean(b) => Ok(b),
            _ => Err(anyhow!("invalid argument type, expected bool")),
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
