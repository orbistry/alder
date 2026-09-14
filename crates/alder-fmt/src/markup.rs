//! Layout markup using parsed children, never an HTML tag/whitespace heuristic.
use alder_region::{Located, Position, Region};
use alder_source::{AttrValue, Child, ChildBlock, ChildItem, Element, ElementName, Expr, Markup};
use bumpalo::Bump;

use crate::{Error, Options, ScanState, scan_line};

#[cfg(test)]
mod tests;

pub(super) fn format(source: &str, options: Options) -> Result<String, Error> {
    let bump = Bump::new();
    let (_, mut ranges) = alder_parse::parse_module_with_verbatim(&bump, source)
        .map_err(|error| Error::Parse(format!("{error:?}")))?;
    ranges.sort_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
    let mut output = String::new();
    let mut cursor = 0;
    for &(start, end) in &ranges {
        if start < cursor || !source[start..end].starts_with('<') {
            continue;
        }
        // A raw macro/template may contain markup-shaped bytes. Only format
        // spans not enclosed in another parser-designated verbatim payload.
        if ranges.iter().any(|&(a, b)| a < start && end <= b) {
            continue;
        }
        let text = &source[start..end];
        let mut parser = alder_parse::Parser::new(&bump, text.as_bytes());
        let expression = parser
            .expression()
            .map_err(|e| Error::Parse(format!("{e:?}")))?;
        let Expr::Markup(markup) = expression.value else {
            continue;
        };
        let mut masked = source.as_bytes()[..start].to_vec();
        for &(a, b) in &ranges {
            if a < start {
                masked[a..b.min(start)].fill(b' ');
            }
        }
        let mut state = ScanState::default();
        for line in std::str::from_utf8(&masked).expect("UTF-8 span").lines() {
            scan_line(line, &mut state);
        }
        let indent = state.delimiter_groups.len() * options.indent_width;
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        if source[line_start..start].trim().is_empty() {
            output.push_str(&source[cursor..line_start]);
            output.push_str(&" ".repeat(indent));
        } else {
            output.push_str(&source[cursor..start]);
        }
        let renderer = Renderer::new(text, options)?;
        output.push_str(&renderer.markup(markup, expression.region, indent));
        cursor = end;
    }
    output.push_str(&source[cursor..]);
    if signature(source)? != signature(&output).map_err(|e| Error::InvalidOutput(e.to_string()))? {
        return Err(Error::ChangedSource);
    }
    Ok(output)
}

