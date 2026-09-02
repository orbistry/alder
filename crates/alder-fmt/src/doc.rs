//! A small Wadler-style document algebra used by the source formatter.

#[derive(Clone, Debug)]
pub enum Doc {
    Nil,
    Text(String),
    Line,
    Concat(Vec<Doc>),
    Nest(usize, Box<Doc>),
    Group(Box<Doc>),
}

impl Doc {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn concat(docs: impl IntoIterator<Item = Doc>) -> Self {
        Self::Concat(docs.into_iter().collect())
    }

    pub fn nest(self, amount: usize) -> Self {
        Self::Nest(amount, Box::new(self))
    }

    pub fn group(self) -> Self {
        Self::Group(Box::new(self))
    }

    pub fn render(&self, width: usize) -> String {
        let mut output = String::new();
        let mut stack = vec![(0, false, self)];
        let mut column = 0;
        while let Some((indent, flat, doc)) = stack.pop() {
            match doc {
                Doc::Nil => {}
                Doc::Text(text) => {
                    output.push_str(text);
                    column += text.chars().count();
                }
                Doc::Line if flat && column < width => {
                    output.push(' ');
                    column += 1;
                }
                Doc::Line => {
                    output.push('\n');
                    output.extend(std::iter::repeat_n(' ', indent));
                    column = indent;
                }
                Doc::Concat(docs) => {
                    stack.extend(docs.iter().rev().map(|doc| (indent, flat, doc)));
                }
                Doc::Nest(amount, doc) => stack.push((indent + amount, flat, doc)),
                Doc::Group(doc) => {
                    stack.push((indent, fits(doc, width.saturating_sub(column)), doc))
                }
            }
        }
        output
    }
}

fn fits(doc: &Doc, mut remaining: usize) -> bool {
    let mut stack = vec![doc];
    while let Some(doc) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(text) => {
                let len = text.chars().count();
                if len > remaining {
                    return false;
                }
                remaining -= len;
            }
            Doc::Line => return true,
            Doc::Concat(docs) => stack.extend(docs.iter().rev()),
            Doc::Nest(_, doc) | Doc::Group(doc) => stack.push(doc),
        }
    }
    true
}
