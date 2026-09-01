//! Port of Elm's `Type.UnionFind`.
//!
//! Elm's `Point` is an `IORef` cell graph; here the cells live in a single
//! `Vec` owned by [`UnionFind`] and a [`Variable`] is an index into it.
//! Index equality is exactly Elm's `IORef` identity equality, and `union`
//! keeps Elm's weight balancing so representative choices match.

use crate::type_::Descriptor;

/// A type variable: Elm's `Type.Variable = UF.Point Descriptor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Variable(u32);

impl Variable {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug)]
enum PointInfo<'a> {
    Info { weight: u32, desc: Descriptor<'a> },
    Link(Variable),
}

#[derive(Debug, Default)]
pub struct UnionFind<'a> {
    points: Vec<PointInfo<'a>>,
}

impl<'a> UnionFind<'a> {
    pub fn new() -> Self {
        UnionFind { points: Vec::new() }
    }

    pub fn fresh(&mut self, desc: Descriptor<'a>) -> Variable {
        let var = Variable(self.points.len() as u32);
        self.points.push(PointInfo::Info { weight: 1, desc });
        var
    }

    /// Find the representative, compressing every point on the path to link
    /// directly to it (the net effect of Elm's recursive `repr`).
    fn repr(&mut self, point: Variable) -> Variable {
        let mut root = point;
        while let PointInfo::Link(next) = self.points[root.index()] {
            root = next;
        }
        let mut walk = point;
        while let PointInfo::Link(next) = self.points[walk.index()] {
            if next != root {
                self.points[walk.index()] = PointInfo::Link(root);
            }
            walk = next;
        }
        root
    }

