pub mod dups;
pub mod foreign;
pub mod local;

use std::collections::BTreeMap;

use alder_ast::{Associativity, CtorOpts, ModuleName, Precedence, Type as CanType};
use alder_region::{Located, Region};
use bumpalo::Bump;

use crate::Error;

/// A resolved name: either uniquely identified or ambiguous across modules.
#[derive(Clone, Debug)]
pub enum Info<'a, T> {
    Specific(ModuleName<'a>, T),
    Ambiguous(ModuleName<'a>, Vec<ModuleName<'a>>),
}

/// Unqualified name -> resolution.
pub type Exposed<'a, T> = BTreeMap<&'a str, Info<'a, T>>;

/// Module prefix -> name -> resolution (for `Module.name` lookups).
pub type Qualified<'a, T> = BTreeMap<&'a str, BTreeMap<&'a str, Info<'a, T>>>;

/// A value variable in scope.
#[derive(Clone, Debug)]
pub enum Var<'a> {
    Local(Region),
    TopLevel(Region),
    /// Imported from another module, like Elm's `Foreign home annotation`.
    /// The annotation comes from the defining module's (post-solve)
    /// interface.
    Foreign(ModuleName<'a>, &'a alder_ast::Annotation<'a>),
    /// Ambiguous import: same name imported from multiple modules.
    Foreigns(ModuleName<'a>, Vec<ModuleName<'a>>),
}

/// A type in scope (alias or union).
#[derive(Clone, Copy, Debug)]
pub enum Type<'a> {
    Alias {
        arity: usize,
        home: ModuleName<'a>,
        parameters: &'a [&'a str],
        typ: &'a Located<CanType<'a>>,
    },
    Union {
        arity: usize,
        home: ModuleName<'a>,
    },
}

/// A data constructor in scope.
#[derive(Clone, Copy, Debug)]
pub enum Ctor<'a> {
    /// Union constructor (e.g., `Just` from `type Maybe a = Just a | Nothing`).
    Union {
        home: ModuleName<'a>,
        type_name: &'a str,
        type_vars: &'a [&'a str],
        union: &'a alder_ast::Union<'a>,
        index: u16,
        arity: u16,
        arguments: &'a [&'a Located<CanType<'a>>],
        options: CtorOpts,
        alternatives: u16,
    },
    /// Built-in Bool constructor (True or False from Basics).
    /// Separated from `Union` so pattern/expression canonicalization can
    /// emit `CanPattern::Bool` / synthesize the annotation without string checks.
    Bool {
        home: ModuleName<'a>,
        union: &'a alder_ast::Union<'a>,
        index: u16,
    },
    /// Record alias ctor (e.g., `Point` from `type alias Point = { x : Int, y : Int }`).
    /// Elm creates these automatically for non-extensible record aliases.
    /// Like Elm's `Env.RecordCtor home vars tipe`, the complete curried
    /// function type is built up front.
    RecordCtor {
        home: ModuleName<'a>,
        alias_name: &'a str,
        type_vars: &'a [&'a str],
        typ: &'a Located<CanType<'a>>,
    },
}

/// Build the curried constructor type for a record alias, mirroring Elm's
/// `toRecordCtor`: `field1 -> field2 -> ... -> Alias args (Filled record)`,
/// with fields in source order (`fieldsToList` sorts by index).
pub fn make_record_ctor<'a>(
    bump: &'a Bump,
    home: ModuleName<'a>,
    alias_name: &'a str,
    parameters: &'a [&'a str],
    record_type: &'a Located<CanType<'a>>,
    fields: &'a [alder_ast::FieldType<'a>],
) -> Ctor<'a> {
    let arguments =
        bump.alloc_slice_fill_iter(parameters.iter().map(|var| alder_ast::AliasArgument {
            name: var,
            typ: bump.alloc(Located::at(Region::zero(), CanType::Var(var))),
        }));
    let result: &'a Located<CanType<'a>> = bump.alloc(Located::at(
        Region::zero(),
        CanType::Alias {
            reference: alder_ast::QualifiedName {
                home,
                name: alias_name,
            },
            arguments,
            target: alder_ast::AliasType::Filled(record_type),
        },
    ));

    let mut fields_in_source_order: Vec<&alder_ast::FieldType<'a>> = fields.iter().collect();
    fields_in_source_order.sort_by_key(|f| f.index);

    let mut typ = result;
    for field in fields_in_source_order.into_iter().rev() {
        typ = bump.alloc(Located::at(
            Region::zero(),
            CanType::Lambda {
                from: field.typ,
                to: typ,
            },
        ));
    }

    Ctor::RecordCtor {
        home,
        alias_name,
        type_vars: parameters,
        typ,
    }
}

