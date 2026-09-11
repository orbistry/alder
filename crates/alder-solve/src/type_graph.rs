//! Shared inference types with weighted union-find representatives.
//!
//! Like Elm's `Type.UnionFind`, a point owns either a descriptor or a link to
//! another point. Structure edges are points too: copying a type never copies
//! its descendants. Union weight is only a balancing heuristic; it is not an
//! inference level and must not determine which variables may be generalized.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use alder_ast::{AssocTypeId, QualifiedName, TraitId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VariableKind {
    Unknown,
    Type,
    RecordRow,
    ErrorRow,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Content<'a> {
    Var(usize),
    Con(QualifiedName<'a>),
    App(Ty<'a>, Vec<Ty<'a>>),
    Partial(QualifiedName<'a>, Vec<TySlot<'a>>),
    Projection(TraitId<'a>, Vec<Ty<'a>>, AssocTypeId<'a>),
    Fn(Vec<Ty<'a>>, Ty<'a>),
    Unit,
    Tuple(Vec<Ty<'a>>),
    Record(BTreeMap<&'a str, Ty<'a>>, Option<Ty<'a>>),
    RecordRow(Ty<'a>),
    ErrorRow {
        tags: BTreeMap<&'a str, Vec<Ty<'a>>>,
        tail: Option<Ty<'a>>,
    },
    Any,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum TySlot<'a> {
    Hole(u16),
    Fixed(Ty<'a>),
}

#[derive(Clone)]
pub(super) struct Ty<'a>(Rc<RefCell<Point<'a>>>);

enum Point<'a> {
    Root {
        weight: usize,
        kind: VariableKind,
        content: Rc<Content<'a>>,
    },
    Link(Ty<'a>),
}

impl<'a> Ty<'a> {
    /// An owned, shallow descriptor snapshot; every child remains a shared
    /// graph handle. Use `content` to inspect a descriptor without copying it.
    pub fn view(&self) -> Content<'a> {
        self.content().as_ref().clone()
    }

    pub fn new(content: Content<'a>) -> Self {
        let kind = match content {
            Content::Var(_) | Content::Any => VariableKind::Unknown,
            Content::RecordRow(_) => VariableKind::RecordRow,
            Content::ErrorRow { .. } => VariableKind::ErrorRow,
            _ => VariableKind::Type,
        };
        Self(Rc::new(RefCell::new(Point::Root {
            weight: 1,
            kind,
            content: Rc::new(content),
        })))
    }

    fn same_point(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    fn root(&self) -> Self {
        let mut root = self.clone();
        loop {
            let next = match &*root.0.borrow() {
                Point::Root { .. } => break,
                Point::Link(next) => next.clone(),
            };
            root = next;
        }
        let mut point = self.clone();
        while !point.same_point(&root) {
            let next = match &*point.0.borrow() {
                Point::Link(next) => next.clone(),
                Point::Root { .. } => unreachable!("only the root owns a descriptor"),
            };
            if !next.same_point(&root) {
                *point.0.borrow_mut() = Point::Link(root.clone());
            }
            point = next;
        }
        root
    }

    pub fn content(&self) -> Rc<Content<'a>> {
        match &*self.root().0.borrow() {
            Point::Root { content, .. } => content.clone(),
            Point::Link(_) => unreachable!("root owns a descriptor"),
        }
    }

    pub fn kind(&self) -> VariableKind {
        match &*self.root().0.borrow() {
            Point::Root { kind, .. } => *kind,
            Point::Link(_) => unreachable!("root owns a descriptor"),
        }
    }

    pub fn set_kind(&self, new_kind: VariableKind) {
        match &mut *self.root().0.borrow_mut() {
            Point::Root { kind, .. } => *kind = new_kind,
            Point::Link(_) => unreachable!("root owns a descriptor"),
        }
    }

    pub fn equivalent(&self, other: &Self) -> bool {
        self.same_point(other) || self.root().same_point(&other.root())
    }

    /// Called only after the unifier has checked kinds, structure, and occurs.
    /// Descriptor selection is independent of which weighted root wins. In
    /// particular, diagnostic names must not depend on balancing decisions.
    pub fn union(&self, other: &Self, content: Rc<Content<'a>>, kind: VariableKind) {
        let left = self.root();
        let right = other.root();
        if left.same_point(&right) {
            return;
        }
        let weight = |point: &Self| match &*point.0.borrow() {
            Point::Root { weight, .. } => *weight,
            Point::Link(_) => unreachable!("root owns a descriptor"),
        };
        let left_weight = weight(&left);
        let right_weight = weight(&right);
        let (winner, loser) = if left_weight >= right_weight {
            (left, right)
        } else {
            (right, left)
        };
        *loser.0.borrow_mut() = Point::Link(winner.clone());
        *winner.0.borrow_mut() = Point::Root {
            weight: left_weight + right_weight,
            kind,
            content,
        };
    }
}

impl PartialEq for Ty<'_> {
    fn eq(&self, other: &Self) -> bool {
        if self.equivalent(other) {
            return true;
        }
        let left = self.content();
        let right = other.content();
        // Diagnostic IDs are local to an attempt, not graph identities.
        !matches!(left.as_ref(), Content::Var(_)) && left == right
    }
}

impl std::fmt::Debug for Ty<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.content().fmt(formatter)
    }
}

