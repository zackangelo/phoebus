use crate::{
    introspection::{IspObjectResolver, IspRootResolver},
    resolver::ObjectResolver,
    value::ConstValue,
};
use anyhow::{anyhow, Result};
use apollo_compiler::{
    hir::{InterfaceTypeDefinition, ObjectTypeDefinition, TypeSystem},
    ApolloCompiler, HirDatabase, Snapshot,
};
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{info_span, span, Instrument, Level};

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

        //TODO probably unwise to share a single snapshot for all introspection requests, figure out another way
        // let schema_snapshot = Arc::new(Mutex::new(compiler.snapshot()));

        Ok(Self { compiler })
    }

    pub async fn run<'a, R: ObjectResolver + 'static>(
        &'a mut self,
        query: &'a str,
        query_resolver: R,
    ) -> Result<ConstValue> {
        // let mut compiler = ApolloCompiler::new();
        // self.compiler.set_type_system_hir(self.db.type_system());
        // self.compiler.db.type_system()

        let compile_start = Instant::now();
        let _query_file_id = self.compiler.add_executable(query, "query.graphql");
        println!(
            "compile took: {}μs",
            Instant::now().duration_since(compile_start).as_micros()
        );

        let validate_start = Instant::now();
        let diags = self.compiler.validate();
        println!(
            "validate took: {}μs",
            Instant::now().duration_since(validate_start).as_micros()
        );

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
        let default_query_op = all_ops
            .iter()
            .find(|op| op.name().is_none())
            .ok_or(anyhow!("default query not found"))?;

        let sel_set = default_query_op.selection_set();
        let query_type = default_query_op
            .object_type(&self.compiler.db)
            .ok_or(anyhow!("query type not found"))?;

        let snapshot_start = Instant::now();
        let ts = self.compiler.db.type_system();

        let ectx = ExecCtx { ts: ts.clone() };

        println!(
            "snapshots took: {}μs",
            Instant::now().duration_since(snapshot_start).as_micros()
        );

        let schema_resolver = IspRootResolver {
            // db: snapshot2,
            inner: &query_resolver,
            ts,
        };

        let query_resolver = IspObjectResolver {
            type_def: query_type.clone(),
            inner: &schema_resolver,
        };

        // let ts = self.compiler.db.type_system();

        let query_fut = futures::ExecuteSelectionSet::new(
            &exec_ctx,
            &self.compiler.db,
            &query_resolver,
            query_type,
            sel_set,
        )?;

        query_fut.await
    }
}

pub struct ExecCtx {
    ts: Arc<TypeSystem>,
    // db: &dyn HirDatabase,
}

impl ExecCtx {
    fn find_object_type_definition(&self, name: &str) -> Option<&ObjectTypeDefinition> {
        self.ts.definitions.objects.get(name).map(|o| o.as_ref())
    }

    fn find_interface_type_definition(&self, name: &str) -> Option<&InterfaceTypeDefinition> {
        self.ts.definitions.interfaces.get(name).map(|o| o.as_ref())
    }
}

fn snapshot_is_send<S: Send>(snapshot: S) -> () {
    todo!()
}

fn snapshot_is_sync<S: Sync>(snapshot: S) -> () {
    todo!()
}

fn snapshot_is_send_sync<S: Send + Sync>(snapshot: S) -> () {
    todo!()
}
