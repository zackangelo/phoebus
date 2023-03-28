use crate::{resolver::ObjectResolver, value::ConstValue};
use anyhow::{anyhow, Result};
use apollo_compiler::{ApolloCompiler, HirDatabase};

use std::sync::Arc;

mod collect_fields;
mod futures;

pub struct Executor {
    compiler: ApolloCompiler,
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

        Ok(Self { compiler })
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

        // let pet_type = snapshot.find_object_type_by_name("Pet".to_owned());
        // dbg!(pet_type);

        let query_fut = futures::ExecuteSelectionSet::new(
            Arc::new(snapshot),
            query_resolver,
            query_type,
            sel_set,
        )?;

        query_fut.await
    }
}