#[derive(Default)]
pub(super) struct TypeGraph<'a> {
    variables: Vec<Ty<'a>>,
    revision: usize,
}

impl<'a> TypeGraph<'a> {
    pub fn fresh(&mut self, kind: VariableKind) -> Ty<'a> {
        let variable = Ty::new(Content::Var(self.variables.len()));
        variable.set_kind(kind);
        self.variables.push(variable.clone());
        variable
    }

    pub fn variable(&self, id: usize) -> Ty<'a> {
        self.variables[id].clone()
    }

    pub fn revision(&self) -> usize {
        self.revision
    }

    pub fn bind(&mut self, id: usize, typ: &Ty<'a>, kind: VariableKind) {
        let variable = &self.variables[id];
        if !variable.equivalent(typ) {
            variable.union(typ, typ.content(), kind);
            self.revision += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_roots_do_not_choose_variable_names() {
        let mut graph = TypeGraph::default();
        let variables = (0..64)
            .map(|_| graph.fresh(VariableKind::Type))
            .collect::<Vec<_>>();
        for (index, variable) in variables.iter().enumerate().skip(1) {
            graph.bind(index - 1, variable, VariableKind::Type);
        }
        assert!(variables[0].root().same_point(&variables[0]));
        assert_eq!(*variables[0].content(), Content::Var(63));
        assert!(variables.iter().all(|ty| ty.equivalent(&variables[0])));
        assert_eq!(graph.revision(), 63);
    }

    #[test]
    fn find_compresses_balanced_union_paths() {
        let mut graph = TypeGraph::default();
        let variables = (0..8)
            .map(|_| graph.fresh(VariableKind::Type))
            .collect::<Vec<_>>();
        for width in [1, 2, 4] {
            for index in (0..8).step_by(width * 2) {
                graph.bind(index, &variables[index + width], VariableKind::Type);
            }
        }
        let root = variables[7].root();
        match &*variables[7].0.borrow() {
            Point::Link(parent) => assert!(parent.same_point(&root)),
            Point::Root { .. } => panic!("the final point is not the weighted root"),
        }
    }

    #[test]
    fn structure_edges_share_variable_updates() {
        let mut graph = TypeGraph::default();
        let value = graph.fresh(VariableKind::Type);
        let pair = Ty::new(Content::Tuple(vec![value.clone(), value]));
        let alias = pair.clone();
        let unit = Ty::new(Content::Unit);
        graph.bind(0, &unit, VariableKind::Type);
        assert!(pair.same_point(&alias));
        let content = alias.content();
        let Content::Tuple(fields) = content.as_ref() else {
            panic!("tuple")
        };
        assert!(fields.iter().all(|field| field.equivalent(&unit)));
    }

    #[test]
    fn independent_attempts_share_no_mutable_graph_state() {
        let mut first = TypeGraph::default();
        let failed = first.fresh(VariableKind::Unknown);
        first.bind(0, &Ty::new(Content::Unit), VariableKind::Type);
        let mut second = TypeGraph::default();
        let retry = second.fresh(VariableKind::Unknown);
        assert!(!failed.equivalent(&retry));
        assert_eq!(*retry.content(), Content::Var(0));
        assert_eq!(retry.kind(), VariableKind::Unknown);
        assert_eq!(second.revision(), 0);
        let mut third = TypeGraph::default();
        assert_ne!(retry, third.fresh(VariableKind::Unknown));
    }

    #[test]
    fn dropping_an_attempt_reclaims_linked_classes_and_shared_children() {
        let points = {
            let mut graph = TypeGraph::default();
            let value = graph.fresh(VariableKind::Type);
            let output = graph.fresh(VariableKind::Type);
            let pair = Ty::new(Content::Tuple(vec![value.clone(), value.clone()]));
            let unit = Ty::new(Content::Unit);
            let points = [&value, &output, &pair, &unit].map(|point| Rc::downgrade(&point.0));
            graph.bind(1, &pair, VariableKind::Type);
            graph.bind(0, &unit, VariableKind::Type);
            points
        };
        assert!(points.iter().all(|point| point.upgrade().is_none()));
    }
}
