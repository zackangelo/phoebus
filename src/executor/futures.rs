//! A set of hand-coded future impls (vs async-await) that power the
//! resolution logic. GraphQL necessarily requires something recursive-looking
//! for this process which can be challenging when working with async/await.

use crate::{
    resolver::{ObjectResolver, Resolved},
    value::{ConstValue, Name},
};
use anyhow::{anyhow, Result};
use apollo_compiler::{
    hir::{self, Field, SelectionSet},
    HirDatabase, Snapshot,
};
use futures::{stream::FuturesOrdered, TryStreamExt};
use indexmap::IndexMap;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub struct ExecuteSelectionSet<'a> {
    field_futs: IndexMap<Name, Pin<Box<dyn Future<Output = Result<ConstValue>> + 'a>>>,
    output_map: IndexMap<Name, ConstValue>,
    field_errors: IndexMap<Name, anyhow::Error>,
}

use super::collect_fields::collect_fields;

impl<'a> ExecuteSelectionSet<'a> {
    pub fn new(
        snapshot: &'a dyn HirDatabase,
        obj_resolver: &'a dyn ObjectResolver,
        object_ty: Arc<hir::ObjectTypeDefinition>,
        sel_set: &'a SelectionSet,
    ) -> Result<Pin<Box<Self>>> {
        let output_map = IndexMap::new();
        let mut field_errors = IndexMap::new();
        let mut field_futs = IndexMap::new();
        let collected_fields = collect_fields(snapshot, sel_set, &object_ty)?;

        //TODO merge selection sets in field groups
        for (response_key, fields) in collected_fields {
            let field = fields
                .first()
                .ok_or(anyhow!(
                    "response key {} in collected fields contained an empty set",
                    response_key
                ))?
                .clone();

            let field_fut = resolve_field(snapshot.clone(), obj_resolver, field);

            match field_fut {
                Ok(ffut) => {
                    field_futs.insert(response_key, ffut);
                }
                Err(err) => {
                    field_errors.insert(response_key, err);
                }
            }
        }

        let fut = Self {
            field_futs,
            output_map,
            field_errors,
        };

        Ok(Box::pin(fut))
    }
}

impl<'a> Future for ExecuteSelectionSet<'a> {
    type Output = Result<ConstValue>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //nb: reference gymnastics necessary here because of
        //mut borrowing multiple fields behind Pin, see: https://github.com/rust-lang/rust/issues/89982
        let self_mut = &mut *self;
        let output_map = &mut self_mut.output_map;
        let field_errors = &mut self_mut.field_errors;
        let field_futs = &mut self_mut.field_futs;

        field_futs.retain(|k, f| match f.as_mut().poll(cx) {
            Poll::Ready(Ok(field_val)) => {
                output_map.insert(k.clone(), field_val);
                false
            }
            Poll::Ready(Err(field_err)) => {
                field_errors.insert(k.clone(), field_err);
                false
            }
            Poll::Pending => true,
        });

        if self.field_futs.is_empty() {
            if !self.field_errors.is_empty() {
                Poll::Ready(Err(anyhow!("field errors: {:?}", self.field_errors)))
            } else {
                Poll::Ready(Ok(ConstValue::Object(self.output_map.clone()))) //TODO remove clone
            }
        } else {
            Poll::Pending
        }
    }
}

fn resolve_field<'a>(
    snapshot: &'a dyn HirDatabase,
    resolver: &'a dyn ObjectResolver,
    field: Arc<Field>,
) -> Result<Pin<Box<dyn Future<Output = Result<ConstValue>> + 'a>>> {
    Ok(Box::pin(async move {
        let resolved = resolver.resolve_field(field.name()).await?;
        resolve_to_value(snapshot, field, resolved).await
    }))
}

fn resolve_to_value<'a>(
    snapshot: &'a dyn HirDatabase,
    field: Arc<Field>,
    resolved: Resolved,
) -> Pin<Box<dyn Future<Output = Result<ConstValue>> + 'a>> {
    use hir::TypeDefinition::*;

    // let snapshot = snapshot.clone();
    Box::pin(async move {
        let field_def = field.field_definition(snapshot).ok_or(anyhow!(
            "field definition not found for field: {:#?}",
            field.as_ref()
        ))?;

        let field_ty = field_def.ty();

        let field_type_def = field_ty
            .type_def(snapshot)
            .ok_or(anyhow!("field type definition not found"))?;

        match resolved {
            Resolved::Value(v) => Ok(v),
            Resolved::Object(obj_resolver) => {
                let object_ty = match field_type_def {
                    ObjectTypeDefinition(o) => o,
                    InterfaceTypeDefinition(iface) => {
                        let type_name = obj_resolver.resolve_type_name().await?.ok_or(anyhow!(
                            "resolver did not return concrete type for {}",
                            iface.name()
                        ))?;

                        snapshot
                            .find_object_type_by_name(type_name.to_owned())
                            .ok_or(anyhow!("concrete object type not found: {}", type_name))?
                    }
                    _ => return Err(anyhow!("type mismatch: object type expected")),
                };

                let obj_resolver = crate::introspection::IspObjectResolver {
                    type_def: object_ty.clone(),
                    inner: obj_resolver.as_ref(),
                };

                let obj_fut = ExecuteSelectionSet::new(
                    snapshot.clone(),
                    &obj_resolver,
                    object_ty,
                    field.selection_set(),
                )?;

                Ok(obj_fut.await?)
            }
            Resolved::Array(arr) => {
                let mut futs = FuturesOrdered::new();

                for element in arr {
                    let fut = resolve_to_value(snapshot.clone(), field.clone(), element);
                    futs.push_back(fut);
                }

                let vals: Vec<ConstValue> = futs.try_collect().await?; //FIXME should not short-circuit here, need to collect errors from each element

                Ok(ConstValue::List(vals))
            }
        }
    })
}
