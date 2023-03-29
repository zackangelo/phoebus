//! Resolver implementations that augment other resolvers with
//! introspection fields

use crate::{
    resolver::{ObjectResolver, Resolved},
    value::ConstValue,
};
use anyhow::anyhow;
use anyhow::Result;
use apollo_compiler::hir::{self, InputValueDefinition, ObjectTypeDefinition, TypeSystem};
use async_trait::async_trait;
use std::sync::Arc;

/// ObjectResolver that adds __typename introspection to another resolver
pub struct IspObjectResolver<'a> {
    pub(crate) type_def: Arc<ObjectTypeDefinition>, //TODO probably use reference instead
    pub(crate) inner: &'a dyn ObjectResolver,
}

#[async_trait]
impl<'a> ObjectResolver for IspObjectResolver<'a> {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        match name {
            "__typename" => Ok(Resolved::Value(ConstValue::String(
                self.type_def.name().to_owned(),
            ))),
            other => self.inner.resolve_field(other).await,
        }
    }
}

/// ObjectResolver intended to be added to a query root to expose schema
/// introspection fields
pub struct IspRootResolver<'a> {
    pub(crate) ts: Arc<hir::TypeSystem>,
    pub(crate) inner: &'a dyn ObjectResolver,
}

#[async_trait]
impl<'a> ObjectResolver for IspRootResolver<'a> {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        match name {
            "__schema" => {
                let resolver = IspSchemaResolver {
                    ts: self.ts.clone(),
                };
                Ok(Resolved::object(resolver))
            }
            other => self.inner.resolve_field(other).await,
        }
    }
}

/*
type __Schema {
  description: String
  types: [__Type!]!
  queryType: __Type!
  mutationType: __Type
  subscriptionType: __Type
  directives: [__Directive!]!
}
*/
pub struct IspSchemaResolver {
    pub(crate) ts: Arc<TypeSystem>,
}

#[async_trait]
impl ObjectResolver for IspSchemaResolver {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        match name {
            "description" => todo!(),
            "types" => {
                let all_type_defs = self
                    .ts
                    .type_definitions_by_name
                    .values()
                    .filter(|ty| !ty.name().starts_with("__")) //TODO there should be a more reliable check somewhere for excluding introspection types
                    .map(|ty| {
                        Resolved::object(IspTypeResolver {
                            ty: hir::Type::Named {
                                name: ty.name().to_owned(),
                                loc: Some(ty.loc()),
                            },
                            ts: self.ts.clone(),
                        })
                    }) //TODO make reference work?
                    .collect::<Vec<_>>();

                Ok(Resolved::Array(all_type_defs))
            }
            "queryType" => todo!(),
            "mutationType" => todo!(),
            "subscriptionType" => todo!(),
            "directives" => todo!(),
            invalid => Err(anyhow!("invalid type field: {}", invalid)),
        }
    }
}

/*
type __Type {
  kind: __TypeKind!
  name: String
  description: String
  # must be non-null for OBJECT and INTERFACE, otherwise null.
  fields(includeDeprecated: Boolean = false): [__Field!]
  # must be non-null for OBJECT and INTERFACE, otherwise null.
  interfaces: [__Type!]
  # must be non-null for INTERFACE and UNION, otherwise null.
  possibleTypes: [__Type!]
  # must be non-null for ENUM, otherwise null.
  enumValues(includeDeprecated: Boolean = false): [__EnumValue!]
  # must be non-null for INPUT_OBJECT, otherwise null.
  inputFields(includeDeprecated: Boolean = false): [__InputValue!]
  # must be non-null for NON_NULL and LIST, otherwise null.
  ofType: __Type
  # may be non-null for custom SCALAR, otherwise null.
  specifiedByURL: String
}
*/
pub struct IspTypeResolver {
    pub(crate) ts: Arc<hir::TypeSystem>,
    pub(crate) ty: hir::Type,
}

