use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use alder_ast::{
    Annotation, ConstructorRef, Interface, InterfaceEnum, LocalId, LocalName, ModuleId, Namespace,
    PackageId, QualifiedName, Type, UseId, ValueRef, Variant, VariantPayload,
};
use alder_region::{Located, Region};
use bumpalo::Bump;

use crate::{Error, ErrorKind, NameError};

#[derive(Clone, Debug)]
pub enum Candidate<'a, T> {
    Unique(T),
    Ambiguous(Vec<QualifiedName<'a>>),
    Private { owner: ModuleId<'a> },
}

#[derive(Clone, Copy, Debug)]
pub struct ValueBinding<'a> {
    pub reference: ValueRef<'a>,
    pub region: Region,
    pub mutable: bool,
    pub annotation: Option<&'a Annotation<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct TypeBinding<'a> {
    pub reference: QualifiedName<'a>,
    pub arity: usize,
    pub region: Region,
}

#[derive(Clone, Copy, Debug)]
pub struct EnumBinding<'a> {
    pub reference: QualifiedName<'a>,
    pub variants: &'a [ConstructorRef<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct TraitBinding<'a> {
    pub reference: QualifiedName<'a>,
    pub arity: usize,
    pub region: Region,
}

#[derive(Clone, Copy, Debug)]
pub struct ModuleBinding<'a> {
    pub module: ModuleId<'a>,
    pub interface: Option<&'a Interface<'a>>,
    pub region: Region,
}

#[derive(Clone, Debug, Default)]
pub struct Scope<'a> {
    pub values: BTreeMap<&'a str, ValueBinding<'a>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ControlContext {
    pub function_depth: u16,
    pub loop_depth: u16,
    pub match_depth: u16,
    pub query_depth: u16,
    pub opaque_names_depth: u16,
    pub task_return: bool,
}

#[derive(Clone, Debug)]
pub struct Env<'a> {
    pub home: ModuleId<'a>,
    pub scopes: Vec<Scope<'a>>,
    pub types: BTreeMap<&'a str, Candidate<'a, TypeBinding<'a>>>,
    pub enums: BTreeMap<&'a str, Candidate<'a, EnumBinding<'a>>>,
    pub traits: BTreeMap<&'a str, Candidate<'a, TraitBinding<'a>>>,
    pub modules: BTreeMap<&'a str, Candidate<'a, ModuleBinding<'a>>>,
    pub providers: Vec<BTreeMap<&'a str, QualifiedName<'a>>>,
    pub control: ControlContext,
    next_local: Rc<Cell<u32>>,
    next_use: Rc<Cell<u32>>,
}

impl<'a> Env<'a> {
    pub fn new(home: ModuleId<'a>) -> Self {
        let mut env = Self {
            home,
            scopes: vec![Scope::default()],
            types: BTreeMap::new(),
            enums: BTreeMap::new(),
            traits: BTreeMap::new(),
            modules: BTreeMap::new(),
            providers: Vec::new(),
            control: ControlContext::default(),
            next_local: Rc::new(Cell::new(0)),
            next_use: Rc::new(Cell::new(0)),
        };
        env.add_builtin_types();
        env.add_builtin_modules();
        env
    }

    fn add_builtin_types(&mut self) {
        for (name, arity) in [
            ("Number", 0),
            ("BigInt", 0),
            ("String", 0),
            ("Bool", 0),
            ("Array", 1),
            ("Map", 2),
            ("Set", 1),
            ("Task", 1),
            ("Option", 1),
            ("Result", 2),
            ("Html", 0),
            ("Style", 0),
            ("Query", 1),
        ] {
            self.types.insert(
                name,
                Candidate::Unique(TypeBinding {
                    reference: QualifiedName {
                        module: ModuleId {
                            package: PackageId::Builtin,
                            path: &[],
                        },
                        name,
                    },
                    arity,
                    region: Region::zero(),
                }),
            );
        }
    }

