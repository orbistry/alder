use std::collections::BTreeMap;

use alder_region::Region;

use crate::Error;

/// Mirrors Elm's `Dups.detect`: exactly one error per duplicated name,
/// carrying the first two occurrences, reported in name order. Errors
/// for distinct names accumulate (Elm's `Map.traverseWithKey` over an
/// error-accumulating `Result`).
pub fn detect<'a>(
    items: impl IntoIterator<Item = (&'a str, Region)>,
    to_error: impl Fn(&'a str, Region, Region) -> Error<'a>,
) -> Result<BTreeMap<&'a str, Region>, Vec<Error<'a>>> {
    let mut occurrences: BTreeMap<&'a str, Vec<Region>> = BTreeMap::new();

    for (name, region) in items {
        occurrences.entry(name).or_default().push(region);
    }

    let mut seen: BTreeMap<&'a str, Region> = BTreeMap::new();
    let mut errors: Vec<Error<'a>> = Vec::new();

    for (name, regions) in occurrences {
        if regions.len() > 1 {
            errors.push(to_error(name, regions[0], regions[1]));
        } else {
            seen.insert(name, regions[0]);
        }
    }

    if errors.is_empty() {
        Ok(seen)
    } else {
        Err(errors)
    }
}
