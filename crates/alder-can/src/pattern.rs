use alder_ast::{ArrayRest, BindingName, Pattern as CanPattern, PatternField, VariantPayload};
use alder_region::Located;
use alder_source::Pattern as SourcePattern;
use bumpalo::Bump;

use crate::environment::{Env, Scope};
use crate::expression::canonicalize_expr;
use crate::{Error, ErrorKind, PatternError};

#[derive(Clone, Copy, Debug)]
pub enum BindingMode<'scope, 'a> {
    Local,
    Match,
    TopLevel,
    Alternative(&'scope Scope<'a>),
}

impl BindingMode<'_, '_> {
    fn is_match(self) -> bool {
        matches!(self, Self::Match | Self::Alternative(_))
    }
}

pub fn canonicalize_pattern<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a Located<SourcePattern<'a>>,
    mode: BindingMode<'_, 'a>,
) -> Result<&'a Located<CanPattern<'a>>, Vec<Error<'a>>> {
    let pin_scopes = if mode.is_match() {
        env.scopes.clone()
    } else {
        Vec::new()
    };
    let pattern = canonicalize_pattern_inner(bump, env, source, mode, &pin_scopes)?;
    if let BindingMode::Alternative(first) = mode {
        let expected = first.values.keys().copied().collect::<Vec<_>>();
        let actual = env
            .scopes
            .last()
            .expect("pattern scope")
            .values
            .keys()
            .copied()
            .collect::<Vec<_>>();
        if expected != actual {
            return Err(vec![Error::new(
                source.region,
                ErrorKind::Pattern(PatternError::AlternativeBindings {
                    expected: bump.alloc_slice_copy(&expected),
                    actual: bump.alloc_slice_copy(&actual),
                }),
            )]);
        }
    }
    Ok(pattern)
}

fn canonicalize_pattern_inner<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a Located<SourcePattern<'a>>,
    mode: BindingMode<'_, 'a>,
    pin_scopes: &[Scope<'a>],
) -> Result<&'a Located<CanPattern<'a>>, Vec<Error<'a>>> {
    let pattern = match source.value {
        SourcePattern::Anything => CanPattern::Anything,
        SourcePattern::Var(name) => CanPattern::Bind(bind(env, name, source.region, mode)?),
        SourcePattern::Pin(expr) => {
            if !mode.is_match() {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Pattern(PatternError::PinOutsideMatch),
                )]);
            }
            let mut pin_env = env.clone();
            pin_env.scopes = pin_scopes.to_vec();
            CanPattern::Pin {
                use_id: env.fresh_use(),
                value: canonicalize_expr(bump, &mut pin_env, expr)?,
            }
        }
        SourcePattern::Number(number) => CanPattern::Number {
            value: number.value,
            text: number.text,
        },
        SourcePattern::BigInt(value) => CanPattern::BigInt(value),
        SourcePattern::Str(value) => CanPattern::Str(value),
        SourcePattern::Bool(value) => CanPattern::Bool(value),
        SourcePattern::Unit => CanPattern::Unit,
        SourcePattern::Ctor { path, args } => {
            let constructor = env
                .find_constructor(bump, source.region, path.segments, mode.is_match())
                .map_err(|error| vec![error])?;
            let VariantPayload::Tuple(types) = constructor.payload else {
                if args.is_empty() && matches!(constructor.payload, VariantPayload::Unit) {
                    return Ok(bump.alloc(Located::at(
                        source.region,
                        CanPattern::Constructor {
                            constructor,
                            args: &[],
                        },
                    )));
                }
                return Err(vec![payload_error(source.region, constructor, "tuple")]);
            };
            if types.len() != args.len() {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Pattern(PatternError::ConstructorArity {
                        name: crate::error::ConstructorDisplay {
                            enum_name: constructor.name.enum_.name,
                            variant: constructor.name.variant,
                        },
                        expected: types.len(),
                        actual: args.len(),
                    }),
                )]);
            }
            CanPattern::Constructor {
                constructor,
                args: canonicalize_patterns(bump, env, args, mode, pin_scopes)?,
            }
        }
        SourcePattern::CtorRecord { path, fields, rest } => {
            let constructor = env
                .find_constructor(bump, source.region, path.segments, mode.is_match())
                .map_err(|error| vec![error])?;
            if !matches!(constructor.payload, VariantPayload::Record(_)) {
                return Err(vec![payload_error(source.region, constructor, "record")]);
            }
            CanPattern::ConstructorRecord {
                constructor,
                fields: canonicalize_fields(bump, env, fields, mode, pin_scopes)?,
                rest: rest.is_some(),
            }
        }
        SourcePattern::Tag { name, args } => CanPattern::Tag {
            group: None,
            name,
            args: canonicalize_patterns(bump, env, args, mode, pin_scopes)?,
        },
        SourcePattern::Tuple {
            first,
            second,
            rest,
        } => {
            let mut patterns = Vec::with_capacity(2 + rest.len());
            patterns.push(canonicalize_pattern_inner(
                bump, env, first, mode, pin_scopes,
            )?);
            patterns.push(canonicalize_pattern_inner(
                bump, env, second, mode, pin_scopes,
            )?);
            for pattern in rest {
                patterns.push(canonicalize_pattern_inner(
                    bump, env, pattern, mode, pin_scopes,
                )?);
            }
            CanPattern::Tuple(bump.alloc_slice_copy(&patterns))
        }
        SourcePattern::Array { elements, rest } => CanPattern::Array {
            elements: canonicalize_patterns(bump, env, elements, mode, pin_scopes)?,
            rest: match rest {
                Some(rest) => Some(ArrayRest {
                    region: rest.region,
                    name: match rest.name {
                        Some(name) => Some(bind(env, name.value, name.region, mode)?),
                        None => None,
                    },
                }),
                None => None,
            },
        },
        SourcePattern::Record { fields, rest } => CanPattern::Record {
            fields: canonicalize_fields(bump, env, fields, mode, pin_scopes)?,
            rest: rest.is_some(),
        },
        SourcePattern::Alias { pattern, name } => CanPattern::Alias {
            pattern: canonicalize_pattern_inner(bump, env, pattern, mode, pin_scopes)?,
            name: bind(env, name.value, name.region, mode)?,
        },
    };
    Ok(bump.alloc(Located::at(source.region, pattern)))
}