impl IspTypeResolver {
    async fn resolve_list_type(&self, field: &str, of_type: &hir::Type) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("LIST")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())), //: String
            "description" => Ok(Resolved::string("")), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__Field!]
            "interfaces" => Ok(Resolved::null()), //: [__Type!]
            "possibleTypes" => Ok(Resolved::null()), //: [__Type!]
            "enumValues" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::object(IspTypeResolver {
                ty: of_type.clone(),
                ts: self.ts.clone(),
            })), //: __Type
            "specifiedByURL" => Ok(Resolved::null()), //: String TODO - not sure where to get this
            _ => Err(anyhow!("invalid list type field")),
        }
    }

    async fn resolve_non_null_type(&self, field: &str, of_type: &hir::Type) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("NON_NULL")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())), //: String
            "description" => Ok(Resolved::null()), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__Field!]
            "interfaces" => Ok(Resolved::null()), //: [__Type!]
            "possibleTypes" => Ok(Resolved::null()), //: [__Type!]
            "enumValues" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::object(IspTypeResolver {
                ty: of_type.clone(),
                ts: self.ts.clone(),
            })), //: __Type
            "specifiedByURL" => Ok(Resolved::null()), //: String TODO - not sure where to get this
            _ => Err(anyhow!("invalid list type field")),
        }
    }

    async fn resolve_named_type(&self, field: &str, type_name: &str) -> Result<Resolved> {
        // let db = self.db.lock().await;
        let ty_def = self.ts.type_definitions_by_name.get(type_name);

        match ty_def {
            Some(ty_def) => match ty_def {
                hir::TypeDefinition::ScalarTypeDefinition(type_def) => {
                    self.resolve_scalar_type(field, type_def)
                }
                hir::TypeDefinition::ObjectTypeDefinition(type_def) => {
                    self.resolve_object_type(field, type_def)
                }
                hir::TypeDefinition::InterfaceTypeDefinition(type_def) => {
                    self.resolve_interface_type(field, type_def)
                }
                hir::TypeDefinition::UnionTypeDefinition(type_def) => {
                    self.resolve_union_type(field, type_def)
                }
                hir::TypeDefinition::EnumTypeDefinition(type_def) => {
                    self.resolve_enum_type(field, type_def)
                }
                hir::TypeDefinition::InputObjectTypeDefinition(type_def) => {
                    self.resolve_input_type(field, type_def)
                }
            },
            None => Ok(Resolved::null()),
        }
    }

    fn resolve_scalar_type(
        &self,
        field: &str,
        type_def: &hir::ScalarTypeDefinition,
    ) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("SCALAR")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())), //: String
            "description" => Ok(Resolved::string_opt(type_def.description())), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__Field!]
            "interfaces" => Ok(Resolved::null()), //: [__Type!]
            "possibleTypes" => Ok(Resolved::null()), //: [__Type!]
            "enumValues" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::null()),      //: __Type
            "specifiedByURL" => Ok(self.resolve_specified_by(type_def)),
            _ => Err(anyhow!("invalid list type field")),
        }
    }

    fn resolve_specified_by(&self, type_def: &hir::ScalarTypeDefinition) -> Resolved {
        Resolved::string_opt(
            type_def
                .directives()
                .find(|d| d.name() == "specifiedBy")
                .and_then(|d| match d.argument_by_name("url") {
                    Some(hir::Value::String(s)) => Some(s.as_str()),
                    _ => None,
                }),
        )
    }

    fn resolve_object_type(
        &self,
        field: &str,
        type_def: &hir::ObjectTypeDefinition,
    ) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("OBJECT")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())), //: String
            "description" => Ok(Resolved::string_opt(type_def.description())), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::Array(
                type_def
                    .fields()
                    .map(|f| {
                        Resolved::object(IspFieldResolver {
                            field_def: f.clone(),
                            ts: self.ts.clone(),
                        })
                    })
                    .collect(),
            )), //(includeDeprecated: Boolean = false): [__Field!]
            "interfaces" => Ok(type_def
                .implements_interfaces()
                .map(|i| IspTypeResolver {
                    ts: self.ts.clone(),
                    ty: hir::Type::Named {
                        name: i.interface().to_owned(),
                        loc: None,
                    },
                })
                .collect::<Vec<_>>()
                .into()), //: [__Type!]
            "possibleTypes" => Ok(Resolved::null()),                           //: [__Type!]
            "enumValues" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::null()),      //: __Type
            "specifiedByURL" => Ok(Resolved::null()), //: String TODO - not sure where to get this
            _ => Err(anyhow!("invalid list type field")),
        }
    }

    fn resolve_interface_type(
        &self,
        field: &str,
        type_def: &hir::InterfaceTypeDefinition,
    ) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("INTERFACE")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())),  //: String
            "description" => Ok(Resolved::string_opt(type_def.description())), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::Array(
                type_def
                    .fields()
                    .map(|f| {
                        Resolved::object(IspFieldResolver {
                            field_def: f.clone(),
                            ts: self.ts.clone(),
                        })
                    })
                    .collect(),
            )), //TODO includeDeprecated arg
            "interfaces" => Ok(Resolved::null()),                              //: [__Type!]
            "possibleTypes" => Ok(self.resolve_impl_possible_types(type_def.name())), //: [__Type!]
            "enumValues" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::null()),      //: __Type
            "specifiedByURL" => Ok(Resolved::null()), //: String TODO - not sure where to get this
            _ => Err(anyhow!("invalid list type field")),
        }
    }

    fn resolve_impl_possible_types(&self, iface_name: &str) -> Resolved {
        //nb: slow but probably fine for now, maybe index in future
        self.ts
            .definitions
            .objects
            .iter()
            .filter(|(_, ty)| ty.implements_interface(iface_name))
            .map(|(name, _ty)| IspTypeResolver {
                //TODO create an IspObjectTypeResolver directly when we refactor
                ts: self.ts.clone(),
                ty: hir::Type::Named {
                    name: name.clone(),
                    loc: None,
                },
            })
            .collect::<Vec<_>>()
            .into()
    }

    fn resolve_union_type(
        &self,
        field: &str,
        type_def: &hir::UnionTypeDefinition,
    ) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("UNION")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())), //: String
            "description" => Ok(Resolved::string_opt(type_def.description())), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__Field!]
            "interfaces" => Ok(Resolved::null()), //: [__Type!]
            "possibleTypes" => Ok(Resolved::null()), //: [__Type!]
            "enumValues" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::null()),      //: __Type
            "specifiedByURL" => Ok(Resolved::null()), //: String TODO - not sure where to get this
            _ => Err(anyhow!("invalid list type field")),
        }
    }

    fn resolve_enum_type(
        &self,
        field: &str,
        type_def: &hir::EnumTypeDefinition,
    ) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("ENUM")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())), //: String
            "description" => Ok(Resolved::string_opt(type_def.description())), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__Field!]
            "interfaces" => Ok(Resolved::null()), //: [__Type!]
            "possibleTypes" => Ok(Resolved::null()), //: [__Type!]
            "enumValues" => Ok(type_def
                .values()
                .map(|v| IspEnumValueResolver {
                    enum_value: v.clone(),
                })
                .collect::<Vec<_>>()
                .into()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::null()),      //: __Type
            "specifiedByURL" => Ok(Resolved::null()), //: String TODO - not sure where to get this
            _ => Err(anyhow!("invalid list type field")),
        }
    }

    fn resolve_input_type(
        &self,
        field: &str,
        type_def: &hir::InputObjectTypeDefinition,
    ) -> Result<Resolved> {
        match field {
            "kind" => Ok(Resolved::enum_value("INPUT_OBJECT")), //": __TypeKind!
            "name" => Ok(Resolved::string(self.ty.name())),     //: String
            "description" => Ok(Resolved::string_opt(type_def.description())), //: String -> TODO is this shared with type definition?
            "fields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__Field!]
            "interfaces" => Ok(Resolved::null()), //: [__Type!]
            "possibleTypes" => Ok(Resolved::null()), //: [__Type!]
            "enumValues" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__EnumValue!]
            "inputFields" => Ok(Resolved::null()), //(includeDeprecated: Boolean = false): [__InputValue!]
            "ofType" => Ok(Resolved::null()),      //: __Type
            "specifiedByURL" => Ok(Resolved::null()), //: String TODO - not sure where to get this
            _ => Err(anyhow!("invalid list type field")),
        }
    }
}
#[async_trait]
impl ObjectResolver for IspTypeResolver {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        //TODO this match will re-run for every field, probably pre-evaluate it in a constructor
        match &self.ty {
            hir::Type::List { ty, .. } => self.resolve_list_type(name, ty.as_ref()).await,
            hir::Type::Named { name: ty_name, .. } => self.resolve_named_type(name, ty_name).await,
            hir::Type::NonNull { ty, .. } => self.resolve_non_null_type(name, ty).await,
        }
    }
}