/// A binary operator in scope. Mirrors Elm's
/// `Env.Binop op home name annotation associativity precedence`.
///
/// Like Elm, only IMPORTED operators are in scope: the defining module's
/// own `infix` declarations do not enter its env (their annotations only
/// exist once that module has been solved), so a module calls the
/// operator's underlying function directly, as Elm core modules do.
#[derive(Clone, Copy, Debug)]
pub struct Binop<'a> {
    pub symbol: &'a str,
    pub home: ModuleName<'a>,
    pub function: &'a str,
    pub annotation: &'a alder_ast::Annotation<'a>,
    pub associativity: Associativity,
    pub precedence: Precedence,
}

/// The canonicalization environment.
///
/// Built from imports (foreign) then augmented with local definitions.
/// Consumed by type, pattern, and expression canonicalization.
#[derive(Clone)]
pub struct Env<'a> {
    pub home: ModuleName<'a>,
    pub vars: BTreeMap<&'a str, Var<'a>>,
    pub types: Exposed<'a, Type<'a>>,
    pub ctors: Exposed<'a, Ctor<'a>>,
    pub binops: Exposed<'a, Binop<'a>>,
    /// Qualified value lookups carry the imported value's annotation,
    /// like Elm's `_q_vars :: Qualified Can.Annotation`.
    pub q_vars: Qualified<'a, &'a alder_ast::Annotation<'a>>,
    pub q_types: Qualified<'a, Type<'a>>,
    pub q_ctors: Qualified<'a, Ctor<'a>>,
}

impl<'a> Env<'a> {
    /// Insert into both the unqualified and self-qualified tables.
    /// Used by local.rs — local definitions overwrite imported ones.
    pub fn insert_local_type(&mut self, name: &'a str, typ: Type<'a>) {
        self.types.insert(name, Info::Specific(self.home, typ));
        self.q_types
            .entry(self.home.name)
            .or_default()
            .insert(name, Info::Specific(self.home, typ));
    }

    /// Insert into both the unqualified and self-qualified ctor tables.
    pub fn insert_local_ctor(&mut self, name: &'a str, ctor: Ctor<'a>) {
        self.ctors.insert(name, Info::Specific(self.home, ctor));
        self.q_ctors
            .entry(self.home.name)
            .or_default()
            .insert(name, Info::Specific(self.home, ctor));
    }

    /// Look up an unqualified constructor. Mirrors Elm's `Env.findCtor`.
    pub fn find_ctor(
        &self,
        bump: &'a Bump,
        region: Region,
        name: &'a str,
    ) -> Result<Ctor<'a>, Vec<Error<'a>>> {
        match self.ctors.get(name) {
            Some(Info::Specific(_, ctor)) => Ok(*ctor),
            Some(Info::Ambiguous(first, others)) => Err(vec![Error::AmbiguousCtor {
                region,
                prefix: None,
                name,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }]),
            None => Err(vec![Error::NotFoundCtor {
                region,
                prefix: None,
                name,
                suggestions: self.possible_ctor_names(bump),
            }]),
        }
    }

    /// Look up a qualified constructor. Mirrors Elm's `Env.findCtorQual`.
    pub fn find_ctor_qual(
        &self,
        bump: &'a Bump,
        region: Region,
        prefix: &'a str,
        name: &'a str,
    ) -> Result<Ctor<'a>, Vec<Error<'a>>> {
        let info = self
            .q_ctors
            .get(prefix)
            .and_then(|m| m.get(name))
            .ok_or_else(|| {
                vec![Error::NotFoundCtor {
                    region,
                    prefix: Some(prefix),
                    name,
                    suggestions: self.possible_ctor_names(bump),
                }]
            })?;
        match info {
            Info::Specific(_, ctor) => Ok(*ctor),
            Info::Ambiguous(first, others) => Err(vec![Error::AmbiguousCtor {
                region,
                prefix: Some(prefix),
                name,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }]),
        }
    }

