//! `import` items and module paths (`@author/package/seg`, `~/seg`).
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/import.rs (Wave 3)

use alder_region::Located;
use alder_source::{Import, ModulePath};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `import`. The bare tail (`ImportTail::Module`) is validated here: no
    /// segments (`import ~`) → Import::RootOnly; reserved last segment
    /// (`import @alder/test`) → Import::ReservedBinding(kw).
    pub(crate) fn import(&mut self, is_pub: bool) -> Result<&'a Import<'a>, error::Import<'a>> {
        todo!()
    }

    /// `@author/package { '/' seg }` | `~ { '/' seg }`; author, package and segments via `raw_lower` (§2.4).
    pub(crate) fn module_path(&mut self) -> Result<Located<ModulePath<'a>>, error::ModulePath> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