    fn add_builtin_modules(&mut self) {
        for name in [
            "Array", "String", "Number", "BigInt", "Map", "Set", "Task", "Fiber", "Http", "Io",
            "Cli", "Json", "Option", "Result",
        ] {
            self.modules.insert(
                name,
                Candidate::Unique(ModuleBinding {
                    module: ModuleId {
                        package: PackageId::Builtin,
                        path: builtin_module_path(name),
                    },
                    interface: None,
                    region: Region::zero(),
                }),
            );
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    pub fn pop_scope(&mut self) {
        assert!(self.scopes.len() > 1, "cannot pop the module scope");
        self.scopes.pop();
    }

    pub fn fresh_local(&mut self, text: &'a str) -> LocalName<'a> {
        let id = self.next_local.get();
        let local = LocalName {
            id: LocalId(id),
            text,
        };
        self.next_local
            .set(id.checked_add(1).expect("local ID overflow"));
        local
    }

    pub fn fresh_use(&self) -> UseId {
        let id = self.next_use.get();
        self.next_use
            .set(id.checked_add(1).expect("use ID overflow"));
        UseId(id)
    }

    pub fn insert_local(
        &mut self,
        text: &'a str,
        region: Region,
        mutable: bool,
    ) -> Result<LocalName<'a>, Region> {
        if let Some(existing) = self.scopes.last().expect("scope exists").values.get(text) {
            return Err(existing.region);
        }
        let local = self.fresh_local(text);
        self.scopes.last_mut().expect("scope exists").values.insert(
            text,
            ValueBinding {
                reference: ValueRef::Local(local),
                region,
                mutable,
                annotation: None,
            },
        );
        Ok(local)
    }

    pub fn insert_top_level(
        &mut self,
        text: &'a str,
        region: Region,
        mutable: bool,
    ) -> Result<QualifiedName<'a>, Region> {
        if let Some(existing) = self.scopes[0].values.get(text) {
            return Err(existing.region);
        }
        let reference = QualifiedName {
            module: self.home,
            name: text,
        };
        self.scopes[0].values.insert(
            text,
            ValueBinding {
                reference: ValueRef::TopLevel(reference),
                region,
                mutable,
                annotation: None,
            },
        );
        Ok(reference)
    }

    pub fn find_value(&self, text: &str) -> Option<ValueBinding<'a>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.values.get(text).copied())
    }