    pub fn get(&mut self, point: Variable) -> &Descriptor<'a> {
        let root = self.repr(point);
        match &self.points[root.index()] {
            PointInfo::Info { desc, .. } => desc,
            PointInfo::Link(_) => unreachable!("repr returns a root"),
        }
    }

    pub fn set(&mut self, point: Variable, new_desc: Descriptor<'a>) {
        let root = self.repr(point);
        match &mut self.points[root.index()] {
            PointInfo::Info { desc, .. } => *desc = new_desc,
            PointInfo::Link(_) => unreachable!("repr returns a root"),
        }
    }

    pub fn modify(&mut self, point: Variable, func: impl FnOnce(&mut Descriptor<'a>)) {
        let root = self.repr(point);
        match &mut self.points[root.index()] {
            PointInfo::Info { desc, .. } => func(desc),
            PointInfo::Link(_) => unreachable!("repr returns a root"),
        }
    }

    pub fn union(&mut self, p1: Variable, p2: Variable, new_desc: Descriptor<'a>) {
        let point1 = self.repr(p1);
        let point2 = self.repr(p2);

        if point1 == point2 {
            match &mut self.points[point1.index()] {
                PointInfo::Info { desc, .. } => *desc = new_desc,
                PointInfo::Link(_) => unreachable!("repr returns a root"),
            }
            return;
        }

        let weight1 = match &self.points[point1.index()] {
            PointInfo::Info { weight, .. } => *weight,
            PointInfo::Link(_) => unreachable!("repr returns a root"),
        };
        let weight2 = match &self.points[point2.index()] {
            PointInfo::Info { weight, .. } => *weight,
            PointInfo::Link(_) => unreachable!("repr returns a root"),
        };

        let new_weight = weight1 + weight2;
        let (winner, loser) = if weight1 >= weight2 {
            (point1, point2)
        } else {
            (point2, point1)
        };

        self.points[loser.index()] = PointInfo::Link(winner);
        match &mut self.points[winner.index()] {
            PointInfo::Info { weight, desc } => {
                *weight = new_weight;
                *desc = new_desc;
            }
            PointInfo::Link(_) => unreachable!("repr returns a root"),
        }
    }

    pub fn equivalent(&mut self, p1: Variable, p2: Variable) -> bool {
        self.repr(p1) == self.repr(p2)
    }

    /// The representative of this point's equivalence class.
    pub fn find(&mut self, point: Variable) -> Variable {
        self.repr(point)
    }

    /// True when this point has been unioned into another representative.
    pub fn redundant(&self, point: Variable) -> bool {
        matches!(self.points[point.index()], PointInfo::Link(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_::{Content, make_descriptor};

    fn flex<'a>(uf: &mut UnionFind<'a>, name: &'a str) -> Variable {
        uf.fresh(make_descriptor(Content::FlexVar(Some(name))))
    }

    fn name_of<'a>(uf: &mut UnionFind<'a>, var: Variable) -> Option<&'a str> {
        match uf.get(var).content {
            Content::FlexVar(name) => name,
            _ => panic!("expected flex var"),
        }
    }

    #[test]
    fn fresh_points_are_distinct() {
        let mut uf = UnionFind::new();
        let a = flex(&mut uf, "a");
        let b = flex(&mut uf, "b");
        assert!(!uf.equivalent(a, b));
        assert!(uf.equivalent(a, a));
        assert!(!uf.redundant(a));
    }

    #[test]
    fn union_makes_points_equivalent_and_sets_descriptor() {
        let mut uf = UnionFind::new();
        let a = flex(&mut uf, "a");
        let b = flex(&mut uf, "b");
        uf.union(a, b, make_descriptor(Content::FlexVar(Some("c"))));
        assert!(uf.equivalent(a, b));
        assert_eq!(name_of(&mut uf, a), Some("c"));
        assert_eq!(name_of(&mut uf, b), Some("c"));
        assert!(uf.redundant(a) != uf.redundant(b));
    }

    #[test]
    fn set_and_modify_write_through_links() {
        let mut uf = UnionFind::new();
        let a = flex(&mut uf, "a");
        let b = flex(&mut uf, "b");
        let c = flex(&mut uf, "c");
        uf.union(a, b, make_descriptor(Content::FlexVar(Some("ab"))));
        uf.union(b, c, make_descriptor(Content::FlexVar(Some("abc"))));

        uf.set(c, make_descriptor(Content::FlexVar(Some("via-c"))));
        assert_eq!(name_of(&mut uf, a), Some("via-c"));

        uf.modify(a, |desc| desc.content = Content::FlexVar(Some("via-a")));
        assert_eq!(name_of(&mut uf, b), Some("via-a"));
        assert_eq!(name_of(&mut uf, c), Some("via-a"));
    }

    #[test]
    fn union_by_weight_keeps_heavier_representative() {
        let mut uf = UnionFind::new();
        let a = flex(&mut uf, "a");
        let b = flex(&mut uf, "b");
        let c = flex(&mut uf, "c");
        // a-b makes a two-element class rooted somewhere; unioning the
        // singleton c in keeps the heavier class's representative.
        uf.union(a, b, make_descriptor(Content::FlexVar(Some("ab"))));
        uf.union(c, a, make_descriptor(Content::FlexVar(Some("abc"))));
        assert!(uf.equivalent(a, c));
        assert!(uf.redundant(c));
    }

    #[test]
    fn union_of_same_class_replaces_descriptor() {
        let mut uf = UnionFind::new();
        let a = flex(&mut uf, "a");
        let b = flex(&mut uf, "b");
        uf.union(a, b, make_descriptor(Content::FlexVar(Some("first"))));
        uf.union(b, a, make_descriptor(Content::FlexVar(Some("second"))));
        assert_eq!(name_of(&mut uf, a), Some("second"));
    }

    #[test]
    fn long_chains_compress_to_the_root() {
        let mut uf = UnionFind::new();
        let vars: Vec<Variable> = (0..64).map(|_| flex(&mut uf, "x")).collect();
        for pair in vars.windows(2) {
            uf.union(pair[0], pair[1], make_descriptor(Content::FlexVar(None)));
        }
        for var in &vars {
            assert!(uf.equivalent(*var, vars[0]));
        }
        assert_eq!(
            vars.iter().filter(|var| !uf.redundant(**var)).count(),
            1,
            "exactly one representative"
        );
    }
}
