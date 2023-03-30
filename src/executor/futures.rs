//! A set of hand-coded future impls (vs async-await) that power the
//! resolution logic. GraphQL necessarily requires something recursive-looking
//! for this process which can be challenging when working with async/await.

use crate::{
    resolver::{ObjectResolver, Resolved},
    value::{self, ConstValue},
    Ctx,
};
use anyhow::{anyhow, Result};
use apollo_compiler::hir::{self, Field, SelectionSet};
use futures::{stream::FuturesOrdered, TryStreamExt};
use indexmap::IndexMap;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};
use tracing::{debug, span, Instrument, Level};

pub struct ExecuteSelectionSet<'a> {
    field_futs: IndexMap<String, Pin<Box<dyn Future<Output = Result<ConstValue>> + Send + 'a>>>,
    output_map: Option<IndexMap<value::Name, ConstValue>>,
    field_errors: IndexMap<String, anyhow::Error>,
}

use super::{collect_fields::collect_fields, ExecCtx};

impl<'a> ExecuteSelectionSet<'a> {
    pub fn new(
        ectx: &'a ExecCtx,
        obj_resolver: &'a dyn ObjectResolver,
        object_ty: Arc<hir::ObjectTypeDefinition>,
        sel_set: &'a SelectionSet,
    ) -> Result<Pin<Box<Self>>> {
        let output_map = Some(IndexMap::new());
        let mut field_errors = IndexMap::new();
        let mut field_futs = IndexMap::new();
        let collected_fields = collect_fields(ectx, sel_set, &object_ty)?;

        //TODO merge selection sets in field groups
        for (response_key, fields) in collected_fields {
            let field = fields
                .first()
                .ok_or(anyhow!(
                    "response key {} in collected fields contained an empty set",
                    response_key
                ))?
                .clone();

            let field_fut = resolve_field(ectx, obj_resolver, field.clone());

            //FIXME fields out of order when constructed in this way, need to pre-arrange fields in ::new()
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
        let output_map = self_mut.output_map.as_mut().expect("output_map missing");
        let field_errors = &mut self_mut.field_errors;
        let field_futs = &mut self_mut.field_futs;

        field_futs.retain(|k, f| {
            let field_poll = f.as_mut().poll(cx);

            match field_poll {
                Poll::Ready(Ok(field_val)) => {
                    output_map.insert(value::Name::new(k), field_val);
                    false
                }
                Poll::Ready(Err(field_err)) => {
                    field_errors.insert(k.clone(), field_err);
                    false
                }
                Poll::Pending => true,
            }
        });

        let poll = if self.field_futs.is_empty() {
            if !self.field_errors.is_empty() {
                Poll::Ready(Err(anyhow!("field errors: {:?}", self.field_errors)))
            } else {
                let result = self.output_map.take().expect("output map state error");
                Poll::Ready(Ok(result.into())) //TODO remove clone
            }
        } else {
            Poll::Pending
        };

        poll
    }
}

fn resolve_field<'a>(
    ectx: &'a ExecCtx,
    resolver: &'a dyn ObjectResolver,
    field: Arc<Field>,
) -> Result<Pin<Box<dyn Future<Output = Result<ConstValue>> + Send + 'a>>> {
    let span = span!(Level::INFO, "field", "{}", field.name());
    Ok(Box::pin(
        async move {
            let ctx = Ctx {
                field: field.clone(),
            };

            let start = Instant::now();
            let resolved = resolver.resolve_field(&ctx, field.name()).await?;
            let self_end = Instant::now();
            let v = resolve_to_value(ectx, field, resolved).await;
            let end = Instant::now();
            debug!(
                "time self: {}μs, full: {}μs",
                self_end.duration_since(start).as_micros(),
                end.duration_since(start).as_micros()
            );
            v
        }
        .instrument(span),
    ))
}

fn resolve_to_value<'a>(
    ectx: &'a ExecCtx,
    field: Arc<Field>,
    resolved: Resolved,
) -> Pin<Box<dyn Future<Output = Result<ConstValue>> + Send + 'a>> {
    use futures::FutureExt;
    use hir::TypeDefinition::*;

    match resolved {
        Resolved::Value(v) => Box::pin(futures::future::ready(Ok(v))),
        Resolved::Array(arr) => {
            let mut futs = FuturesOrdered::new();

            let mut ix = 0;
            for element in arr {
                let span = span!(Level::DEBUG, "ix", "{}", ix);
                let fut = resolve_to_value(ectx, field.clone(), element).instrument(span);
                futs.push_back(fut);
                ix = ix + 1;
            }

            let vals = futs
                .try_collect()
                .map(|vs: Result<Vec<_>>| vs.map(|vs| ConstValue::List(vs))); //FIXME should not short-circuit here, need to collect errors from each element

            Box::pin(vals)
        }
        Resolved::Object(obj_resolver) => {
            Box::pin(async move {
                let field_def = ectx.field_definition(&field).ok_or_else(|| {
                    anyhow!(
                        "field definition not found for field: {:#?}",
                        field.as_ref()
                    )
                })?;

                let field_ty = field_def.ty();

                let field_type_def = ectx
                    .find_type_definition_by_name(&field_ty.name()) // TODO why String instead of &str?
                    .ok_or_else(|| anyhow!("field type definition not found"))?;

                let object_ty = match field_type_def {
                    ObjectTypeDefinition(o) => o,
                    InterfaceTypeDefinition(iface) => {
                        let type_name =
                            obj_resolver.resolve_type_name().await?.ok_or_else(|| {
                                anyhow!(
                                    "resolver did not return concrete type for {}",
                                    iface.name()
                                )
                            })?;

                        ectx.find_object_type_definition(type_name).ok_or_else(|| {
                            anyhow!("concrete object type not found: {}", type_name)
                        })?
                    }
                    _ => return Err(anyhow!("type mismatch: object type expected")),
                };

                let object_ty = Arc::new(object_ty.clone());

                let obj_resolver = crate::introspection::IspObjectResolver {
                    type_def: object_ty.clone(),
                    inner: obj_resolver.as_ref(),
                };

                let obj_fut = ExecuteSelectionSet::new(
                    ectx,
                    &obj_resolver,
                    object_ty,
                    field.selection_set(),
                )?;

                Ok(obj_fut.await?)
            })
        }
    }
}