    pub fn find_module(&self, text: &str) -> Option<ModuleBinding<'a>> {
        match self.modules.get(text) {
            Some(Candidate::Unique(module)) => Some(*module),
            _ => None,
        }
    }

    pub fn insert_module(
        &mut self,
        text: &'a str,
        region: Region,
        module: ModuleId<'a>,
        interface: Option<&'a Interface<'a>>,
    ) -> Result<(), Region> {
        if let Some(Candidate::Unique(existing)) = self.modules.get(text) {
            return Err(existing.region);
        }
        self.modules.insert(
            text,
            Candidate::Unique(ModuleBinding {
                module,
                interface,
                region,
            }),
        );
        Ok(())
    }

    pub fn insert_foreign_value(
        &mut self,
        text: &'a str,
        region: Region,
        reference: QualifiedName<'a>,
        annotation: &'a Annotation<'a>,
    ) -> Result<(), Region> {
        if let Some(existing) = self.scopes[0].values.get(text) {
            return Err(existing.region);
        }
        self.scopes[0].values.insert(
            text,
            ValueBinding {
                reference: ValueRef::Foreign {
                    reference,
                    annotation,
                },
                region,
                mutable: false,
                annotation: Some(annotation),
            },
        );
        Ok(())
    }

    pub fn insert_foreign_type(
        &mut self,
        text: &'a str,
        region: Region,
        reference: QualifiedName<'a>,
        arity: usize,
    ) -> Result<(), Region> {
        if let Some(Candidate::Unique(existing)) = self.types.get(text) {
            return Err(existing.region);
        }
        self.types.insert(
            text,
            Candidate::Unique(TypeBinding {
                reference,
                arity,
                region,
            }),
        );
        Ok(())
    }

    pub fn register_enum_as(
        &mut self,
        text: &'a str,
        reference: QualifiedName<'a>,
        variants: &'a [ConstructorRef<'a>],
    ) {
        self.enums.insert(
            text,
            Candidate::Unique(EnumBinding {
                reference,
                variants,
            }),
        );
    }

    pub fn insert_foreign_trait(
        &mut self,
        text: &'a str,
        region: Region,
        reference: QualifiedName<'a>,
        arity: usize,
    ) -> Result<(), Region> {
        if let Some(Candidate::Unique(existing)) = self.traits.get(text) {
            return Err(existing.region);
        }
        self.traits.insert(
            text,
            Candidate::Unique(TraitBinding {
                reference,
                arity,
                region,
            }),
        );
        Ok(())
    }

    pub fn type_binding(&self, text: &str) -> Option<TypeBinding<'a>> {
        match self.types.get(text) {
            Some(Candidate::Unique(binding)) => Some(*binding),
            _ => None,
        }
    }

    pub fn find_provider(&self, text: &str) -> Option<QualifiedName<'a>> {
        self.providers
            .iter()
            .rev()
            .find_map(|scope| scope.get(text).copied())
    }

    pub fn find_constructor(
        &self,
        bump: &'a Bump,
        region: Region,
        segments: &'a [alder_source::Name<'a>],
        allow_unqualified: bool,
    ) -> Result<ConstructorRef<'a>, Error<'a>> {
        let variant = segments.last().expect("source paths are nonempty");
        if segments.len() >= 3
            && let Some(module) = self.find_module(segments[0].value)
            && let Some(interface) = module.interface
            && let Some(enum_) = interface
                .enums
                .iter()
                .find(|enum_| enum_.exported_as == segments[segments.len() - 2].value)
            && let Some(found) = enum_
                .variants
                .iter()
                .find(|candidate| candidate.name.variant == variant.value)
        {
            return Ok(ConstructorRef {
                name: found.name,
                index: found.index,
                alternatives: found.alternatives,
                payload: found.payload,
                annotation: interface_constructor_annotation(bump, enum_, *found),
            });
        }
        if segments.len() >= 2 {
            let enum_name = segments[segments.len() - 2].value;
            if let Some(Candidate::Unique(binding)) = self.enums.get(enum_name)
                && let Some(constructor) = binding
                    .variants
                    .iter()
                    .find(|constructor| constructor.name.variant == variant.value)
            {
                return Ok(*constructor);
            }
        } else if allow_unqualified || matches!(variant.value, "Some" | "None" | "Ok" | "Err") {
            let matches: Vec<_> = self
                .enums
                .values()
                .filter_map(|candidate| match candidate {
                    Candidate::Unique(binding) => binding
                        .variants
                        .iter()
                        .find(|constructor| constructor.name.variant == variant.value)
                        .copied(),
                    _ => None,
                })
                .collect();
            if matches.len() == 1 {
                return Ok(matches[0]);
            }
            if matches.len() > 1 {
                return Err(Error::new(
                    region,
                    ErrorKind::Pattern(crate::PatternError::Name(NameError::Ambiguous {
                        namespace: Namespace::Constructor,
                        name: variant.value,
                        candidates: bump.alloc_slice_fill_iter(matches.into_iter().map(
                            |constructor| QualifiedName {
                                module: constructor.name.enum_.module,
                                name: constructor.name.variant,
                            },
                        )),
                    })),
                ));
            }
        }
        Err(self.unknown_name(
            bump,
            region,
            Namespace::Constructor,
            (segments.len() >= 2).then(|| segments[segments.len() - 2].value),
            variant.value,
            self.enums.values().flat_map(|candidate| match candidate {
                Candidate::Unique(binding) => binding
                    .variants
                    .iter()
                    .map(|constructor| constructor.name.variant)
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            }),
        ))
    }

    pub fn insert_type(
        &mut self,
        text: &'a str,
        region: Region,
        arity: usize,
    ) -> Result<QualifiedName<'a>, Region> {
        if let Some(Candidate::Unique(existing)) = self.types.get(text) {
            return Err(existing.region);
        }
        if self.enums.contains_key(text) || self.traits.contains_key(text) {
            return Err(region);
        }
        let reference = QualifiedName {
            module: self.home,
            name: text,
        };
        self.types.insert(
            text,
            Candidate::Unique(TypeBinding {
                reference,
                arity,
                region,
            }),
        );
        Ok(reference)
    }

    pub fn register_enum(
        &mut self,
        reference: QualifiedName<'a>,
        variants: &'a [ConstructorRef<'a>],
    ) {
        self.enums.insert(
            reference.name,
            Candidate::Unique(EnumBinding {
                reference,
                variants,
            }),
        );
    }

    pub fn insert_trait(
        &mut self,
        text: &'a str,
        region: Region,
        arity: usize,
    ) -> Result<QualifiedName<'a>, Region> {
        if let Some(Candidate::Unique(existing)) = self.traits.get(text) {
            return Err(existing.region);
        }
        if let Some(Candidate::Unique(existing)) = self.types.get(text) {
            return Err(existing.region);
        }
        let reference = QualifiedName {
            module: self.home,
            name: text,
        };
        self.traits.insert(
            text,
            Candidate::Unique(TraitBinding {
                reference,
                arity,
                region,
            }),
        );
        Ok(reference)
    }

    pub fn find_trait(
        &self,
        bump: &'a Bump,
        region: Region,
        qualifier: Option<&'a str>,
        name: &'a str,
    ) -> Result<TraitBinding<'a>, Error<'a>> {
        if let Some(qualifier) = qualifier {
            if let Some(module) = self.find_module(qualifier)
                && let Some(interface) = module.interface
                && let Some(trait_) = interface
                    .traits
                    .iter()
                    .find(|trait_| trait_.exported_as == name)
            {
                return Ok(TraitBinding {
                    reference: trait_.reference,
                    arity: trait_.params.len(),
                    region,
                });
            }
            return Err(self.unknown_name(
                bump,
                region,
                Namespace::Trait,
                Some(qualifier),
                name,
                self.traits.keys().copied(),
            ));
        }
        match self.traits.get(name) {
            Some(Candidate::Unique(binding)) => Ok(*binding),
            Some(Candidate::Ambiguous(candidates)) => Err(Error::new(
                region,
                ErrorKind::Type(crate::TypeError::Name(NameError::Ambiguous {
                    namespace: Namespace::Trait,
                    name,
                    candidates: bump.alloc_slice_copy(candidates),
                })),
            )),
            Some(Candidate::Private { owner }) => Err(Error::new(
                region,
                ErrorKind::Type(crate::TypeError::Name(NameError::Private {
                    owner: *owner,
                    namespace: Namespace::Trait,
                    name,
                })),
            )),
            None => Err(self.unknown_name(
                bump,
                region,
                Namespace::Trait,
                None,
                name,
                self.traits.keys().copied(),
            )),
        }
    }

    pub fn find_type(
        &self,
        bump: &'a Bump,
        region: Region,
        qualifier: Option<&'a str>,
        name: &'a str,
    ) -> Result<TypeBinding<'a>, Error<'a>> {
        if let Some(qualifier) = qualifier {
            if let Some(module) = self.find_module(qualifier)
                && let Some(interface) = module.interface
            {
                if let Some(typ) = interface.types.iter().find(|typ| typ.exported_as == name) {
                    return Ok(TypeBinding {
                        reference: typ.reference,
                        arity: typ.params.len(),
                        region,
                    });
                }
                if let Some(enum_) = interface
                    .enums
                    .iter()
                    .find(|enum_| enum_.exported_as == name)
                {
                    return Ok(TypeBinding {
                        reference: enum_.reference,
                        arity: enum_.params.len(),
                        region,
                    });
                }
            }
            return Err(self.unknown_name(
                bump,
                region,
                Namespace::Type,
                Some(qualifier),
                name,
                self.types.keys().copied(),
            ));
        }
        match self.types.get(name) {
            Some(Candidate::Unique(binding)) => Ok(*binding),
            Some(Candidate::Ambiguous(candidates)) => Err(Error::new(
                region,
                ErrorKind::Type(crate::TypeError::Name(NameError::Ambiguous {
                    namespace: Namespace::Type,
                    name,
                    candidates: bump.alloc_slice_copy(candidates),
                })),
            )),
            Some(Candidate::Private { owner }) => Err(Error::new(
                region,
                ErrorKind::Type(crate::TypeError::Name(NameError::Private {
                    owner: *owner,
                    namespace: Namespace::Type,
                    name,
                })),
            )),
            None => Err(self.unknown_name(
                bump,
                region,
                Namespace::Type,
                None,
                name,
                self.types.keys().copied(),
            )),
        }
    }

    fn unknown_name(
        &self,
        bump: &'a Bump,
        region: Region,
        namespace: Namespace,
        qualifier: Option<&'a str>,
        name: &'a str,
        available: impl Iterator<Item = &'a str>,
    ) -> Error<'a> {
        let suggestions = suggestions(bump, name, available);
        let name_error = NameError::Unknown {
            namespace,
            qualifier,
            name,
            suggestions,
        };
        let kind = match namespace {
            Namespace::Type | Namespace::Enum | Namespace::Trait => {
                ErrorKind::Type(crate::TypeError::Name(name_error))
            }
            Namespace::Value | Namespace::Module | Namespace::Provider => {
                ErrorKind::Expr(crate::ExprError::Name(name_error))
            }
            Namespace::Constructor => ErrorKind::Pattern(crate::PatternError::Name(name_error)),
            Namespace::AssociatedItem => ErrorKind::Expr(crate::ExprError::Name(name_error)),
        };
        Error::new(region, kind)
    }
}

