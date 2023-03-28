use crate::Name;
use anyhow::{anyhow, Result};
use apollo_compiler::{
    hir::{self, Directive, Field, ObjectTypeDefinition, Selection, SelectionSet, TypeDefinition},
    HirDatabase, Snapshot,
};
use indexmap::IndexMap;
use std::sync::Arc;

/// Collects a selection set's fields and fragments into a flattened represention to
/// ensure resolvers are not invoked more than once for a given field.
///
/// FIXME track visitedFragments according to spec
///
/// https://spec.graphql.org/draft/#sec-Field-Collection
pub fn collect_fields(
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
            .find(|ii| ii.interface() == obj_iface_type.name())
            .is_some()),
        TypeDefinition::UnionTypeDefinition(union_type) => Ok(union_type
            .self_members()
            .iter()
            .find(|ut| ut.name() == obj_type.name())
            .is_some()),
        invalid @ _ => Err(anyhow!(
            "invalid type in fragment type condition {}",
            invalid.name()
        )),
    }
}
