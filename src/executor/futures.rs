//! A set of hand-coded future impls (vs async-await) that power the
//! resolution logic. GraphQL necessarily requires something recursive-looking
//! for this process which can be challenging when working with async/await.

use crate::{
    resolver::{ObjectResolver, Resolved},
    value::{ConstValue, Name},
};
use anyhow::{anyhow, Result};
use apollo_compiler::{
    hir::{self, Directive, Field, ObjectTypeDefinition, Selection, SelectionSet, TypeDefinition},
    HirDatabase, Snapshot,
};
use futures::{
    stream::{FuturesOrdered, FuturesUnordered},
    TryStreamExt,
};
use indexmap::IndexMap;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub struct SelectionSetFuture<'a> {
    // snapshot: Arc<Snapshot>,
    // resolvers: &'a ResolverRegistry,
    // obj_resolver: &'a dyn ObjectResolver,
    // object_ty: Arc<hir::ObjectTypeDefinition>,
    field_futs: IndexMap<Name, Pin<Box<dyn Future<Output = Result<ConstValue>> + 'a>>>,
    output_map: IndexMap<Name, ConstValue>,
    field_errors: IndexMap<Name, anyhow::Error>,
}

impl<'a> SelectionSetFuture<'a> {
    pub fn new(
        snapshot: Arc<Snapshot>,
        obj_resolver: &'a dyn ObjectResolver,
        object_ty: Arc<hir::ObjectTypeDefinition>,
        sel_set: &'a SelectionSet,
    ) -> Result<Pin<Box<Self>>> {
        let output_map = IndexMap::new();
        let mut field_errors = IndexMap::new();

        // let obj_resolver = resolvers
        //     .for_obj(&object_ty)
        //     .ok_or(anyhow!("resolver not found for object type"))?
        //     .as_ref();

        // let mut field_futs = IndexMap::new();
        // for sel in sel_set.selection() {
        //     match sel {
        //         Selection::Field(field) => {
        //             let output_key = Name::new(
        //                 field
        //                     .alias()
        //                     .map(|a| a.0.as_str())
        //                     .unwrap_or_else(|| field.name()),
        //             );

        //             // let ffut = FieldFuture::new(snapshot.clone(), resolvers, obj_resolver, field);
        //             let ffut = resolve_field(snapshot.clone(), obj_resolver, field);

        //             match ffut {
        //                 Ok(ffut) => {
        //                     field_futs.insert(output_key, ffut);
        //                 }
        //                 Err(err) => {
        //                     field_errors.insert(output_key, err);
        //                 }
        //             };
        //         }
        //         Selection::FragmentSpread(_) => todo!(),
        //         Selection::InlineFragment(_) => todo!(),
        //     }
        // }

        let mut field_futs = IndexMap::new();
        let collected_fields = collect_fields(&snapshot, sel_set, &object_ty)?;

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

fn sel_directives(selection: &Selection) -> &[Directive] {
    match selection {
        Selection::Field(field) => field.directives(),
        Selection::FragmentSpread(frag) => frag.directives(),
        Selection::InlineFragment(frag) => frag.directives(),
    }
}

fn skip_directive(selection: &Selection) -> Option<&Directive> {
    sel_directives(selection)
        .iter()
        .find(|d| d.name() == "skip")
}

fn include_directive(selection: &Selection) -> Option<&Directive> {
    sel_directives(selection)
        .iter()
        .find(|d| d.name() == "include")
}

fn should_skip(sel: &Selection) -> Result<bool> {
    let skip_directive = skip_directive(sel);

    if let Some(skip) = skip_directive {
        let if_arg = skip
            .argument_by_name("if")
            .ok_or(anyhow!("if expression missing from @skip"))?;

        match if_arg {
            hir::Value::Boolean(skip_if) => Ok(*skip_if),
            hir::Value::Variable(_var) => todo!(),
            _ => Err(anyhow!("invalid @skip if argument")),
        }
    } else {
        Ok(false)
    }
}

fn should_include(sel: &Selection) -> Result<bool> {
    let include_directive = include_directive(sel);

    if let Some(include) = include_directive {
        let if_arg = include
            .argument_by_name("if")
            .ok_or(anyhow!("if expression missing from @include"))?;

        match if_arg {
            hir::Value::Boolean(include_if) => Ok(*include_if),
            hir::Value::Variable(_var) => todo!(),
            _ => Err(anyhow!("invalid @include if argument")),
        }
    } else {
        Ok(true)
    }
}

fn fragment_type_applies(
    obj_type: &ObjectTypeDefinition,
    frag_type: &TypeDefinition,
) -> Result<bool> {
    match frag_type {
        TypeDefinition::ObjectTypeDefinition(obj_frag_type) => {
            Ok(obj_type == obj_frag_type.as_ref())
        }
        TypeDefinition::InterfaceTypeDefinition(obj_iface_type) => Ok(obj_type
            .implements_interfaces()
            .iter()
            .find(|ii| ii.interface() == obj_iface_type.name())
            .is_some()),
        TypeDefinition::UnionTypeDefinition(union_type) => Ok(union_type
            .union_members()
            .iter()
            .find(|ut| ut.name() == obj_type.name())
            .is_some()),
        invalid @ _ => Err(anyhow!(
            "invalid type in fragment type condition {}",
            invalid.name()
        )),
    }
}

/// Collects a selection set's fields and fragments into a flattened represention to
/// ensure resolvers are not invoked more than once for a given field.
///
/// FIXME track visitedFragments according to spec
///
/// https://spec.graphql.org/draft/#sec-Field-Collection
fn collect_fields(
    snapshot: &Snapshot,
    sel_set: &SelectionSet,
    concrete_type: &ObjectTypeDefinition,
) -> Result<IndexMap<Name, Vec<Arc<Field>>>> {
    fn inner(
        snapshot: &Snapshot,
        sel_set: &SelectionSet,
        concrete_type: &ObjectTypeDefinition,
        grouped_fields: &mut IndexMap<Name, Vec<Arc<Field>>>,
    ) -> Result<()> {
        for sel in sel_set.selection() {
            if should_skip(sel)? || !should_include(sel)? {
                continue;
            }

            match sel {
                Selection::Field(field) => {
                    let response_key = field.alias().map(|a| a.0.as_str()).unwrap_or(field.name());
                    let response_key = Name::new(response_key);
                    let field_entry = grouped_fields.entry(response_key);
                    field_entry.or_default().push(field.clone());
                    //TODO what happens when grouped fields have arguments that differ? need to check for that case and handle explictly
                }
                Selection::FragmentSpread(frag_spread) => {
                    let frag_def = frag_spread.fragment(&**snapshot).ok_or(anyhow!(
                        "fragment definition not found: {}",
                        frag_spread.name()
                    ))?;

                    let type_cond = frag_def.type_condition();
                    let type_cond_type = snapshot
                        .find_type_definition_by_name(type_cond.to_owned())
                        .ok_or(anyhow!(
                            "fragment definition type condition type not found: {}",
                            type_cond
                        ))?;

                    if fragment_type_applies(concrete_type, &type_cond_type)? {
                        inner(
                            snapshot,
                            frag_def.selection_set(),
                            concrete_type,
                            grouped_fields,
                        )?;
                    }
                }
                Selection::InlineFragment(inline_frag) => {
                    if let Some(type_cond) = inline_frag.type_condition() {
                        let type_cond_type = snapshot
                            .find_type_definition_by_name(type_cond.to_owned())
                            .ok_or(anyhow!(
                                "inline fragment type condition type not found: {}",
                                type_cond
                            ))?;

                        if fragment_type_applies(concrete_type, &type_cond_type)? {
                            inner(
                                snapshot,
                                inline_frag.selection_set(),
                                concrete_type,
                                grouped_fields,
                            )?;
                        }
                    }
                }
            };
        }

        Ok(())
    }

    let mut grouped_fields = IndexMap::new();
    inner(snapshot, sel_set, concrete_type, &mut grouped_fields)?;
    Ok(grouped_fields)
}

fn resolve_field<'a>(
    snapshot: Arc<Snapshot>,
    resolver: &'a dyn ObjectResolver,
    field: Arc<Field>,
) -> Result<Pin<Box<dyn Future<Output = Result<ConstValue>> + 'a>>> {
    Ok(Box::pin(async move {
        let resolved = resolver.resolve_field(field.name()).await?;
        resolve_to_value(snapshot, field, resolved).await
    }))
}

