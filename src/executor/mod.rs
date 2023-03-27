use crate::{
    resolver::{ObjectResolver, ResolverRegistry},
    value::{ConstValue, Name},
};
use anyhow::{anyhow, Result};
use apollo_compiler::{
    hir::{self, ObjectTypeDefinition, TypeSystem},
    ApolloCompiler, HirDatabase, RootDatabase,
};

use std::sync::Arc;

mod futures;

pub struct Ctx {
    args: (),
}

pub struct Executor {
    compiler: ApolloCompiler,
    resolvers: ResolverRegistry,
}

impl Executor {
    pub fn new(schema: &str) -> Result<Self> {
        let mut compiler = ApolloCompiler::new();
        compiler.add_type_system(schema, "schema.graphql");

        let diags = compiler.validate();
        let has_errors = diags.iter().filter(|d| d.data.is_error()).count() > 0;

        for diag in diags.iter() {
            // if diag.data.is_error() {
            tracing::error!("{}", diag);
            // }
        }

        if has_errors {
            return Err(anyhow!("graphql had errors"));
        }

        // let type_system = compiler.db.type_system();

        Ok(Self {
            compiler,
            resolvers: ResolverRegistry::default(),
        })
    }

    pub fn query_type(&self) -> Option<Arc<ObjectTypeDefinition>> {
        self.type_system()
            .definitions
            .schema
            .query(&self.compiler.db)
    }

    pub fn object_type_def(&self, name: &str) -> Option<Arc<ObjectTypeDefinition>> {
        self.type_system()
            .type_definitions_by_name
            .get(name)
            .and_then(|ty| match ty {
                hir::TypeDefinition::ObjectTypeDefinition(obj_ty) => Some(obj_ty.clone()),
                _ => None,
            })
    }

    pub fn type_system(&self) -> Arc<TypeSystem> {
        self.compiler.db.type_system().clone()
    }

    pub fn resolvers_mut(&mut self) -> &mut ResolverRegistry {
        &mut self.resolvers
    }

    pub async fn run(
        &mut self,
        query: &str,
        query_resolver: &dyn ObjectResolver,
    ) -> Result<ConstValue> {
        // let mut compiler = ApolloCompiler::new();
        // self.compiler.set_type_system_hir(self.db.type_system());
        let _query_file_id = self.compiler.add_executable(query, "query.graphql");

        let diags = self.compiler.validate();
        let has_errors = diags.iter().filter(|d| d.data.is_error()).count() > 0;

        for diag in diags.iter() {
            // if diag.data.is_error() {
            tracing::error!("query error: {}", diag);
            // }
        }

        if has_errors {
            return Err(anyhow!("graphql had errors"));
        }

        let all_ops = self.compiler.db.all_operations();

        // dbg!(&all_ops);

        let default_query_op = all_ops
            .iter()
            .find(|op| op.name().is_none())
            .ok_or(anyhow!("default query not found"))?;

        let sel_set = default_query_op.selection_set();
        let query_type = default_query_op
            .object_type(&self.compiler.db)
            .ok_or(anyhow!("query type not found"))?;

        let snapshot = self.compiler.snapshot();

        let query_fut = futures::SelectionSetFuture::new(
            Arc::new(snapshot),
            query_resolver,
            query_type,
            sel_set,
        )?;

        query_fut.await
    }
}
