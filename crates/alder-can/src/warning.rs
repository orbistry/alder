use alder_region::Region;

/// Mirrors Elm's `Reporting.Warning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning<'a> {
    UnusedVariable {
        region: Region,
        context: WarningContext,
        name: &'a str,
    },
    UnusedImport {
        region: Region,
        module_name: &'a str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningContext {
    /// Unused variable introduced by a pattern (lambda arg, case branch, let destruct)
    Pattern,
    /// Unused variable introduced by a let definition
    Def,
}