fn resolve_to_value<'a>(
    snapshot: Arc<Snapshot>,
    field: Arc<Field>,
    resolved: Resolved,
) -> Pin<Box<dyn Future<Output = Result<ConstValue>> + 'a>> {
    use hir::TypeDefinition::*;

    let snapshot = snapshot.clone();
    Box::pin(async move {
        let field_ty = field
            .ty(&**snapshot)
            .ok_or(anyhow!("field type not found"))?;
        let field_type_def = field_ty
            .type_def(&**snapshot)
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

                let obj_fut = SelectionSetFuture::new(
                    snapshot.clone(),
                    obj_resolver.as_ref(),
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

                let vals: Vec<ConstValue> = futs.try_collect().await?;

                Ok(ConstValue::List(vals))
            }
        }
    })
}

// pub struct FieldFuture<'r> {
//     // snapshot: Arc<Snapshot>,
//     // resolvers: &'r ResolverRegistry,
//     // resolver: &'r dyn ObjectResolver,
//     // field: &'r Field,
//     inner: InnerFieldFut<'r>,
// }

// enum InnerFieldFut<'a> {
//     Resolver(Pin<Box<dyn Future<Output = Result<Resolved>> + 'a>>),
//     SelectionSet(Pin<Box<SelectionSetFuture<'a>>>),
// }

