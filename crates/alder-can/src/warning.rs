use alder_region::Region;

#[derive(Clone, Copy, Debug)]
pub struct Warning<'a> {
    pub region: Region,
    pub kind: WarningKind<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum WarningKind<'a> {
    UnusedImport { name: &'a str },
    UnusedBinding { name: &'a str },
    UnusedTypeParameter { name: &'a str },
}