fn interface_constructor_annotation<'a>(
    bump: &'a Bump,
    enum_: &'a InterfaceEnum<'a>,
    variant: Variant<'a>,
) -> &'a Annotation<'a> {
    let args = bump.alloc_slice_fill_iter(enum_.params.iter().map(|param| {
        bump.alloc(Located::at(
            Region::zero(),
            Type::Var {
                name: param,
                args: &[],
            },
        )) as &Located<Type<'a>>
    }));
    let result = bump.alloc(Located::at(
        Region::zero(),
        Type::Named {
            reference: enum_.reference,
            args,
        },
    ));
    let params = match variant.payload {
        VariantPayload::Unit => &[] as &[&Located<Type<'a>>],
        VariantPayload::Tuple(types) => types,
        VariantPayload::Record(fields) => {
            bump.alloc_slice_fill_iter(fields.iter().map(|field| field.typ))
        }
    };
    let typ = if params.is_empty() {
        result
    } else {
        bump.alloc(Located::at(
            Region::zero(),
            Type::Fn {
                params,
                ret: result,
            },
        ))
    };
    bump.alloc(Annotation {
        free_vars: enum_.params,
        typ,
    })
}

fn builtin_module_path(name: &str) -> &'static [&'static str] {
    match name {
        "Array" => &["Array"],
        "String" => &["String"],
        "Number" => &["Number"],
        "BigInt" => &["BigInt"],
        "Map" => &["Map"],
        "Set" => &["Set"],
        "Task" => &["Task"],
        "Fiber" => &["Fiber"],
        "Http" => &["Http"],
        "Io" => &["Io"],
        "Cli" => &["Cli"],
        "Json" => &["Json"],
        "Option" => &["Option"],
        "Result" => &["Result"],
        _ => unreachable!("all builtin module names are listed"),
    }
}

