use crate::value::ConstValue;
use anyhow::Result;
use apollo_compiler::hir;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct ResolverRegistry {
    obj_resolvers: HashMap<Arc<hir::ObjectTypeDefinition>, Box<dyn ObjectResolver + Send + Sync>>,
}

impl ResolverRegistry {
    pub fn register_obj<T: ObjectResolver + Send + Sync + 'static>(
        &mut self,
        ty: Arc<hir::ObjectTypeDefinition>,
        r: T,
    ) -> () {
        self.obj_resolvers.insert(ty, Box::new(r));
    }

    pub fn for_obj(
        &self,
        ty: &Arc<hir::ObjectTypeDefinition>,
    ) -> Option<&Box<dyn ObjectResolver + Send + Sync + 'static>> {
        self.obj_resolvers.get(ty)
    }
}

#[async_trait::async_trait]
pub trait ObjectResolver {
    /// Resolves the type of a field that has an interface or union type
    async fn resolve_field_type(&self, name: &str) -> Result<String>;

    /// Resolves the value of the specified field
    async fn resolve_field(&self, name: &str) -> Result<ConstValue>;
}
