use crate::value::ConstValue;
use anyhow::Result;
use apollo_compiler::hir;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct ResolverRegistry {
    iface_resolvers:
        HashMap<Arc<hir::InterfaceTypeDefinition>, Box<dyn InterfaceResolver + Send + Sync>>,
    obj_resolvers: HashMap<Arc<hir::ObjectTypeDefinition>, Box<dyn ObjectResolver + Send + Sync>>,
}

impl ResolverRegistry {
    pub fn register_iface<T: InterfaceResolver + Send + Sync + 'static>(
        &mut self,
        ty: Arc<hir::InterfaceTypeDefinition>,
        r: T,
    ) -> () {
        self.iface_resolvers.insert(ty, Box::new(r));
    }

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

pub trait InterfaceResolver {
    fn resolve_concrete_type(&self, output: ()) -> ();
}

#[async_trait::async_trait]
pub trait ObjectResolver {
    async fn resolve_field(&self, name: &str) -> Result<ConstValue>;
}