/*
type __Field {
  name: String!
  description: String
  args(includeDeprecated: Boolean = false): [__InputValue!]!
  type: __Type!
  isDeprecated: Boolean!
  deprecationReason: String
}
 */

pub struct IspFieldResolver {
    pub(crate) field_def: hir::FieldDefinition,
    pub(crate) ts: Arc<hir::TypeSystem>,
}

#[async_trait]
impl ObjectResolver for IspFieldResolver {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        Ok(match name {
            "name" => Resolved::string(self.field_def.name()),
            "description" => Resolved::string_opt(self.field_def.description()),
            "args" => self
                .field_def
                .arguments()
                .input_values()
                .iter()
                .map(|iv| IspInputValueResolver {
                    input_value_def: iv.clone(),
                    ts: self.ts.clone(),
                })
                .collect::<Vec<_>>()
                .into(), //Resolved::Array(vec![]), //TODO
            "type" => Resolved::object(IspTypeResolver {
                ty: self.field_def.ty().clone(),
                ts: self.ts.clone(),
            }),
            "isDeprecated" => self.field_def.resolve_is_deprecated(),
            "deprecationReason" => self.field_def.resolve_deprecation_reason(),
            _ => Resolved::null(),
        })
    }
}

pub struct IspInputValueResolver {
    ts: Arc<TypeSystem>,
    input_value_def: InputValueDefinition,
}

