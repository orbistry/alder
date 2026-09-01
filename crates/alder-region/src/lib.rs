use std::cmp::Ordering;

#[derive(Clone, Debug, Eq, Copy, PartialEq, PartialOrd, Ord, Hash)]
pub struct Located<T> {
    pub region: Region,
    pub value: T,
}

impl<T> Located<T> {
    pub const fn at(region: Region, value: T) -> Located<T> {
        Located { region, value }
    }

    pub const fn at_zero(value: T) -> Located<T> {
        let region = Region::zero();

        Located { region, value }
    }
}

#[derive(Clone, Debug, Eq, Copy, PartialEq, PartialOrd, Ord, Hash)]
pub struct Region {
    pub start: Position,
    pub end: Position,
}

impl Region {
    pub const fn zero() -> Self {
        Self {
            start: Position::zero(),
            end: Position::zero(),
        }
    }

    pub const fn one() -> Self {
        Self {
            start: Position::one(),
            end: Position::one(),
        }
    }

    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, other: &Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    pub fn contains_pos(&self, pos: Position) -> bool {
        self.start <= pos && self.end >= pos
    }

    pub fn span_across(start: &Region, end: &Region) -> Self {
        Region {
            start: start.start,
            end: end.end,
        }
    }
}

#[derive(Clone, Debug, Eq, Copy, PartialEq, Hash)]
pub struct Position {
    pub line: u16,
    pub column: u16,
}

impl Position {
    pub const fn new(line: u16, column: u16) -> Self {
        Self { line, column }
    }

    pub const fn zero() -> Self {
        Self { line: 0, column: 0 }
    }

    pub const fn one() -> Self {
        Self { line: 1, column: 1 }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> Ordering {
        self.line
            .cmp(&other.line)
            .then_with(|| self.column.cmp(&other.column))
    }
}