    /// Extend env with local bindings (clone-on-scope-extension).
    /// Shadows foreign imports silently.
    /// Errors on re-shadowing a local/top-level.
    pub fn add_locals(
        &self,
        bindings: &std::collections::BTreeMap<&'a str, Region>,
    ) -> Result<Env<'a>, Vec<Error<'a>>> {
        let mut new_env = self.clone();
        let mut errors = Vec::new();

        for (&name, &region) in bindings {
            match new_env.vars.get(name) {
                Some(Var::Local(original)) | Some(Var::TopLevel(original)) => {
                    errors.push(Error::Shadowing {
                        name,
                        original: *original,
                        new: region,
                    });
                }
                _ => {
                    new_env.vars.insert(name, Var::Local(region));
                }
            }
        }

        if errors.is_empty() {
            Ok(new_env)
        } else {
            Err(errors)
        }
    }

    /// Look up a binop by symbol. Mirrors Elm's `Env.findBinop`.
    pub fn find_binop(
        &self,
        bump: &'a Bump,
        region: Region,
        symbol: &'a str,
    ) -> Result<Binop<'a>, Vec<Error<'a>>> {
        match self.binops.get(symbol) {
            Some(Info::Specific(_, binop)) => Ok(*binop),
            Some(Info::Ambiguous(first, others)) => Err(vec![Error::AmbiguousBinop {
                region,
                name: symbol,
                first_module: *first,
                other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
            }]),
            None => Err(vec![Error::NotFoundBinop {
                region,
                name: symbol,
                available: self.available_binops(bump),
            }]),
        }
    }

    fn available_binops(&self, bump: &'a Bump) -> &'a [&'a str] {
        bump.alloc_slice_fill_iter(self.binops.keys().copied())
    }

    pub fn possible_var_names(&self, bump: &'a Bump) -> crate::error::PossibleNames<'a> {
        let locals = bump.alloc_slice_fill_iter(self.vars.keys().copied());
        let qualified = bump.alloc_slice_fill_iter(self.q_vars.iter().map(|(prefix, inner)| {
            let names = bump.alloc_slice_fill_iter(inner.keys().copied());
            (*prefix, names as &[&str])
        }));
        crate::error::PossibleNames { locals, qualified }
    }

    pub fn possible_type_names(&self, bump: &'a Bump) -> crate::error::PossibleNames<'a> {
        let locals = bump.alloc_slice_fill_iter(self.types.keys().copied());
        let qualified = bump.alloc_slice_fill_iter(self.q_types.iter().map(|(prefix, inner)| {
            let names = bump.alloc_slice_fill_iter(inner.keys().copied());
            (*prefix, names as &[&str])
        }));
        crate::error::PossibleNames { locals, qualified }
    }

    pub fn possible_ctor_names(&self, bump: &'a Bump) -> crate::error::PossibleNames<'a> {
        let locals = bump.alloc_slice_fill_iter(self.ctors.keys().copied());
        let qualified = bump.alloc_slice_fill_iter(self.q_ctors.iter().map(|(prefix, inner)| {
            let names = bump.alloc_slice_fill_iter(inner.keys().copied());
            (*prefix, names as &[&str])
        }));
        crate::error::PossibleNames { locals, qualified }
    }
}

// --- Merge helpers (Elm's mergeInfo) ---

pub fn merge_exposed<'a, T: Clone>(
    table: &mut Exposed<'a, T>,
    name: &'a str,
    home: ModuleName<'a>,
    value: T,
) {
    use std::collections::btree_map::Entry;
    match table.entry(name) {
        Entry::Vacant(e) => {
            e.insert(Info::Specific(home, value));
        }
        Entry::Occupied(mut e) => match e.get() {
            // Elm's `mergeInfo` compares the full canonical name (package
            // and module) and keeps the FIRST value when they are equal.
            Info::Specific(existing, _) if *existing != home => {
                let first = *existing;
                e.insert(Info::Ambiguous(first, vec![home]));
            }
            // Like `OneOrMore.more`, appends are unconditional.
            Info::Ambiguous(..) => {
                if let Info::Ambiguous(_, others) = e.get_mut() {
                    others.push(home);
                }
            }
            _ => {}
        },
    }
}

pub fn merge_qualified<'a, T: Clone>(
    table: &mut Qualified<'a, T>,
    prefix: &'a str,
    name: &'a str,
    home: ModuleName<'a>,
    value: T,
) {
    let inner = table.entry(prefix).or_default();
    merge_exposed(inner, name, home, value);
}