// type __InputValue {
//     name: String!
//     description: String
//     type: __Type!
//     defaultValue: String
//     isDeprecated: Boolean!
//     deprecationReason: String
//   }
#[async_trait]
impl ObjectResolver for IspInputValueResolver {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        Ok(match name {
            "name" => Resolved::string(self.input_value_def.name()),
            "description" => Resolved::string_opt(self.input_value_def.description()),
            "type" => resolve_ty(&self.ts, &self.input_value_def.ty()),
            "defaultValue" => Resolved::string_opt(
                self.input_value_def
                    .default_value()
                    .map(|v| format!("{:?}", v)), //TODO not sure what this represenation needs to be, debug for now
            ),
            "isDeprecated" => self.input_value_def.resolve_is_deprecated(),
            "deprecationReason" => self.input_value_def.resolve_deprecation_reason(),
            _ => Resolved::null(),
        })
    }
}

// type __EnumValue {
//     name: String!
//     description: String
//     isDeprecated: Boolean!
//     deprecationReason: String
//   }
pub struct IspEnumValueResolver {
    enum_value: hir::EnumValueDefinition,
}

#[async_trait]
impl ObjectResolver for IspEnumValueResolver {
    async fn resolve_field(&self, name: &str) -> Result<Resolved> {
        Ok(match name {
            "name" => Resolved::string(self.enum_value.enum_value()),
            "description" => Resolved::string_opt(self.enum_value.description()),
            "isDeprecated" => self.enum_value.resolve_is_deprecated(),
            "deprecationReason" => self.enum_value.resolve_deprecation_reason(),
            _ => Resolved::null(),
        })
    }
}

fn resolve_ty(ts: &Arc<TypeSystem>, ty: &hir::Type) -> Resolved {
    Resolved::object(IspTypeResolver {
        ty: ty.clone(),
        ts: ts.clone(),
    })
}

trait IspDirectives {
    fn directives(&self) -> &[hir::Directive];

    fn deprecated_directive(&self) -> Option<&hir::Directive> {
        self.directives().iter().find(|d| d.name() == "deprecated")
    }

    fn is_deprecated(&self) -> bool {
        self.deprecated_directive().is_some()
    }

    fn resolve_is_deprecated(&self) -> Resolved {
        Resolved::Value(self.is_deprecated().into())
    }

    fn deprecation_reason(&self) -> Option<&str> {
        self.deprecated_directive()
            .and_then(|d| match d.argument_by_name("reason") {
                Some(hir::Value::String(s)) => Some(s.as_str()),
                _ => None,
            })
    }

    fn resolve_deprecation_reason(&self) -> Resolved {
        Resolved::string_opt(self.deprecation_reason())
    }
}

macro_rules! directives_impl {
    ($ty: ty) => {
        impl IspDirectives for $ty {
            fn directives(&self) -> &[hir::Directive] {
                self.directives()
            }
        }
    };
}

directives_impl!(hir::FieldDefinition);
directives_impl!(hir::InputValueDefinition);
directives_impl!(hir::EnumValueDefinition);

/*
enum __TypeKind {
  SCALAR
  OBJECT
  INTERFACE
  UNION
  ENUM
  INPUT_OBJECT
  LIST
  NON_NULL
}

type __InputValue {
  name: String!
  description: String
  type: __Type!
  defaultValue: String
  isDeprecated: Boolean!
  deprecationReason: String
}



type __Directive {
  name: String!
  description: String
  locations: [__DirectiveLocation!]!
  args(includeDeprecated: Boolean = false): [__InputValue!]!
  isRepeatable: Boolean!
}

enum __DirectiveLocation {
  QUERY
  MUTATION
  SUBSCRIPTION
  FIELD
  FRAGMENT_DEFINITION
  FRAGMENT_SPREAD
  INLINE_FRAGMENT
  VARIABLE_DEFINITION
  SCHEMA
  SCALAR
  OBJECT
  FIELD_DEFINITION
  ARGUMENT_DEFINITION
  INTERFACE
  UNION
  ENUM
  ENUM_VALUE
  INPUT_OBJECT
  INPUT_FIELD_DEFINITION
}
*/
