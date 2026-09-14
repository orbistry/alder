use super::*;
use alder_ast::{AttrValue, BindingName, Child, ElementName, Markup, Stmt};
use alder_region::Region;

fn web_type_signature(typ: &alder_ast::Type<'_>) -> String {
    use alder_ast::Type;
    let types = |items: &[&Located<Type<'_>>]| {
        items
            .iter()
            .map(|item| web_type_signature(&item.value))
            .collect::<Vec<_>>()
            .join(",")
    };
    match typ {
        Type::Var { name, args } => format!("{name}[{}]", types(args)),
        Type::Named { reference, args } => {
            format!("{}[{}]", qualified_key(*reference), types(args))
        }
        Type::Unit => "()".to_owned(),
        Type::Tuple(items) => format!("({})", types(items)),
        Type::Fn { params, ret } => {
            format!("fn({}):{}", types(params), web_type_signature(&ret.value))
        }
        Type::Record { fields, ext } => format!(
            "{{{}|{ext:?}}}",
            fields
                .iter()
                .map(|field| format!("{}:{}", field.name, web_type_signature(&field.typ.value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::ErrorRow { tags, ext } => format!(
            "[{}|{ext:?}]",
            tags.iter()
                .map(|tag| format!("{}({})", tag.name, types(tag.args)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                web_type_signature(&target.value)
            }
        },
        Type::Partial { constructor, slots } => format!(
            "{}[{}]",
            qualified_key(*constructor),
            slots
                .iter()
                .map(|slot| match slot {
                    alder_ast::TypeSlot::Hole(index) => format!("_{index}"),
                    alder_ast::TypeSlot::Fixed(typ) => web_type_signature(&typ.value),
                })
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Projection(projection) => format!(
            "{}::{}[{}]",
            qualified_key(projection.trait_ref.trait_.0),
            projection.assoc.name,
            types(projection.trait_ref.args)
        ),
    }
}

fn web_pattern_names<'a>(pattern: &Pattern<'a>, names: &mut Vec<alder_ast::LocalName<'a>>) {
    match pattern {
        Pattern::Bind(BindingName::Local(name)) => names.push(*name),
        Pattern::Alias { pattern, name } => {
            if let BindingName::Local(name) = name {
                names.push(*name);
            }
            web_pattern_names(&pattern.value, names);
        }
        Pattern::Tuple(items)
        | Pattern::Constructor { args: items, .. }
        | Pattern::Tag { args: items, .. } => {
            for item in *items {
                web_pattern_names(&item.value, names);
            }
        }
        Pattern::Array { elements, rest } => {
            for item in *elements {
                web_pattern_names(&item.value, names);
            }
            if let Some(BindingName::Local(name)) = rest.and_then(|rest| rest.name) {
                names.push(name);
            }
        }
        Pattern::Record { fields, .. } | Pattern::ConstructorRecord { fields, .. } => {
            for field in *fields {
                web_pattern_names(&field.pattern.value, names);
            }
        }
        _ => {}
    }
}

fn web_html_function(expression: &Expr<'_>, name: &str) -> bool {
    let Expr::Var { reference, .. } = expression else {
        return false;
    };
    let reference = match reference {
        ValueRef::TopLevel(reference) | ValueRef::Foreign { reference, .. } => reference,
        _ => return false,
    };
    reference.module.package == alder_ast::PackageId::Builtin
        && reference.module.path == ["html"]
        && reference.name == name
}

impl<'src, 'js> Emitter<'src, 'js> {
    pub(super) fn component(
        &mut self,
        component: &alder_ast::ComponentDecl<'src>,
    ) -> Result<Statement<'js>, Error> {
        if self
            .solved
            .and_then(|solved| solved.bindings.get(&component.name))
            .is_some_and(|binding| !binding.dictionary_params.is_empty())
        {
            return Err(Error {
                region: component.body.region,
                message: "polymorphic component dictionaries are not supported; use concrete props",
            });
        }
        self.in_component = false;
        self.web_cells.clear();
        self.web_setup_locals.clear();
        self.web_signature.clear();
        self.web_resources.clear();
        self.web_store_cells.clear();
        let args = (0..component.params.len())
            .map(|index| format!("$a{index}"))
            .collect::<Vec<_>>();
        let mut setup = self.js.vec();
        let stores = self.web_store_dependencies(alder_can::callable_value_dependencies(
            self.home,
            component.params,
            component.body,
        ));
        for store in stores {
            let raw = if store.module == self.home {
                self.js.identifier(&top_name(store))
            } else {
                let imported = self.value_import(store, format!("$store${}", store.name));
                self.js.identifier(&imported)
            };
            let name = self.temp();
            setup.push(self.js.variable(
                VariableDeclarationKind::Const,
                &name,
                Some(self.js.call(self.js.identifier("$webStoreCell"), [raw])),
            ));
            self.web_store_cells.insert(store, name);
        }
        for (param, arg) in component.params.iter().zip(&args) {
            if param.annotation.is_none() {
                return Err(Error {
                    region: param.pattern.region,
                    message: "component props require explicit type annotations",
                });
            }
            if let Pattern::Bind(BindingName::Local(local)) = param.pattern.value {
                self.kernel.insert("$webInput");
                let name = super::super::local_name(local);
                setup.push(self.js.variable(
                    VariableDeclarationKind::Const,
                    &name,
                    Some(self.js.call(
                        self.js.identifier("$webInput"),
                        [self.js.identifier("$owner"), self.js.identifier(arg)],
                    )),
                ));
                self.web_cells.insert(local.id, (name, false));
            } else {
                self.kernel.insert("$webInput");
                let input = self.temp();
                setup.push(self.js.variable(
                    VariableDeclarationKind::Const,
                    &input,
                    Some(self.js.call(
                        self.js.identifier("$webInput"),
                        [self.js.identifier("$owner"), self.js.identifier(arg)],
                    )),
                ));
                self.web_pattern_cells(param.pattern, &input, &mut setup)?;
            }
            self.web_setup_locals
                .extend(alder_can::pattern_local_bindings(self.home, param.pattern));
        }
        for statement in component.body.value.statements {
            let Stmt::Let(decl) = &statement.value else {
                return Err(Error {
                    region: statement.region,
                    message: "component setup supports let bindings only",
                });
            };
            if self.web_resource_binding(decl, &mut setup)? {
                continue;
            }
            let (initial, state) = match decl.value.value {
                Expr::State(initial) => (initial, true),
                _ => (decl.value, false),
            };
            let dependencies = self.web_dependencies(initial);
            if state || !dependencies.is_empty() {
                let Pattern::Bind(BindingName::Local(local)) = decl.pattern.value else {
                    return Err(Error {
                        region: decl.pattern.region,
                        message: "reactive component bindings require a simple local name",
                    });
                };
                let value = if state {
                    self.web_signature
                        .push(self.web_state_signature(local.text, initial));
                    let value = self.expr(initial)?;
                    setup.extend(value.prefix);
                    self.kernel.insert("$webState");
                    self.js.call(
                        self.js.identifier("$webState"),
                        [
                            self.js.identifier("$owner"),
                            value.expr,
                            self.js.string(local.text),
                        ],
                    )
                } else {
                    let compute = self.web_thunk(initial)?;
                    self.kernel.insert("$webMemo");
                    self.js.call(
                        self.js.identifier("$webMemo"),
                        [
                            self.js.identifier("$owner"),
                            self.web_dependency_array(&dependencies),
                            compute,
                        ],
                    )
                };
                let name = super::super::local_name(local);
                setup.push(
                    self.js
                        .variable(VariableDeclarationKind::Const, &name, Some(value)),
                );
                self.web_cells.insert(local.id, (name, state));
            } else {
                setup.extend(self.statement(statement)?);
            }
            self.web_setup_locals
                .extend(alder_can::pattern_local_bindings(self.home, decl.pattern));
        }
        let Some(tail) = component.body.value.tail else {
            return Err(Error {
                region: component.body.region,
                message: "a component must end in markup",
            });
        };
        self.in_component = true;
        if matches!(tail.value, Expr::Markup(_)) {
            let rendered = self.expr(tail)?;
            setup.extend(rendered.prefix);
            setup.push(self.js.return_statement(rendered.expr));
        } else {
            let read = self.web_thunk(tail)?;
            let mut render = self.js.vec();
            render.push(self.web_operation(
                "value",
                [
                    read,
                    self.web_dependency_array(&self.web_dependencies(tail)),
                ],
            ));
            setup.push(self.js.return_statement(self.js.arrow(
                &["$render".to_owned()],
                render,
                false,
            )));
        }
        let setup = self.js.arrow(&["$owner".to_owned()], setup, false);
        self.kernel.insert("$webComponent");
        let component_value = self.js.call(
            self.js.identifier("$webComponent"),
            [
                setup,
                self.js.string(&qualified_key(component.name)),
                self.js.string(&self.web_signature.join("|")),
            ],
        );
        let mut body = self.js.vec();
        body.push(self.js.return_statement(component_value));
        self.in_component = false;
        self.web_cells.clear();
        self.web_setup_locals.clear();
        self.web_resources.clear();
        self.web_store_cells.clear();
        Ok(self
            .js
            .function(&top_name(component.name), &args, body, false))
    }

    fn web_dependencies(&self, expression: &'src Located<Expr<'src>>) -> Vec<String> {
        let mut dependencies = alder_can::expression_local_dependencies(self.home, expression)
            .iter()
            .filter_map(|id| self.web_cells.get(id).map(|(name, _)| name.clone()))
            .collect::<Vec<_>>();
        for store in self.web_store_dependencies(alder_can::expression_value_dependencies(
            self.home, expression,
        )) {
            if let Some(name) = self.web_store_cells.get(&store) {
                dependencies.push(name.clone());
            }
        }
        dependencies
    }

    fn web_state_signature(&self, name: &str, initial: &Located<Expr<'src>>) -> String {
        let typ = self
            .solved
            .and_then(|solved| solved.state_types.get(&initial.region))
            .map(|typ| web_type_signature(&typ.value))
            .unwrap_or_else(|| format!("{:?}", std::mem::discriminant(&initial.value)));
        format!("state:{name}:{typ}")
    }

    pub(super) fn web_store_dependencies(
        &self,
        values: impl IntoIterator<Item = alder_ast::QualifiedName<'src>>,
    ) -> BTreeSet<alder_ast::QualifiedName<'src>> {
        let mut result = BTreeSet::new();
        for value in values {
            if self.web_stores.contains(&value) {
                result.insert(value);
            }
            if let Some(dependencies) = self.web_value_dependencies.get(&value) {
                result.extend(dependencies.intersection(&self.web_stores).copied());
            }
        }
        result
    }

    pub(super) fn web_store_value(
        &self,
        name: alder_ast::QualifiedName<'src>,
        raw: Expression<'js>,
    ) -> Expression<'js> {
        if !self.web_stores.contains(&name) {
            return raw;
        }
        let cell = if let Some(local) = self.web_store_cells.get(&name) {
            self.js.identifier(local)
        } else {
            self.js.call(self.js.identifier("$webStoreCell"), [raw])
        };
        self.js.member(cell, "value")
    }

    pub(super) fn web_store_declaration(
        &mut self,
        decl: &alder_ast::TopLevelLet<'src>,
        initial: &Located<Expr<'src>>,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        let Pattern::Bind(BindingName::TopLevel(binding)) = decl.pattern.value else {
            return Err(Error {
                region: decl.pattern.region,
                message: "module stores require a simple named binding",
            });
        };
        let annotation = self.solved.and_then(|solved| solved.schemes.get(&binding));
        if annotation.is_some_and(|annotation| !annotation.params.is_empty()) {
            return Err(Error {
                region: decl.pattern.region,
                message: "module stores require a concrete type; add a type annotation",
            });
        }
        let signature = annotation
            .map(|annotation| web_type_signature(&annotation.typ.value))
            .unwrap_or_else(|| format!("{:?}", std::mem::discriminant(&initial.value)));
        let value = self.expr(initial)?;
        let mut initialize = value.prefix;
        initialize.push(self.js.return_statement(value.expr));
        self.kernel.insert("$webStore");
        let mut body = self.js.vec();
        body.push(self.js.variable(
            VariableDeclarationKind::Const,
            &top_name(binding),
            Some(self.js.call(
                self.js.identifier("$webStore"),
                [
                    self.js.string(&module_specifier(binding.module)),
                    self.js.string(binding.name),
                    self.js.arrow(&[], initialize, false),
                    self.js.string(&signature),
                ],
            )),
        ));
        Ok(body)
    }

    fn web_resource_binding(
        &mut self,
        decl: &alder_ast::LocalLet<'src>,
        setup: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<bool, Error> {
        let Expr::Call {
            function,
            arguments,
            ..
        } = &decl.value.value
        else {
            return Ok(false);
        };
        if !web_html_function(&function.value, "resource") {
            return Ok(false);
        }
        let Pattern::Bind(BindingName::Local(local)) = decl.pattern.value else {
            return Err(Error {
                region: decl.pattern.region,
                message: "resource bindings require a simple local name",
            });
        };
        let [load] = *arguments else {
            return Err(Error {
                region: decl.value.region,
                message: "resource requires a loader function",
            });
        };
        let dependencies = self.web_dependencies(load);
        let previous = std::mem::replace(&mut self.in_resource_loader, true);
        let loader = self.expr(load);
        self.in_resource_loader = previous;
        let loader = loader?;
        setup.extend(loader.prefix);
        self.kernel.insert("$webResource");
        let name = super::super::local_name(local);
        setup.push(self.js.variable(
            VariableDeclarationKind::Const,
            &name,
            Some(self.js.call(
                self.js.identifier("$webResource"),
                [
                    self.js.identifier("$owner"),
                    self.web_dependency_array(&dependencies),
                    loader.expr,
                    self.js.string(local.text),
                ],
            )),
        ));
        self.web_signature.push(format!("resource:{}", local.text));
        self.web_cells.insert(local.id, (name, false));
        self.web_resources.insert(local.id);
        self.web_setup_locals.insert(local.id);
        Ok(true)
    }

    pub(super) fn web_resource_call(
        &mut self,
        function: &Located<Expr<'src>>,
        arguments: &[&Located<Expr<'src>>],
        region: Region,
    ) -> Result<Option<Value<'js>>, Error> {
        if web_html_function(&function.value, "resource") {
            return Err(Error {
                region,
                message: "resource must initialize a direct component or directive let binding",
            });
        }
        let operation = if web_html_function(&function.value, "refresh") {
            "$webResourceRefresh"
        } else if web_html_function(&function.value, "cancel") {
            "$webResourceCancel"
        } else {
            return Ok(None);
        };
        if self.web_computation || self.in_resource_loader {
            return Err(Error {
                region,
                message: "reactive computations cannot refresh or cancel resources; use an event handler",
            });
        }
        let [argument] = arguments else {
            return Err(Error {
                region,
                message: "resource operations require one resource binding",
            });
        };
        let Expr::Var {
            reference: ValueRef::Local(local),
            ..
        } = argument.value
        else {
            return Err(Error {
                region: argument.region,
                message: "resource operations require a direct resource binding",
            });
        };
        if !self.web_resources.contains(&local.id) {
            return Err(Error {
                region: argument.region,
                message: "resource operations require a direct resource binding",
            });
        }
        self.kernel.insert(operation);
        let (name, _) = &self.web_cells[&local.id];
        Ok(Some(self.pure(self.js.call(
            self.js.identifier(operation),
            [self.js.identifier(name)],
        ))))
    }

    fn web_dependency_array(&self, dependencies: &[String]) -> Expression<'js> {
        self.js
            .array(dependencies.iter().map(|name| self.js.identifier(name)))
    }

    fn web_thunk(
        &mut self,
        expression: &'src Located<Expr<'src>>,
    ) -> Result<Expression<'js>, Error> {
        let outer = std::mem::replace(&mut self.web_computation, true);
        let value = self.expr(expression);
        self.web_computation = outer;
        let value = value?;
        let mut body = value.prefix;
        body.push(self.js.return_statement(value.expr));
        Ok(self.js.arrow(&[], body, false))
    }

    pub(super) fn web_binding(&self, binding: BindingName<'src>) -> Expression<'js> {
        let value = self.js.identifier(&binding_name(binding));
        if let BindingName::TopLevel(name) = binding {
            return self.web_store_value(name, value);
        }
        if let BindingName::Local(local) = binding
            && let Some((name, _)) = self.web_cells.get(&local.id)
        {
            self.js.member(self.js.identifier(name), "value")
        } else {
            value
        }
    }

    pub(super) fn check_web_assignment(
        &self,
        place: &alder_ast::Place<'src>,
        region: Region,
    ) -> Result<(), Error> {
        if let BindingName::TopLevel(name) = place.root
            && self.web_stores.contains(&name)
        {
            if self.in_resource_loader {
                return Err(Error {
                    region,
                    message: "resource loaders cannot mutate module stores; update stores from an event handler",
                });
            }
            if !place.steps.is_empty() {
                return Err(Error {
                    region,
                    message: "replace the whole module store value; nested store mutation does not notify subscribers",
                });
            }
        }
        if let BindingName::Local(local) = place.root
            && let Some((_, state)) = self.web_cells.get(&local.id)
        {
            if self.in_resource_loader {
                return Err(Error {
                    region,
                    message: "resource loaders cannot mutate component state; update state from an event handler",
                });
            }
            if !state {
                return Err(Error {
                    region,
                    message: "derived component bindings are read-only",
                });
            }
            if !place.steps.is_empty() {
                return Err(Error {
                    region,
                    message: "replace the whole state value; nested state mutation is not supported yet",
                });
            }
        } else if let BindingName::Local(local) = place.root
            && self.web_setup_locals.contains(&local.id)
        {
            return Err(Error {
                region,
                message: "component props and setup bindings are read-only; use state for values changed by events",
            });
        }
        Ok(())
    }

    pub(super) fn markup(
        &mut self,
        markup: &Markup<'src>,
        region: Region,
    ) -> Result<Value<'js>, Error> {
        if !self.in_component {
            return Err(Error {
                region,
                message: "executable markup is supported only as a component's final expression",
            });
        }
        let mut body = self.js.vec();
        match markup {
            Markup::Element(element) => self.web_element(element, &mut body)?,
            Markup::Fragment(children) => self.web_children(children, &mut body)?,
        }
        Ok(self.pure(self.js.arrow(&["$render".to_owned()], body, false)))
    }

    fn web_operation(
        &self,
        name: &str,
        args: impl IntoIterator<Item = Expression<'js>>,
    ) -> Statement<'js> {
        self.js.expression_statement(
            self.js
                .call(self.js.member(self.js.identifier("$render"), name), args),
        )
    }

    fn web_element(
        &mut self,
        element: &alder_ast::Element<'src>,
        body: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<(), Error> {
        let ElementName::Tag(tag) = element.name.value else {
            return self.web_component_element(element, body);
        };
        if matches!(tag, "script" | "style") {
            return Err(Error {
                region: element.name.region,
                message: "script and style content requires the asset pipeline; executable inline markup is not supported",
            });
        }
        body.push(self.web_operation("open", [self.js.string(tag)]));
        for attr in element.attrs {
            let name = attr.name.value;
            if name.starts_with("on") && name.as_bytes().get(2).is_some_and(u8::is_ascii_uppercase)
            {
                let Some(AttrValue::Expr(expression)) = attr.value else {
                    return Err(Error {
                        region: attr.name.region,
                        message: "event attributes require a handler expression",
                    });
                };
                let outer = std::mem::replace(&mut self.in_component, false);
                let handler = self.expr(expression);
                self.in_component = outer;
                let handler = handler?;
                let mut read = handler.prefix;
                read.push(self.js.return_statement(handler.expr));
                body.push(self.web_operation(
                    "eventValue",
                    [
                        self.js.string(&name[2..].to_ascii_lowercase()),
                        self.js.arrow(&[], read, false),
                        self.web_dependency_array(&self.web_dependencies(expression)),
                    ],
                ));
                continue;
            }
            let (read, dependencies) = match attr.value {
                Some(AttrValue::Expr(expression)) => (
                    self.web_thunk(expression)?,
                    self.web_dependencies(expression),
                ),
                value => {
                    let value = match value {
                        Some(AttrValue::Str(value)) => self.js.string(value.value),
                        None => self.js.boolean(true),
                        _ => unreachable!(),
                    };
                    let mut statements = self.js.vec();
                    statements.push(self.js.return_statement(value));
                    (self.js.arrow(&[], statements, false), vec![])
                }
            };
            body.push(self.web_operation(
                "attr",
                [
                    self.js.string(name),
                    read,
                    self.web_dependency_array(&dependencies),
                ],
            ));
        }
        self.web_children(element.children, body)?;
        body.push(self.web_operation("close", []));
        Ok(())
    }

    fn web_children(
        &mut self,
        children: &[&Located<Child<'src>>],
        body: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<(), Error> {
        for child in children {
            match &child.value {
                Child::Element(element) => self.web_element(element, body)?,
                Child::Fragment(children) => self.web_children(children, body)?,
                Child::Text(text) => {
                    let mut statements = self.js.vec();
                    statements.push(self.js.return_statement(self.js.string(text)));
                    body.push(self.web_operation(
                        "text",
                        [self.js.arrow(&[], statements, false), self.js.array([])],
                    ));
                }
                Child::Hole(expression) => {
                    let read = self.web_thunk(expression)?;
                    let dependencies = self.web_dependencies(expression);
                    body.push(
                        self.web_operation(
                            "value",
                            [read, self.web_dependency_array(&dependencies)],
                        ),
                    );
                }
                Child::If {
                    branches,
                    final_else,
                } => self.web_if(branches, *final_else, body)?,
                Child::For {
                    pattern,
                    iter,
                    key,
                    body: block,
                    empty,
                } => self.web_for(pattern, iter, *key, block, *empty, body)?,
                Child::Match { scrutinee, arms } => self.web_match(scrutinee, arms, body)?,
            }
        }
        Ok(())
    }

    fn web_component_element(
        &mut self,
        element: &alder_ast::Element<'src>,
        body: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<(), Error> {
        self.web_signature
            .push(format!("component:{:?}", element.name.value));
        let reference = element.component_value.expect("canonical component value");
        let factory = self.expr(reference)?;
        body.extend(factory.prefix);
        let mut properties = self.js.vec();
        let mut compute = self.js.vec();
        let mut dependencies = Vec::new();
        if let Some(defaults) = self
            .solved
            .and_then(|solved| solved.omitted_record_fields.get(&element.name.region))
        {
            for name in defaults {
                properties.push(self.js.property(
                    name,
                    self.js.builder.expression_null_literal(oxc_span::SPAN),
                ));
            }
        }
        for attr in element.attrs {
            let mut value = match attr.value {
                Some(AttrValue::Expr(expression)) => {
                    dependencies.extend(self.web_dependencies(expression));
                    let value = self.expr(expression)?;
                    compute.extend(value.prefix);
                    value.expr
                }
                Some(AttrValue::Str(value)) => self.js.string(value.value),
                None => self.js.boolean(true),
            };
            for _ in 0..self
                .solved
                .and_then(|solved| solved.field_lifts.get(&attr.name.region))
                .copied()
                .unwrap_or_default()
            {
                self.kernel.insert("$optionSome");
                value = self.js.call(self.js.identifier("$optionSome"), [value]);
            }
            properties.push(self.js.property(attr.name.value, value));
        }
        if !element.children.is_empty() {
            let mut render = self.js.vec();
            self.web_children(element.children, &mut render)?;
            let mut setup = self.js.vec();
            setup.push(self.js.return_statement(self.js.arrow(
                &["$render".to_owned()],
                render,
                false,
            )));
            self.kernel.insert("$webComponent");
            properties.push(self.js.property(
                "children",
                self.js.call(
                    self.js.identifier("$webComponent"),
                    [self.js.arrow(&["$owner".to_owned()], setup, false)],
                ),
            ));
        }
        compute.push(self.js.return_statement(self.js.object(properties)));
        self.kernel.insert("$webProps");
        let props = self.js.call(
            self.js.identifier("$webProps"),
            [
                self.js.identifier("$owner"),
                self.web_dependency_array(&dependencies),
                self.js.arrow(&[], compute, false),
            ],
        );
        let instance = self.js.call(factory.expr, [props]);
        let name = self.temp();
        body.push(
            self.js
                .variable(VariableDeclarationKind::Const, &name, Some(instance)),
        );
        let mut read = self.js.vec();
        read.push(self.js.return_statement(self.js.identifier(&name)));
        body.push(self.web_operation(
            "value",
            [self.js.arrow(&[], read, false), self.js.array([])],
        ));
        Ok(())
    }

    fn web_pattern_cells(
        &mut self,
        pattern: &Located<Pattern<'src>>,
        input: &str,
        setup: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<(), Error> {
        let mut names = Vec::new();
        web_pattern_names(&pattern.value, &mut names);
        for local in names {
            let mut compute = self.js.vec();
            let raw = self.temp();
            compute.push(self.js.variable(
                VariableDeclarationKind::Const,
                &raw,
                Some(self.js.member(self.js.identifier(input), "value")),
            ));
            self.checked_bind_pattern(pattern, &raw, &mut compute)?;
            let name = super::super::local_name(local);
            compute.push(self.js.return_statement(self.js.identifier(&name)));
            let cell = format!("{name}$cell");
            self.kernel.insert("$webMemo");
            setup.push(self.js.variable(
                VariableDeclarationKind::Const,
                &cell,
                Some(self.js.call(
                    self.js.identifier("$webMemo"),
                    [
                        self.js.identifier("$owner"),
                        self.js.array([self.js.identifier(input)]),
                        self.js.arrow(&[], compute, false),
                    ],
                )),
            ));
            self.web_cells.insert(local.id, (cell, false));
        }
        Ok(())
    }

    fn web_child_setup(
        &mut self,
        block: &alder_ast::ChildBlock<'src>,
        pattern: Option<&Located<Pattern<'src>>>,
    ) -> Result<Expression<'js>, Error> {
        let saved = self.web_cells.clone();
        let saved_locals = self.web_setup_locals.clone();
        let saved_resources = self.web_resources.clone();
        let mut setup = self.js.vec();
        if let Some(pattern) = pattern {
            self.web_pattern_cells(pattern, "$item", &mut setup)?;
        }
        let mut render = self.js.vec();
        for item in block.items {
            match item {
                alder_ast::ChildItem::Stmt(statement) => {
                    if let Stmt::Let(decl) = &statement.value {
                        if self.web_resource_binding(decl, &mut setup)? {
                            continue;
                        }
                        let (initial, state) = match decl.value.value {
                            Expr::State(initial) => (initial, true),
                            _ => (decl.value, false),
                        };
                        let dependencies = self.web_dependencies(initial);
                        if (state || !dependencies.is_empty())
                            && let Pattern::Bind(BindingName::Local(local)) = decl.pattern.value
                        {
                            let value = if state {
                                self.web_signature
                                    .push(self.web_state_signature(local.text, initial));
                                let initial = self.expr(initial)?;
                                setup.extend(initial.prefix);
                                self.kernel.insert("$webState");
                                self.js.call(
                                    self.js.identifier("$webState"),
                                    [
                                        self.js.identifier("$owner"),
                                        initial.expr,
                                        self.js.string(local.text),
                                    ],
                                )
                            } else {
                                let compute = self.web_thunk(initial)?;
                                self.kernel.insert("$webMemo");
                                self.js.call(
                                    self.js.identifier("$webMemo"),
                                    [
                                        self.js.identifier("$owner"),
                                        self.web_dependency_array(&dependencies),
                                        compute,
                                    ],
                                )
                            };
                            let name = super::super::local_name(local);
                            setup.push(self.js.variable(
                                VariableDeclarationKind::Const,
                                &name,
                                Some(value),
                            ));
                            self.web_cells.insert(local.id, (name, state));
                        } else {
                            setup.extend(self.statement(statement)?);
                        }
                        self.web_setup_locals
                            .extend(alder_can::pattern_local_bindings(self.home, decl.pattern));
                    } else {
                        setup.extend(self.statement(statement)?);
                    }
                }
                alder_ast::ChildItem::Child(child) => self.web_children(&[*child], &mut render)?,
            }
        }
        setup.push(
            self.js
                .return_statement(self.js.arrow(&["$render".to_owned()], render, false)),
        );
        self.web_cells = saved;
        self.web_setup_locals = saved_locals;
        self.web_resources = saved_resources;
        Ok(self
            .js
            .arrow(&["$owner".to_owned(), "$item".to_owned()], setup, false))
    }

    fn web_if(
        &mut self,
        branches: &[alder_ast::ChildIfBranch<'src>],
        final_else: Option<&Located<alder_ast::ChildBlock<'src>>>,
        body: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<(), Error> {
        self.web_signature
            .push(format!("if:{}:{}", branches.len(), final_else.is_some()));
        let mut select = self.js.vec();
        let mut dependencies = Vec::new();
        for (index, branch) in branches.iter().enumerate() {
            dependencies.extend(self.web_dependencies(branch.condition));
            let condition = self.expr(branch.condition)?;
            select.extend(condition.prefix);
            let setup = self.web_child_setup(&branch.body.value, None)?;
            let mut chosen = self.js.vec();
            chosen.push(self.js.return_statement(self.js.array([
                self.js.number(index as f64),
                self.js.undefined(),
                setup,
            ])));
            select.push(self.js.if_statement(condition.expr, chosen, None));
        }
        let otherwise = if let Some(block) = final_else {
            let setup = self.web_child_setup(&block.value, None)?;
            self.js.array([
                self.js.number(branches.len() as f64),
                self.js.undefined(),
                setup,
            ])
        } else {
            self.js.undefined()
        };
        select.push(self.js.return_statement(otherwise));
        body.push(self.web_operation(
            "region",
            [
                self.js.arrow(&[], select, false),
                self.web_dependency_array(&dependencies),
            ],
        ));
        Ok(())
    }

    fn web_for(
        &mut self,
        pattern: &Located<Pattern<'src>>,
        iter: &'src Located<Expr<'src>>,
        key: Option<&'src Located<Expr<'src>>>,
        block: &Located<alder_ast::ChildBlock<'src>>,
        empty: Option<&Located<alder_ast::ChildBlock<'src>>>,
        body: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<(), Error> {
        self.web_signature
            .push(format!("for:{}:{}", key.is_some(), empty.is_some()));
        let read = self.web_thunk(iter)?;
        let mut dependencies = self.web_dependencies(iter);
        if let Some(key) = key {
            dependencies.extend(self.web_dependencies(key));
        }
        let mut key_body = self.js.vec();
        self.checked_bind_pattern(pattern, "$value", &mut key_body)?;
        let key = if let Some(key) = key {
            let key = self.expr(key)?;
            key_body.extend(key.prefix);
            key.expr
        } else {
            self.js.identifier("$index")
        };
        key_body.push(self.js.return_statement(key));
        let key = self
            .js
            .arrow(&["$value".to_owned(), "$index".to_owned()], key_body, false);
        let setup = self.web_child_setup(&block.value, Some(pattern))?;
        let empty = if let Some(empty) = empty {
            self.web_child_setup(&empty.value, None)?
        } else {
            self.js.undefined()
        };
        body.push(self.web_operation(
            "list",
            [
                read,
                self.web_dependency_array(&dependencies),
                key,
                setup,
                empty,
            ],
        ));
        Ok(())
    }

    fn web_match(
        &mut self,
        scrutinee: &'src Located<Expr<'src>>,
        arms: &[alder_ast::ChildMatchArm<'src>],
        body: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Result<(), Error> {
        self.web_signature.push(format!("match:{}", arms.len()));
        let mut dependencies = self.web_dependencies(scrutinee);
        let value = self.expr(scrutinee)?;
        let mut select = value.prefix;
        let name = self.temp();
        select.push(
            self.js
                .variable(VariableDeclarationKind::Const, &name, Some(value.expr)),
        );
        let mut index = 0;
        for arm in arms {
            if let Some(guard) = arm.guard {
                dependencies.extend(self.web_dependencies(guard));
            }
            for pattern in arm.patterns {
                let (test, mut matched) = self.prepare_pattern(pattern, &name)?;
                select.extend(test.prefix);
                let setup = self.web_child_setup(&arm.body.value, Some(pattern))?;
                let result = self.js.array([
                    self.js.number(index as f64),
                    self.js.identifier(&name),
                    setup,
                ]);
                index += 1;
                if let Some(guard) = arm.guard {
                    let guard = self.expr(guard)?;
                    matched.extend(guard.prefix);
                    let mut chosen = self.js.vec();
                    chosen.push(self.js.return_statement(result));
                    matched.push(self.js.if_statement(guard.expr, chosen, None));
                } else {
                    matched.push(self.js.return_statement(result));
                }
                select.push(self.js.if_statement(test.expr, matched, None));
            }
        }
        select.push(self.js.return_statement(self.js.undefined()));
        body.push(self.web_operation(
            "region",
            [
                self.js.arrow(&[], select, false),
                self.web_dependency_array(&dependencies),
            ],
        ));
        Ok(())
    }
}