fn suggestions<'a>(
    bump: &'a Bump,
    needle: &str,
    available: impl Iterator<Item = &'a str>,
) -> &'a [&'a str] {
    let mut ranked: Vec<(&'a str, usize)> = available
        .map(|candidate| (candidate, edit_distance(needle, candidate)))
        .filter(|(candidate, distance)| {
            candidate.starts_with(needle)
                || needle.starts_with(*candidate)
                || *distance <= suggestion_limit(needle.len())
        })
        .collect();
    ranked.sort_by(|(left_name, left_distance), (right_name, right_distance)| {
        left_distance
            .cmp(right_distance)
            .then_with(|| left_name.cmp(right_name))
    });
    bump.alloc_slice_fill_iter(ranked.into_iter().take(4).map(|(name, _)| name))
}

fn suggestion_limit(length: usize) -> usize {
    match length {
        0..=3 => 1,
        4..=7 => 2,
        _ => 3,
    }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let mut previous: Vec<usize> = (0..=right.chars().count()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(previous.len());
        current.push(left_index + 1);
        for (right_index, right_char) in right.chars().enumerate() {
            let insert = current[right_index] + 1;
            let delete = previous[right_index + 1] + 1;
            let replace = previous[right_index] + usize::from(left_char != right_char);
            current.push(insert.min(delete).min(replace));
        }
        previous = current;
    }
    previous[right.chars().count()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_are_ranked_and_capped() {
        let bump = Bump::new();
        let result = suggestions(
            &bump,
            "Nubmer",
            ["String", "Numeric", "Number", "Name", "Never"].into_iter(),
        );
        assert_eq!(result, &["Number"]);
    }

    #[test]
    fn local_ids_are_stable_and_unique() {
        let mut env = Env::new(ModuleId {
            package: PackageId::Application,
            path: &[],
        });
        let x = env.insert_local("x", Region::one(), false).unwrap();
        env.push_scope();
        let inner_x = env.insert_local("x", Region::zero(), true).unwrap();
        assert_ne!(x.id, inner_x.id);
        assert!(env.find_value("x").unwrap().mutable);
    }

    #[test]
    fn cloned_environments_share_id_allocators() {
        let mut env = Env::new(ModuleId {
            package: PackageId::Application,
            path: &[],
        });
        let mut branch = env.clone();

        assert_eq!(env.fresh_use(), UseId(0));
        assert_eq!(branch.fresh_use(), UseId(1));
        assert_eq!(env.fresh_local("left").id, LocalId(0));
        assert_eq!(branch.fresh_local("right").id, LocalId(1));
    }
}