fn canonicalize_patterns<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    patterns: &'a [&'a Located<SourcePattern<'a>>],
    mode: BindingMode<'_, 'a>,
    pin_scopes: &[Scope<'a>],
) -> Result<&'a [&'a Located<CanPattern<'a>>], Vec<Error<'a>>> {
    let mut result = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        result.push(canonicalize_pattern_inner(
            bump, env, pattern, mode, pin_scopes,
        )?);
    }
    Ok(bump.alloc_slice_copy(&result))
}

fn canonicalize_fields<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    fields: &'a [alder_source::FieldPattern<'a>],
    mode: BindingMode<'_, 'a>,
    pin_scopes: &[Scope<'a>],
) -> Result<&'a [PatternField<'a>], Vec<Error<'a>>> {
    let mut result = Vec::with_capacity(fields.len());
    let mut seen = std::collections::BTreeMap::new();
    for field in fields {
        if let Some(first) = seen.insert(field.name.value, field.name.region) {
            return Err(vec![Error::new(
                field.name.region,
                ErrorKind::Pattern(PatternError::DuplicateField {
                    name: field.name.value,
                    first,
                }),
            )]);
        }
        let pattern = match field.pattern {
            Some(pattern) => canonicalize_pattern_inner(bump, env, pattern, mode, pin_scopes)?,
            None => bump.alloc(Located::at(
                field.name.region,
                CanPattern::Bind(bind(env, field.name.value, field.name.region, mode)?),
            )),
        };
        result.push(PatternField {
            name: field.name,
            pattern,
        });
    }
    Ok(bump.alloc_slice_copy(&result))
}

fn bind<'a>(
    env: &mut Env<'a>,
    name: &'a str,
    region: alder_region::Region,
    mode: BindingMode<'_, 'a>,
) -> Result<BindingName<'a>, Vec<Error<'a>>> {
    match mode {
        BindingMode::Local | BindingMode::Match | BindingMode::Alternative(_) => {
            let mut local = env.insert_local(name, region).map_err(|first| {
                vec![Error::new(
                    region,
                    ErrorKind::Pattern(PatternError::DuplicateBinding { name, first }),
                )]
            })?;
            if let BindingMode::Alternative(first) = mode
                && let Some(binding) = first.values.get(name)
                && let alder_ast::ValueRef::Local(shared) = binding.reference
            {
                local = shared;
                env.scopes
                    .last_mut()
                    .expect("pattern scope")
                    .values
                    .get_mut(name)
                    .expect("inserted binding")
                    .reference = alder_ast::ValueRef::Local(shared);
            }
            Ok(BindingName::Local(local))
        }
        BindingMode::TopLevel => env
            .find_value(name)
            .and_then(|binding| match binding.reference {
                alder_ast::ValueRef::TopLevel(reference) => Some(BindingName::TopLevel(reference)),
                _ => None,
            })
            .ok_or_else(|| {
                vec![Error::new(
                    region,
                    ErrorKind::Pattern(PatternError::DuplicateBinding {
                        name,
                        first: region,
                    }),
                )]
            }),
    }
}

fn payload_error<'a>(
    region: alder_region::Region,
    constructor: alder_ast::ConstructorRef<'a>,
    actual: &'static str,
) -> Error<'a> {
    Error::new(
        region,
        ErrorKind::Pattern(PatternError::ConstructorPayload {
            name: crate::error::ConstructorDisplay {
                enum_name: constructor.name.enum_.name,
                variant: constructor.name.variant,
            },
            expected: match constructor.payload {
                VariantPayload::Unit => "unit",
                VariantPayload::Tuple(_) => "tuple",
                VariantPayload::Record(_) => "record",
            },
            actual,
        }),
    )
}