// impl<'a> FieldFuture<'a> {
//     pub fn new(
//         snapshot: Arc<Snapshot>,
//         resolvers: &'a ResolverRegistry,
//         resolver: &'a dyn ObjectResolver,
//         field: &'a Field,
//     ) -> Result<Self> {
//         use hir::TypeDefinition::*;
//         let field_ty = field
//             .ty(&**snapshot)
//             .ok_or(anyhow!("field type not found"))?;
//         let field_type_def = field_ty
//             .type_def(&**snapshot)
//             .ok_or(anyhow!("field type definition not found"))?;

//         let field_name = field.name();
//         let inner = match field_type_def {
//             ScalarTypeDefinition(_scalar_ty) => {
//                 InnerFieldFut::Resolver(resolver.resolve_field(field_name))
//             }
//             ObjectTypeDefinition(object_ty) => {
//                 InnerFieldFut::SelectionSet(SelectionSetFuture::new(
//                     snapshot.clone(),
//                     resolvers,
//                     object_ty,
//                     field.selection_set(),
//                 )?)
//             }
//             InterfaceTypeDefinition(_iface_ty) => {
//                 let iface_value = resolve_iface_field(
//                     field_name,
//                     field.selection_set(),
//                     snapshot,
//                     resolver,
//                     resolvers,
//                 );

//                 InnerFieldFut::Resolver(iface_value)
//             }
//             UnionTypeDefinition(_) => todo!(),
//             EnumTypeDefinition(_) => todo!(),
//             InputObjectTypeDefinition(_) => todo!(),
//         };

//         Ok(Self { inner })
//     }
// }

// impl<'a> Future for FieldFuture<'a> {
//     type Output = Result<ConstValue>;

//     fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
//         match &mut self.as_mut().inner {
//             InnerFieldFut::Resolver(f) => f.as_mut().poll(cx),
//             InnerFieldFut::SelectionSet(f) => f.as_mut().poll(cx),
//         }
//     }
// }

// fn resolve_iface_field<'a>(
//     field_name: &'a str,
//     field_sels: &'a SelectionSet,
//     snapshot: Arc<Snapshot>,
//     resolver: &'a dyn ObjectResolver,
//     resolvers: &'a ResolverRegistry,
// ) -> Pin<Box<dyn Future<Output = Result<Resolved>> + 'a>> {
//     Box::pin(async move {
//         let field_type = resolver.resolve_field_type(field_name).await?;

//         let object_ty = snapshot
//             .find_object_type_by_name(field_type)
//             .ok_or(anyhow!("concrete object type not found"))?;

//         let sel_fut = SelectionSetFuture::new(snapshot.clone(), resolvers, object_ty, field_sels)?;

//         let sel_fut_value = sel_fut.await?;

//         Ok(sel_fut_value.into())
//     })
// }

/*
let concrete_ty_fut = resolver
    .resolve_field_type(field_name)
    .map(move |cty| {
        let snapshot = snapshot.clone();
        cty.and_then(move |cty| {
            snapshot
                .clone()
                .find_object_type_by_name(cty)
                .ok_or(anyhow!("concrete type not found"))
        })
    })
    .and_then(move |object_ty| {
        SelectionSetFuture::new(
            snapshot.clone(),
            resolvers,
            object_ty,
            field.selection_set(),
        )
        .unwrap()
    });

InnerFieldFut::Resolver(Box::pin(concrete_ty_fut))*/
