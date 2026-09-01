use bumpalo::Bump;

use crate::Error;

/// Elm's `f <$> a <*> b`.
pub fn accumulate2<'a, A, B>(
    a: Result<A, Vec<Error<'a>>>,
    b: Result<B, Vec<Error<'a>>>,
) -> Result<(A, B), Vec<Error<'a>>> {
    match (a, b) {
        (Ok(a), Ok(b)) => Ok((a, b)),
        (Err(mut e1), Err(mut e2)) => {
            e1.append(&mut e2);
            Err(e1)
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

/// Elm's `f <$> a <*> b <*> c`.
pub fn accumulate3<'a, A, B, C>(
    a: Result<A, Vec<Error<'a>>>,
    b: Result<B, Vec<Error<'a>>>,
    c: Result<C, Vec<Error<'a>>>,
) -> Result<(A, B, C), Vec<Error<'a>>> {
    let mut errors = Vec::new();
    let a = collect_result(a, &mut errors);
    let b = collect_result(b, &mut errors);
    let c = collect_result(c, &mut errors);
    match (a, b, c) {
        (Some(a), Some(b), Some(c)) => Ok((a, b, c)),
        _ => Err(errors),
    }
}

/// Elm's `f <$> a <*> b <*> c <*> d`.
pub fn accumulate4<'a, A, B, C, D>(
    a: Result<A, Vec<Error<'a>>>,
    b: Result<B, Vec<Error<'a>>>,
    c: Result<C, Vec<Error<'a>>>,
    d: Result<D, Vec<Error<'a>>>,
) -> Result<(A, B, C, D), Vec<Error<'a>>> {
    let mut errors = Vec::new();
    let a = collect_result(a, &mut errors);
    let b = collect_result(b, &mut errors);
    let c = collect_result(c, &mut errors);
    let d = collect_result(d, &mut errors);
    match (a, b, c, d) {
        (Some(a), Some(b), Some(c), Some(d)) => Ok((a, b, c, d)),
        _ => Err(errors),
    }
}

fn collect_result<'a, T>(
    result: Result<T, Vec<Error<'a>>>,
    errors: &mut Vec<Error<'a>>,
) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(mut errs) => {
            errors.append(&mut errs);
            None
        }
    }
}

/// Elm's `traverse f xs` (Applicative).
pub fn try_all<'a, T>(
    iter: impl IntoIterator<Item = Result<T, Vec<Error<'a>>>>,
) -> Result<Vec<T>, Vec<Error<'a>>> {
    let mut results = Vec::new();
    let mut errors = Vec::new();
    for item in iter {
        match item {
            Ok(v) => results.push(v),
            Err(mut errs) => errors.append(&mut errs),
        }
    }
    if errors.is_empty() {
        Ok(results)
    } else {
        Err(errors)
    }
}

pub fn try_all_alloc<'a, T>(
    bump: &'a Bump,
    iter: impl IntoIterator<Item = Result<T, Vec<Error<'a>>>>,
) -> Result<&'a [T], Vec<Error<'a>>> {
    try_all(iter).map(|results| {
        let s: &[T] = bump.alloc_slice_fill_iter(results);
        s
    })
}

pub fn try_all_alloc_ref<'a, T>(
    bump: &'a Bump,
    iter: impl IntoIterator<Item = Result<&'a T, Vec<Error<'a>>>>,
) -> Result<&'a [&'a T], Vec<Error<'a>>> {
    try_all(iter).map(|results| {
        let s: &[&'a T] = bump.alloc_slice_fill_iter(results);
        s
    })
}