// Compare the complete parsed structure, including comments and literal
// payloads, while excluding source locations. Scan quoted Debug strings intact
// so text containing "Region {" is never mistaken for location metadata.
fn signature(source: &str) -> Result<String, Error> {
    let bump = Bump::new();
    let module =
        alder_parse::parse_module(&bump, source).map_err(|e| Error::Parse(format!("{e:?}")))?;
    let debug = format!("{module:?}");
    let mut output = String::new();
    let mut index = 0;
    let bytes = debug.as_bytes();
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            while index < bytes.len() {
                match bytes[index] {
                    b'\\' => index += 2,
                    b'"' => {
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
            output.push_str(&debug[start..index]);
        } else if debug[index..].starts_with("Region {") || debug[index..].starts_with("Position {")
        {
            let mut depth = 0;
            while index < bytes.len() {
                match bytes[index] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            index += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            output.push_str("<location>");
        } else {
            let ch = debug[index..].chars().next().unwrap();
            output.push(ch);
            index += ch.len_utf8();
        }
    }
    Ok(output)
}

struct Renderer<'a> {
    source: &'a str,
    lines: Vec<usize>,
    options: Options,
    protected: Vec<(usize, usize)>,
    comments: Vec<(usize, usize)>,
}

impl<'a> Renderer<'a> {
    fn new(source: &'a str, options: Options) -> Result<Self, Error> {
        let mut lines = vec![0];
        lines.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        let prefix = "pub async fn formattingContext() {\n";
        let wrapped = format!("{prefix}{source}\n}}");
        let bump = Bump::new();
        let (module, ranges) = alder_parse::parse_module_with_verbatim(&bump, &wrapped)
            .map_err(|e| Error::Parse(format!("{e:?}")))?;
        let convert =
            |position: Position| lines[position.line as usize - 2] + position.column as usize - 1;
        let comments = module
            .comments
            .iter()
            .map(|c| (convert(c.region.start), convert(c.region.end)))
            .collect();
        let protected = ranges
            .into_iter()
            .filter_map(|(start, end)| {
                if start >= prefix.len()
                    && end <= prefix.len() + source.len()
                    && end - start < source.len()
                {
                    Some((start - prefix.len(), end - prefix.len()))
                } else {
                    None
                }
            })
            .collect();
        Ok(Self {
            source,
            lines,
            options,
            protected,
            comments,
        })
    }

    fn offset(&self, p: Position) -> usize {
        self.lines[p.line as usize - 1] + p.column as usize - 1
    }
    fn raw(&self, region: Region) -> &'a str {
        &self.source[self.offset(region.start)..self.offset(region.end)]
    }
    fn pad(&self, indent: usize) -> String {
        " ".repeat(indent)
    }

    fn comments_between(&self, start: usize, end: usize, indent: usize) -> String {
        self.comments
            .iter()
            .filter(|&&(a, b)| start <= a && b <= end)
            .map(|&(a, b)| format!("\n{}{}", self.pad(indent), &self.source[a..b]))
            .collect()
    }

    fn markup(&self, markup: &Markup<'_>, region: Region, indent: usize) -> String {
        match markup {
            Markup::Element(element) => self.element(element, region, indent),
            Markup::Fragment(children) => self.container("<>".into(), "</>", children, indent),
        }
    }

    fn element(&self, element: &Element<'_>, region: Region, indent: usize) -> String {
        // Preserve the entire lexical subtree, not just each text node: even
        // whitespace between otherwise independent children is meaningful here.
        if matches!(element.name.value, ElementName::Tag("pre" | "textarea")) {
            return self.raw(region).to_owned();
        }
        let name = self.raw(element.name.region);
        let mut attrs = Vec::new();
        let mut previous = self.offset(element.name.region.end);
        for attr in element.attrs {
            let comments = self.comments_between(
                previous,
                self.offset(attr.name.region.start),
                indent + self.options.indent_width,
            );
            if !comments.is_empty() {
                attrs.push(comments.trim_start().to_owned());
            }
            let value_start = match attr.value {
                None => attr.name.region.end,
                Some(AttrValue::Str(value)) => value.region.start,
                Some(AttrValue::Expr(expr)) => expr.region.start,
            };
            let comments = self.comments_between(
                self.offset(attr.name.region.end),
                self.offset(value_start),
                indent + self.options.indent_width,
            );
            if !comments.is_empty() {
                attrs.push(comments.trim_start().to_owned());
            }
            let name = self.raw(attr.name.region);
            attrs.push(match attr.value {
                None => name.to_owned(),
                Some(AttrValue::Str(value)) => format!("{name}={}", self.raw(value.region)),
                Some(AttrValue::Expr(expr)) => format!(
                    "{name}={{{}}}",
                    self.code(expr.region, indent + self.options.indent_width)
                ),
            });
            previous = self.offset(match attr.value {
                None => attr.name.region.end,
                Some(AttrValue::Str(value)) => value.region.end,
                Some(AttrValue::Expr(expr)) => expr.region.end,
            });
        }
        let close_start = self
            .raw(region)
            .rfind("</")
            .map_or(self.offset(region.end), |i| self.offset(region.start) + i);
        let header_end = element
            .children
            .first()
            .map_or(close_start, |child| self.offset(child.region.start));
        let comments =
            self.comments_between(previous, header_end, indent + self.options.indent_width);
        if !comments.is_empty() {
            attrs.push(comments.trim_start().to_owned());
        }
        let ending = if element.self_closing { " />" } else { ">" };
        let mut open = format!("<{name}");
        let width = indent
            + open.len()
            + attrs.iter().map(|attr| attr.len() + 1).sum::<usize>()
            + ending.len();
        if !attrs.is_empty()
            && (width > self.options.max_width
                || attrs
                    .iter()
                    .any(|a| a.contains('\n') || a.starts_with("//")))
        {
            for attr in attrs {
                open.push_str(&format!(
                    "\n{}{attr}",
                    self.pad(indent + self.options.indent_width)
                ));
            }
            open.push_str(&format!("\n{}{}", self.pad(indent), ending.trim_start()));
        } else {
            for attr in attrs {
                open.push(' ');
                open.push_str(&attr);
            }
            open.push_str(ending);
        }
        if element.self_closing {
            open
        } else {
            let comments = self.comments_between(
                close_start,
                self.offset(region.end),
                indent + self.options.indent_width,
            );
            let close = if comments.is_empty() {
                format!("</{name}>")
            } else {
                format!("</{name}{comments}\n{}>", self.pad(indent))
            };
            self.container(open, &close, element.children, indent)
        }
    }

    fn container(
        &self,
        open: String,
        close: &str,
        children: &[&Located<Child<'_>>],
        indent: usize,
    ) -> String {
        if children.is_empty() {
            return format!("{open}{close}");
        }
        let nested = indent + self.options.indent_width;
        let structural = children
            .iter()
            .all(|child| !matches!(child.value, Child::Text(_) | Child::Hole(_)));
        if structural {
            let body = children
                .iter()
                .map(|child| format!("{}{}", self.pad(nested), self.child(child, nested)))
                .collect::<Vec<_>>()
                .join("\n");
            return format!("{open}\n{body}\n{}{close}", self.pad(indent));
        }
        if let [child] = children
            && let Child::Text(text) = child.value
            && !text.starts_with([' ', '\t'])
            && !text.ends_with([' ', '\t'])
            && indent + open.len() + text.len() + close.len() > self.options.max_width
        {
            let body = self.wrap(text, nested);
            return format!(
                "{open}\n{}{body}\n{}{close}",
                self.pad(nested),
                self.pad(indent)
            );
        }
        // Mixed content is an inline sequence. Inserting breaks between these
        // children could drop significant boundary spaces or join words.
        let body = children
            .iter()
            .map(|child| self.child(child, indent))
            .collect::<String>();
        let safe_start =
            !matches!(children[0].value, Child::Text(text) if text.starts_with([' ', '\t']));
        let safe_end = !matches!(children[children.len() - 1].value, Child::Text(text) if text.ends_with([' ', '\t']));
        if safe_start
            && safe_end
            && (indent + open.len() + body.len() + close.len() > self.options.max_width
                || body.contains('\n'))
        {
            let body = children
                .iter()
                .map(|child| self.child(child, nested))
                .collect::<String>();
            return format!(
                "{open}\n{}{body}\n{}{close}",
                self.pad(nested),
                self.pad(indent)
            );
        }
        format!("{open}{body}{close}")
    }

    fn wrap(&self, text: &str, indent: usize) -> String {
        let mut output = String::new();
        let mut column = indent;
        let mut start = 0;
        for (index, _) in text.match_indices(' ') {
            // Only a single separating ASCII space is interchangeable with a
            // folded source newline. Never rewrite repeated or non-ASCII space.
            if text.as_bytes().get(index.wrapping_sub(1)) == Some(&b' ')
                || text.as_bytes().get(index + 1) == Some(&b' ')
            {
                continue;
            }
            let next_end = text[index + 1..]
                .find(' ')
                .map_or(text.len(), |n| index + 1 + n);
            if column + next_end - start > self.options.max_width
                && !text[index + 1..].starts_with('@')
            {
                output.push_str(&text[start..index]);
                output.push('\n');
                output.push_str(&self.pad(indent));
                start = index + 1;
                column = indent;
            }
        }
        output.push_str(&text[start..]);
        output
    }

    fn child(&self, child: &Located<Child<'_>>, indent: usize) -> String {
        match &child.value {
            Child::Text(text)
                if text.starts_with('@')
                    && text.as_bytes().get(1).is_some_and(u8::is_ascii_alphabetic) =>
            {
                format!("\n{}{text}", self.pad(indent))
            }
            Child::Text(text) => (*text).to_owned(),
            Child::Element(element) => self.element(element, child.region, indent),
            Child::Fragment(children) => self.container("<>".into(), "</>", children, indent),
            Child::Hole(expr) => {
                let leading = self.comments_between(
                    self.offset(child.region.start),
                    self.offset(expr.region.start),
                    indent,
                );
                let trailing = self.comments_between(
                    self.offset(expr.region.end),
                    self.offset(child.region.end),
                    indent,
                );
                let code = self.code(expr.region, indent);
                if leading.is_empty() && trailing.is_empty() {
                    format!("{{{code}}}")
                } else {
                    format!(
                        "{{{leading}\n{}{code}{trailing}\n{}}}",
                        self.pad(indent),
                        self.pad(indent)
                    )
                }
            }
            Child::If {
                branches,
                final_else,
            } => {
                let mut output = String::new();
                let mut start = child.region.start;
                for branch in *branches {
                    if !output.is_empty() {
                        output.push(' ');
                    }
                    output.push_str(&self.head(start, branch.body.region.start, indent));
                    output.push_str(&self.block(branch.body, indent));
                    start = branch.body.region.end;
                }
                if let Some(body) = final_else {
                    output.push(' ');
                    output.push_str(&self.head(start, body.region.start, indent));
                    output.push_str(&self.block(body, indent));
                }
                output
            }
            Child::For { body, empty, .. } => {
                let mut output = self.head(child.region.start, body.region.start, indent);
                output.push_str(&self.block(body, indent));
                if let Some(empty) = empty {
                    output.push(' ');
                    output.push_str(&self.head(body.region.end, empty.region.start, indent));
                    output.push_str(&self.block(empty, indent));
                }
                output
            }
            Child::Match { scrutinee, arms } => {
                let nested = indent + self.options.indent_width;
                let mut output = format!(
                    "{}{{",
                    self.head(child.region.start, scrutinee.region.end, indent)
                );
                let mut previous = self.offset(scrutinee.region.end);
                for arm in *arms {
                    output.push_str(&self.comments_between(
                        previous,
                        self.offset(arm.patterns[0].region.start),
                        nested,
                    ));
                    output.push_str(&format!(
                        "\n{}{}",
                        self.pad(nested),
                        self.head(arm.patterns[0].region.start, arm.body.region.start, nested)
                    ));
                    if let [ChildItem::Child(child)] = arm.body.value.items {
                        if !self.raw(arm.body.region).starts_with('{') {
                            output.push_str(&self.child(child, nested));
                        } else {
                            output.push_str(&self.block(arm.body, nested));
                        }
                    } else {
                        output.push_str(&self.block(arm.body, nested));
                    }
                    output.push(',');
                    previous = self.offset(arm.body.region.end);
                }
                output.push_str(&self.comments_between(
                    previous,
                    self.offset(child.region.end),
                    nested,
                ));
                output.push_str(&format!("\n{}}}", self.pad(indent)));
                output
            }
        }
    }

    fn block(&self, block: &Located<ChildBlock<'_>>, indent: usize) -> String {
        let nested = indent + self.options.indent_width;
        if block.value.items.iter().any(|item| matches!(item, ChildItem::Child(child) if matches!(child.value, Child::Text(_) | Child::Hole(_)))) {
            // Text adjacent to the braces may carry intentional spaces.
            return self.raw(block.region).to_owned();
        }
        let mut output = "{".to_owned();
        let mut previous = self.offset(block.region.start);
        for item in block.value.items {
            let region = match item {
                ChildItem::Stmt(stmt) => stmt.region,
                ChildItem::Child(child) => child.region,
            };
            output.push_str(&self.comments_between(previous, self.offset(region.start), nested));
            output.push_str(&format!("\n{}", self.pad(nested)));
            output.push_str(&match item {
                ChildItem::Stmt(stmt) => self.code(stmt.region, nested),
                ChildItem::Child(child) => self.child(child, nested),
            });
            previous = self.offset(region.end);
        }
        output.push_str(&self.comments_between(previous, self.offset(block.region.end), nested));
        output.push_str(&format!("\n{}}}", self.pad(indent)));
        output
    }

    fn head(&self, start: Position, end: Position, indent: usize) -> String {
        let region = Region::new(start, end);
        let mut code = self.code(region, indent);
        let last = self.offset(start) + self.raw(region).trim_end().len();
        if self.comments.iter().any(|&(_, end)| end == last) {
            code.push('\n');
            code.push_str(&self.pad(indent));
        } else {
            code.push(' ');
        }
        code
    }

    fn code(&self, region: Region, indent: usize) -> String {
        let source = self.raw(region).trim();
        let base = self.offset(region.start) + self.raw(region).len()
            - self.raw(region).trim_start().len();
        // Only insert line breaks at braces. Strings, templates and comments
        // are copied intact; existing newlines keep Alder statement boundaries.
        let mut output = String::new();
        let mut depth = 0usize;
        let mut quoted = None;
        let mut escaped = false;
        let mut comment = false;
        let mut chars = source.chars().peekable();
        let mut position = 0;
        while let Some(ch) = chars.next() {
            let absolute = base + position;
            position += ch.len_utf8();
            if let Some(&(_, end)) = self
                .protected
                .iter()
                .filter(|&&(start, _)| start == absolute)
                .max_by_key(|&&(_, end)| end)
            {
                output.push_str(&self.source[absolute..end]);
                let remaining = end - absolute - ch.len_utf8();
                let mut consumed = 0;
                while consumed < remaining {
                    consumed += chars
                        .next()
                        .expect("protected span within expression")
                        .len_utf8();
                }
                position += consumed;
                continue;
            }
            if comment {
                output.push(ch);
                if ch == '\n' {
                    comment = false;
                    while chars.peek().is_some_and(|c| matches!(c, ' ' | '\t')) {
                        chars.next();
                        position += 1;
                    }
                    output.push_str(&self.pad(indent + depth * self.options.indent_width));
                }
                continue;
            }
            if let Some(quote) = quoted {
                output.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == quote {
                    quoted = None;
                }
                continue;
            }
            match ch {
                '"' | '`' => {
                    quoted = Some(ch);
                    output.push(ch);
                }
                '/' if chars.peek() == Some(&'/') => {
                    comment = true;
                    output.push(ch);
                }
                '{' => {
                    output.push(ch);
                    depth += 1;
                    while chars.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                        chars.next();
                        position += 1;
                    }
                    output.push('\n');
                    output.push_str(&self.pad(indent + depth * self.options.indent_width));
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    while output.ends_with([' ', '\t', '\n']) {
                        output.pop();
                    }
                    output.push('\n');
                    output.push_str(&self.pad(indent + depth * self.options.indent_width));
                    output.push(ch);
                }
                '\n' => {
                    while output.ends_with([' ', '\t']) {
                        output.pop();
                    }
                    output.push('\n');
                    while chars.peek().is_some_and(|c| matches!(c, ' ' | '\t')) {
                        chars.next();
                        position += 1;
                    }
                    output.push_str(&self.pad(indent + depth * self.options.indent_width));
                }
                _ => output.push(ch),
            }
        }
        output
    }
}
