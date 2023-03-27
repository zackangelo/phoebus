//! A set of hand-coded future impls (vs async-await) that power the
//! resolution logic. GraphQL necessarily requires something recursive-looking
//! for this process which can be challenging when working with async/await.

use crate::{
    resolver::{ObjectResolver, ResolverRegistry},
    value::{ConstValue, Name},
};
use anyhow::{anyhow, Result};
use apollo_compiler::{
    hir::{self, Field, Selection, SelectionSet},
    Snapshot,
};
use indexmap::IndexMap;
use std::{future::Future, ops::DerefMut, pin::Pin, sync::Arc, task::Poll};

pub struct SelectionSetFuture<'a> {
    // snapshot: Arc<Snapshot>,
    // resolvers: &'a ResolverRegistry,
    // obj_resolver: &'a dyn ObjectResolver,
    // object_ty: Arc<hir::ObjectTypeDefinition>,
    field_futs: IndexMap<Name, Pin<Box<FieldFuture<'a>>>>,
    output_map: IndexMap<Name, ConstValue>,
    field_errors: IndexMap<Name, anyhow::Error>,
}

impl<'a> SelectionSetFuture<'a> {
    pub fn new(
        snapshot: Arc<Snapshot>,
        resolvers: &'a ResolverRegistry,
        object_ty: Arc<hir::ObjectTypeDefinition>,
        sel_set: &'a SelectionSet,
    ) -> Result<Pin<Box<Self>>> {
        let output_map = IndexMap::new();
        let mut field_errors = IndexMap::new();

        let obj_resolver = resolvers
            .for_obj(&object_ty)
            .ok_or(anyhow!("resolver not found for object type"))?
            .as_ref();

        let mut field_futs = IndexMap::new();
        for sel in sel_set.selection() {
            match sel {
                Selection::Field(field) => {
                    let output_key = Name::new(
                        field
                            .alias()
                            .map(|a| a.0.as_str())
                            .unwrap_or_else(|| field.name()),
                    );

                    let ffut = FieldFuture::new(snapshot.clone(), resolvers, obj_resolver, field);

                    match ffut {
                        Ok(ffut) => {
                            field_futs.insert(output_key, Box::pin(ffut));
                        }
                        Err(err) => {
                            field_errors.insert(output_key, err);
                        }
                    };
                }
                Selection::FragmentSpread(_) => todo!(),
                Selection::InlineFragment(_) => todo!(),
            }
        }

        let fut = Self {
            // snapshot,
            // resolvers,
            // obj_resolver,
            // object_ty,
            field_futs,
            output_map,
            field_errors,
        };

        Ok(Box::pin(fut))
    }
}

impl<'a> Future for SelectionSetFuture<'a> {
    type Output = Result<ConstValue>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        //nb: reference gymnastics necessary here because of
        //mut borrowing multiple fields behind Pin, see: https://github.com/rust-lang/rust/issues/89982
        let self_mut = &mut self.deref_mut();
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

pub struct FieldFuture<'r> {
    // snapshot: Arc<Snapshot>,
    // resolvers: &'r ResolverRegistry,
    // resolver: &'r dyn ObjectResolver,
    // field: &'r Field,
    inner: InnerFieldFut<'r>,
}

enum InnerFieldFut<'a> {
    Resolver(Pin<Box<dyn Future<Output = Result<ConstValue>> + 'a>>),
    SelectionSet(Pin<Box<SelectionSetFuture<'a>>>),
}

impl<'a> FieldFuture<'a> {
    pub fn new(
        snapshot: Arc<Snapshot>,
        resolvers: &'a ResolverRegistry,
        resolver: &'a dyn ObjectResolver,
        field: &'a Field,
    ) -> Result<Self> {
        use hir::TypeDefinition::*;
        let field_ty = field
            .ty(&**snapshot)
            .ok_or(anyhow!("field type not found"))?;
        let field_type_def = field_ty
            .type_def(&**snapshot)
            .ok_or(anyhow!("field type definition not found"))?;

        let field_name = field.name();
        let inner = match field_type_def {
            ScalarTypeDefinition(_scalar_ty) => {
                InnerFieldFut::Resolver(resolver.resolve_field(field_name))
            }
            ObjectTypeDefinition(object_ty) => {
                InnerFieldFut::SelectionSet(SelectionSetFuture::new(
                    snapshot.clone(),
                    resolvers,
                    object_ty,
                    field.selection_set(),
                )?)
            }
            InterfaceTypeDefinition(_) => todo!(),
            UnionTypeDefinition(_) => todo!(),
            EnumTypeDefinition(_) => todo!(),
            InputObjectTypeDefinition(_) => todo!(),
        };

        Ok(Self { inner })
    }
}

impl<'a> Future for FieldFuture<'a> {
    type Output = Result<ConstValue>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        match &mut self.as_mut().inner {
            InnerFieldFut::Resolver(f) => f.as_mut().poll(cx),
            InnerFieldFut::SelectionSet(f) => f.as_mut().poll(cx),
        }
    }
}
