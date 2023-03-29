use crate::{
    introspection::{IspObjectResolver, IspRootResolver},
    resolver::ObjectResolver,
    value::ConstValue,
};
use anyhow::{anyhow, Result};
use apollo_compiler::{
    hir::{
        Field, FieldDefinition, FragmentDefinition, ObjectTypeDefinition, TypeDefinition,
        TypeSystem,
    },
    ApolloCompiler, HirDatabase,
};
use std::{collections::HashMap, time::Instant};

use std::sync::Arc;

mod collect_fields;
mod futures;

pub struct Executor {
    type_system: Arc<TypeSystem>,
    exec_schema: Arc<ExecSchema>,
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

        let type_system = compiler.db.type_system();
        let exec_schema = Arc::new(ExecSchema::new(&compiler.db));

        Ok(Self {
            // compiler,
            type_system,
            exec_schema,
        })
    }

    pub async fn run<'a, R: ObjectResolver + 'static>(
        &'a self,
        query: &'a str,
        query_resolver: R,
    ) -> Result<ConstValue> {
        let mut compiler = ApolloCompiler::new();
        compiler.set_type_system_hir(self.type_system.clone());

        let compile_start = Instant::now();
        let _query_file_id = compiler.add_executable(query, "query.graphql");
        tracing::debug!(
            "compile took: {}μs",
            Instant::now().duration_since(compile_start).as_micros()
        );

        let validate_start = Instant::now();
        let diags = compiler.validate();
        tracing::debug!(
            "validate took: {}μs",
            Instant::now().duration_since(validate_start).as_micros()
        );

        for diag in diags.iter() {
            if diag.data.is_error() {
                tracing::error!("query error: {}", diag);
            }
        }

        let _has_errors = diags.iter().filter(|d| d.data.is_error()).count() > 0;
        // https://github.com/apollographql/apollo-rs/issues/504
        // if has_errors {
        //     return Err(anyhow!("graphql had errors"));
        // }
        let ectx = ExecCtx::new(&compiler.db, self.exec_schema.clone());

        let result_fut = tokio::spawn(async move {
            let all_ops = compiler.db.all_operations();
            let default_query_op = all_ops
                .iter()
                .find(|op| op.name().is_none())
                .ok_or_else(|| anyhow!("default query not found"))?;

            let sel_set = default_query_op.selection_set();
            let query_type = default_query_op
                .object_type(&compiler.db)
                .ok_or_else(|| anyhow!("query type not found"))?;

            let snapshot_start = Instant::now();
            let ts = compiler.db.type_system();

            tracing::debug!(
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

            let query_fut =
                futures::ExecuteSelectionSet::new(&ectx, &query_resolver, query_type, sel_set)?;

            let exec_start = Instant::now();
            let result = query_fut.await;
            tracing::info!(
                "query took {}μs",
                Instant::now().duration_since(exec_start).as_micros()
            );
            result
        });

        result_fut.await?
    }
}

pub struct ExecSchema {
    ts: Arc<TypeSystem>,
    //TODO would rather just have a big flat map here but couldn't get a tuple string key to work immediately
    all_fields: HashMap<String, HashMap<String, FieldDefinition>>,
}

impl ExecSchema {
    fn new<DB: HirDatabase>(db: &DB) -> Self {
        let ts = db.type_system();
        let mut all_fields = HashMap::new();

        for (k, v) in db.types_definitions_by_name().iter() {
            let field_map: HashMap<String, FieldDefinition> = match v {
                TypeDefinition::ObjectTypeDefinition(ty) => ty
                    .fields()
                    .chain(ty.implicit_fields(db))
                    .cloned()
                    .map(|f| (f.name().to_owned(), f))
                    .collect(),
                TypeDefinition::InterfaceTypeDefinition(ty) => ty
                    .fields()
                    .chain(ty.implicit_fields().iter())
                    .cloned()
                    .map(|f| (f.name().to_owned(), f))
                    .collect(),
                _ => HashMap::new(), //TODO fix
            };

            all_fields.insert(k.to_owned(), field_map);
        }

        Self { ts, all_fields }
    }
}

#[derive(Clone)]
pub struct ExecCtx {
    schema: Arc<ExecSchema>,
    fragments: HashMap<String, FragmentDefinition>,
}

impl ExecCtx {
    fn new<DB: HirDatabase>(db: &DB, schema: Arc<ExecSchema>) -> Self {
        let mut fragments = HashMap::new();

        for (name, frag) in db.all_fragments().iter() {
            fragments.insert(name.clone(), frag.as_ref().clone());
        }

        Self { fragments, schema }
    }

    fn field_definition(&self, field: &Field) -> Option<&FieldDefinition> {
        let type_name = field.parent_type_name()?;
        self.schema.all_fields.get(type_name)?.get(field.name())
    }

    fn find_type_definition_by_name(&self, name: &str) -> Option<&TypeDefinition> {
        self.schema.ts.type_definitions_by_name.get(name)
    }

    fn find_object_type_definition(&self, name: &str) -> Option<&ObjectTypeDefinition> {
        self.schema
            .ts
            .definitions
            .objects
            .get(name)
            .map(|o| o.as_ref())
    }

    fn fragment(&self, name: &str) -> Option<&FragmentDefinition> {
        self.fragments.get(name)
    }

    fn is_subtype(&self, concrete_type: &str, abstract_type: &str) -> bool {
        if let Some(ats) = self.schema.ts.subtype_map.get(concrete_type) {
            ats.contains(abstract_type)
        } else {
            false
        }
    }

    // fn find_interface_type_definition(&self, name: &str) -> Option<&InterfaceTypeDefinition> {
    //     self.ts.definitions.interfaces.get(name).map(|o| o.as_ref())
    // }
}
