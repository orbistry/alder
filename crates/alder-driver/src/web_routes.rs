//! Deterministic, I/O-free filesystem route discovery and semantic headers.
//!
//! Paths are relative to the supplied source root; diagnostics retain the original
//! URI. Callers discover files through their normal FileSource, then supply solved
//! interfaces and evaluated public options in separate passes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::interface::{
    InterfaceFile, OwnedKind, OwnedLocatedType, OwnedModuleId, OwnedPackageId, OwnedPublicTypeBody,
    OwnedQualifiedName, OwnedRecordField, OwnedType, OwnedTypeDecl,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDiagnostic {
    pub source_uri: String,
    pub related_uri: Option<String>,
    pub message: String,
}

impl std::fmt::Display for RouteDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.source_uri, self.message)?;
        if let Some(uri) = &self.related_uri {
            write!(f, " (also {uri})")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Segment {
    Static(String),
    Param(String),
    Optional(String),
    Rest(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub universal: Option<SourceFile>,
    pub server: Option<SourceFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// Filesystem identity, including groups (e.g. `/(app)/users/[id]`).
    pub id: String,
    pub segments: Vec<Segment>,
    pub page: Option<SourceFile>,
    pub page_server: Option<SourceFile>,
    pub endpoint: Option<SourceFile>,
    /// Outer to inner; server load precedes universal load at each level.
    pub layouts: Vec<Layout>,
    /// Outer to inner; the last boundary is the nearest fallback.
    pub errors: Vec<SourceFile>,
    /// Outer to inner, server before universal at the same directory.
    pub option_sources: Vec<SourceFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteManifest {
    /// Precedence: static, required parameter, optional parameter, rest.
    pub routes: Vec<Route>,
    pub hooks_server: Option<SourceFile>,
    pub hooks_client: Option<SourceFile>,
    pub remotes: Vec<SourceFile>,
}

#[derive(Default)]
struct Directory {
    files: BTreeMap<String, SourceFile>,
}

fn diagnostic(file: &SourceFile, message: impl Into<String>) -> RouteDiagnostic {
    RouteDiagnostic {
        source_uri: file.uri.clone(),
        related_uri: None,
        message: message.into(),
    }
}

/// Recognizes `src/hooks.*` (and the documented legacy `src/routes/hooks.*`),
/// remote modules anywhere below src, and route files below src/routes.
pub fn discover(
    source_root: &Path,
    files: &[SourceFile],
) -> Result<RouteManifest, Vec<RouteDiagnostic>> {
    // File URLs discard Windows verbatim prefixes. Normalize both sides of
    // the source-root comparison, including callers passing canonical paths.
    let source_root = url::Url::from_file_path(source_root)
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
        .unwrap_or_else(|| source_root.to_path_buf());
    let mut manifest = RouteManifest::default();
    let mut directories: BTreeMap<String, Directory> = BTreeMap::new();
    let mut errors = Vec::new();
    let mut sorted = files.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path).then(a.uri.cmp(&b.uri)));
    for file in sorted {
        let path = url::Url::from_file_path(&file.path)
            .ok()
            .and_then(|uri| uri.to_file_path().ok())
            .unwrap_or_else(|| file.path.clone());
        let Ok(relative) = path.strip_prefix(&source_root) else {
            continue;
        };
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            errors.push(diagnostic(&file, "source path must be normalized"));
            continue;
        }
        let Some(parts) = relative
            .components()
            .map(|part| part.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()
        else {
            errors.push(diagnostic(&file, "route paths must be UTF-8"));
            continue;
        };
        // Route syntax uses URL separators, not host filesystem separators.
        let relative = parts.join("/");
        let relative = relative.as_str();
        let hook = match relative {
            "hooks.server.ald" | "routes/hooks.server.ald" => Some(&mut manifest.hooks_server),
            "hooks.client.ald" | "routes/hooks.client.ald" => Some(&mut manifest.hooks_client),
            _ => None,
        };
        if let Some(slot) = hook {
            if let Some(previous) = slot.as_ref() {
                let mut error = diagnostic(&file, "duplicate application hook");
                error.related_uri = Some(previous.uri.clone());
                errors.push(error);
            } else {
                *slot = Some(file);
            }
            continue;
        }
        if relative.ends_with(".remote.ald") {
            manifest.remotes.push(file);
            continue;
        }
        let Some(relative) = relative.strip_prefix("routes/") else {
            continue;
        };
        let (directory, name) = relative.rsplit_once('/').unwrap_or(("", relative));
        if !name.starts_with('+') {
            continue;
        }
        if !matches!(
            name,
            "+page.ald"
                | "+page.server.ald"
                | "+layout.ald"
                | "+layout.server.ald"
                | "+server.ald"
                | "+error.ald"
        ) {
            errors.push(diagnostic(&file, format!("unknown route file `{name}`")));
            continue;
        }
        if let Err(message) = parse_segments(directory) {
            errors.push(diagnostic(&file, message));
            continue;
        }
        let slot = directories.entry(directory.into()).or_default();
        if let Some(previous) = slot.files.insert(name.into(), file.clone()) {
            let mut error = diagnostic(&file, "duplicate route source");
            error.related_uri = Some(previous.uri);
            errors.push(error);
        }
    }
    for (directory, node) in &directories {
        let get = |name: &str| node.files.get(name).cloned();
        let page = get("+page.ald");
        let page_server = get("+page.server.ald");
        let endpoint = get("+server.ald");
        if page.is_none() && page_server.is_none() && endpoint.is_none() {
            continue;
        }
        let mut route = Route {
            id: format!("/{directory}"),
            segments: parse_segments(directory).expect("validated"),
            page,
            page_server,
            endpoint,
            layouts: vec![],
            errors: vec![],
            option_sources: vec![],
        };
        let mut ancestors = vec![String::new()];
        let mut prefix = String::new();
        for part in directory.split('/').filter(|s| !s.is_empty()) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            ancestors.push(prefix.clone());
        }
        for ancestor in ancestors {
            let Some(node) = directories.get(&ancestor) else {
                continue;
            };
            let layout = Layout {
                universal: node.files.get("+layout.ald").cloned(),
                server: node.files.get("+layout.server.ald").cloned(),
            };
            route
                .option_sources
                .extend(layout.server.iter().chain(layout.universal.iter()).cloned());
            if layout.universal.is_some() || layout.server.is_some() {
                route.layouts.push(layout);
            }
            route.errors.extend(node.files.get("+error.ald").cloned());
        }
        route
            .option_sources
            .extend(route.page_server.iter().chain(route.page.iter()).cloned());
        manifest.routes.push(route);
    }
    manifest.routes.sort_by(|a, b| {
        precedence(&a.segments)
            .cmp(&precedence(&b.segments))
            .then(a.id.cmp(&b.id))
    });
    for (index, route) in manifest.routes.iter().enumerate() {
        for other in &manifest.routes[..index] {
            if shape(&route.segments) == shape(&other.segments) {
                let mut error = diagnostic(
                    route.source(),
                    format!(
                        "ambiguous route `{}` has the same URL pattern as `{}`",
                        route.id, other.id
                    ),
                );
                error.related_uri = Some(other.source().uri.clone());
                errors.push(error);
            }
        }
    }
    if errors.is_empty() {
        Ok(manifest)
    } else {
        Err(errors)
    }
}

fn parse_segments(directory: &str) -> Result<Vec<Segment>, String> {
    let mut segments = Vec::new();
    let mut names = BTreeSet::new();
    let mut has_rest = false;
    for part in directory.split('/').filter(|part| !part.is_empty()) {
        if part.starts_with('(')
            && part.ends_with(')')
            && part.len() > 2
            && !part[1..part.len() - 1].contains(['(', ')', '[', ']'])
        {
            continue;
        }
        let segment = if let Some(name) = part.strip_prefix("[[").and_then(|s| s.strip_suffix("]]"))
        {
            Segment::Optional(name.into())
        } else if let Some(name) = part.strip_prefix("[...").and_then(|s| s.strip_suffix(']')) {
            Segment::Rest(name.into())
        } else if let Some(name) = part.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Segment::Param(name.into())
        } else if part.contains(['[', ']', '(', ')', '?', '#', '%', '\\'])
            || part == "."
            || part == ".."
        {
            return Err(format!("malformed or unsupported route segment `{part}`"));
        } else {
            Segment::Static(part.into())
        };
        if let Segment::Param(name) | Segment::Optional(name) | Segment::Rest(name) = &segment {
            if name.is_empty()
                || !name.bytes().enumerate().all(|(i, c)| {
                    c == b'_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit()
                })
            {
                return Err(format!("invalid parameter name `{name}`"));
            }
            if !names.insert(name.clone()) {
                return Err(format!("duplicate route parameter `{name}`"));
            }
        }
        if matches!(segment, Segment::Rest(_)) {
            if has_rest {
                return Err("a route may contain only one rest parameter".into());
            }
            has_rest = true;
        } else if has_rest && matches!(segment, Segment::Optional(_)) {
            return Err("an optional parameter cannot follow a rest parameter".into());
        }
        segments.push(segment);
    }
    Ok(segments)
}

fn precedence(segments: &[Segment]) -> Vec<(u8, String)> {
    segments
        .iter()
        .map(|segment| match segment {
            Segment::Static(s) => (0, s.clone()),
            Segment::Param(_) => (1, String::new()),
            Segment::Optional(_) => (2, String::new()),
            Segment::Rest(_) => (3, String::new()),
        })
        .collect()
}

fn shape(segments: &[Segment]) -> Vec<(u8, String)> {
    precedence(segments)
}

pub type Params = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteMatch<'a> {
    pub route: &'a Route,
    pub params: Params,
}

/// The reversible hexadecimal identifier is stable under route insertion and
/// cannot collide after punctuation, case, or Unicode normalization. Consumers
/// may display `route_id` while exposing `export_name` in the generated module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteLink {
    pub export_name: String,
    pub route_id: String,
    pub params: OwnedType,
}

impl RouteManifest {
    pub fn route_links(&self) -> Vec<RouteLink> {
        self.routes
            .iter()
            .map(|route| {
                let mut export_name = String::from("route");
                for byte in route.id.bytes() {
                    use std::fmt::Write;
                    write!(&mut export_name, "_{byte:02x}").expect("string write");
                }
                RouteLink {
                    export_name,
                    route_id: route.id.clone(),
                    params: route.params_type(),
                }
            })
            .collect()
    }

    /// A record of route link functions, each accepting exactly that route's
    /// parameter record. Runtime link functions use the same `route_links` map.
    pub fn routes_type(&self) -> OwnedType {
        record(
            self.route_links()
                .into_iter()
                .map(|link| {
                    (
                        link.export_name,
                        located(OwnedType::Fn {
                            params: vec![located(link.params)],
                            ret: Box::new(located(builtin("String"))),
                        }),
                    )
                })
                .collect(),
        )
    }

    pub fn match_path(&self, pathname: &str) -> Result<Option<RouteMatch<'_>>, String> {
        let parts = decode_path(pathname)?;
        Ok(self.routes.iter().find_map(|route| {
            match_segments(&route.segments, &parts).map(|params| RouteMatch { route, params })
        }))
    }
}

impl Route {
    pub fn source(&self) -> &SourceFile {
        self.page
            .as_ref()
            .or(self.page_server.as_ref())
            .or(self.endpoint.as_ref())
            .expect("route has source")
    }

    pub fn match_path(&self, pathname: &str) -> Result<Option<Params>, String> {
        Ok(match_segments(&self.segments, &decode_path(pathname)?))
    }

    /// Returns the encoded canonical location for a matching pathname. Ignore
    /// preserves the caller's trailing slash but still normalizes URL escaping.
    pub fn canonical_path(
        &self,
        pathname: &str,
        trailing_slash: TrailingSlash,
    ) -> Result<Option<String>, String> {
        let Some(params) = self.match_path(pathname)? else {
            return Ok(None);
        };
        let policy = if trailing_slash == TrailingSlash::Ignore && pathname.ends_with('/') {
            TrailingSlash::Always
        } else {
            trailing_slash
        };
        self.href(&params, policy).map(Some)
    }

    pub fn href(&self, params: &Params, trailing_slash: TrailingSlash) -> Result<String, String> {
        let expected: BTreeSet<_> = self
            .segments
            .iter()
            .filter_map(|s| match s {
                Segment::Param(n) | Segment::Optional(n) | Segment::Rest(n) => Some(n.as_str()),
                _ => None,
            })
            .collect();
        if let Some(name) = params.keys().find(|n| !expected.contains(n.as_str())) {
            return Err(format!("unknown route parameter `{name}`"));
        }
        let mut parts = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Static(s) => parts.push(encode(s)),
                Segment::Param(name) => {
                    let value = params
                        .get(name)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| format!("missing route parameter `{name}`"))?;
                    validate_value(value)?;
                    parts.push(encode(value));
                }
                Segment::Optional(name) => {
                    if let Some(value) = params.get(name).filter(|s| !s.is_empty()) {
                        validate_value(value)?;
                        parts.push(encode(value));
                    }
                }
                Segment::Rest(name) => {
                    if let Some(value) = params.get(name).filter(|s| !s.is_empty()) {
                        for part in value.split('/') {
                            validate_value(part)?;
                            parts.push(encode(part));
                        }
                    }
                }
            }
        }
        let mut path = format!("/{}", parts.join("/"));
        if trailing_slash == TrailingSlash::Always && path != "/" {
            path.push('/');
        }
        let matched = match_segments(&self.segments, &decode_path(&path)?)
            .ok_or_else(|| "parameters do not form a matching route URL".to_owned())?;
        if params
            .iter()
            .any(|(name, value)| !value.is_empty() && matched.get(name) != Some(value))
        {
            return Err("optional parameters produce an ambiguous route URL".into());
        }
        Ok(path)
    }
}

fn validate_value(value: &str) -> Result<(), String> {
    if value.is_empty() || matches!(value, "." | "..") || value.contains(['/', '\\', '\0']) {
        Err("route parameters must not contain empty, dot, slash, backslash or NUL segments".into())
    } else {
        Ok(())
    }
}

fn encode(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(&mut result, "%{byte:02X}").expect("string write");
        }
    }
    result
}

fn decode_path(path: &str) -> Result<Vec<String>, String> {
    if !path.starts_with('/') || path.contains(['?', '#', '\\']) {
        return Err("expected an absolute URL pathname without query or fragment".into());
    }
    let mut parts = Vec::new();
    let inner = path.strip_prefix('/').expect("validated");
    let inner = inner.strip_suffix('/').unwrap_or(inner);
    if inner.is_empty() {
        return if path == "/" {
            Ok(parts)
        } else {
            Err("empty pathname segment".into())
        };
    }
    for part in inner.split('/') {
        let mut bytes = Vec::new();
        let mut input = part.bytes();
        while let Some(byte) = input.next() {
            if byte == b'%' {
                let hi = input.next().and_then(|c| char::from(c).to_digit(16));
                let lo = input.next().and_then(|c| char::from(c).to_digit(16));
                let (Some(hi), Some(lo)) = (hi, lo) else {
                    return Err("invalid percent escape in pathname".into());
                };
                bytes.push((hi * 16 + lo) as u8);
            } else {
                bytes.push(byte);
            }
        }
        let value = String::from_utf8(bytes).map_err(|_| "pathname is not valid UTF-8")?;
        validate_value(&value)?;
        parts.push(value);
    }
    Ok(parts)
}

fn match_segments(segments: &[Segment], parts: &[String]) -> Option<Params> {
    let Some((segment, rest)) = segments.split_first() else {
        return parts.is_empty().then(Params::new);
    };
    match segment {
        Segment::Static(value) => {
            let (first, tail) = parts.split_first()?;
            if value != first {
                return None;
            }
            match_segments(rest, tail)
        }
        Segment::Param(name) => {
            let (first, tail) = parts.split_first()?;
            let mut params = match_segments(rest, tail)?;
            params.insert(name.clone(), first.clone());
            Some(params)
        }
        Segment::Optional(name) => {
            if let Some((first, tail)) = parts.split_first()
                && let Some(mut params) = match_segments(rest, tail)
            {
                params.insert(name.clone(), first.clone());
                return Some(params);
            }
            match_segments(rest, parts)
        }
        Segment::Rest(name) => {
            for count in (0..=parts.len()).rev() {
                if let Some(mut params) = match_segments(rest, &parts[count..]) {
                    params.insert(name.clone(), parts[..count].join("/"));
                    return Some(params);
                }
            }
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrailingSlash {
    #[default]
    Never,
    Always,
    Ignore,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageOptionExports {
    pub prerender: Option<bool>,
    pub ssr: Option<bool>,
    pub csr: Option<bool>,
    pub trailing_slash: Option<TrailingSlash>,
    pub entries: Option<Vec<Params>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageOptions {
    pub prerender: bool,
    pub ssr: bool,
    pub csr: bool,
    pub trailing_slash: TrailingSlash,
    pub entries: Option<Vec<Params>>,
}

impl Default for PageOptions {
    fn default() -> Self {
        Self {
            prerender: false,
            ssr: true,
            csr: true,
            trailing_slash: TrailingSlash::Never,
            entries: None,
        }
    }
}

impl Route {
    /// Export values must be evaluated by the compiler/build host, never scraped
    /// from source text. Map keys are source URIs from `option_sources`.
    pub fn resolve_options(
        &self,
        exports: &BTreeMap<String, PageOptionExports>,
    ) -> Result<PageOptions, RouteDiagnostic> {
        let mut result = PageOptions::default();
        for source in &self.option_sources {
            if let Some(options) = exports.get(&source.uri) {
                if let Some(value) = options.prerender {
                    result.prerender = value;
                }
                if let Some(value) = options.ssr {
                    result.ssr = value;
                }
                if let Some(value) = options.csr {
                    result.csr = value;
                }
                if let Some(value) = options.trailing_slash {
                    result.trailing_slash = value;
                }
                if let Some(value) = &options.entries {
                    result.entries = Some(value.clone());
                }
            }
        }
        if !result.ssr && !result.csr {
            return Err(diagnostic(
                self.source(),
                "ssr and csr cannot both be false",
            ));
        }
        if result.prerender
            && self
                .segments
                .iter()
                .any(|s| !matches!(s, Segment::Static(_)))
            && result.entries.is_none()
        {
            return Err(diagnostic(
                self.source(),
                "prerendering a dynamic route requires exported entries",
            ));
        }
        if let Some(entries) = &result.entries {
            let mut paths = BTreeSet::new();
            for entry in entries {
                let href = self
                    .href(entry, result.trailing_slash)
                    .map_err(|e| diagnostic(self.source(), e))?;
                if !paths.insert(href.clone()) {
                    return Err(diagnostic(
                        self.source(),
                        format!("duplicate prerender entry `{href}`"),
                    ));
                }
            }
        }
        Ok(result)
    }
}

/// Solved semantic types ready to inject as route-local aliases. These preserve
/// nominal package/module identities instead of converting types into strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteTypes {
    pub route_id: String,
    pub params: OwnedType,
    pub load_event: OwnedType,
    pub page_data: OwnedType,
}

impl RouteTypes {
    /// Alias declarations for a generated, explicitly imported route-local
    /// module. Hydrating the containing InterfaceFile makes these ordinary
    /// canonical types; no special names need to enter the inference engine.
    pub fn aliases(&self, module: &OwnedModuleId) -> Vec<OwnedTypeDecl> {
        [
            ("Params", &self.params),
            ("LoadEvent", &self.load_event),
            ("PageData", &self.page_data),
        ]
        .into_iter()
        .map(|(name, typ)| OwnedTypeDecl {
            exported_as: name.into(),
            reference: OwnedQualifiedName {
                module: module.clone(),
                name: name.into(),
            },
            params: vec![],
            result_kind: OwnedKind::Type,
            body: OwnedPublicTypeBody::Alias(Box::new(located(typ.clone()))),
        })
        .collect()
    }
}

fn located(typ: OwnedType) -> OwnedLocatedType {
    OwnedLocatedType {
        region: alder_region::Region::zero(),
        typ,
    }
}
fn builtin(name: &str) -> OwnedType {
    OwnedType::Named {
        reference: OwnedQualifiedName {
            module: OwnedModuleId {
                package: OwnedPackageId::Builtin,
                path: vec![],
            },
            name: name.into(),
        },
        args: vec![],
    }
}
fn record(fields: BTreeMap<String, OwnedLocatedType>) -> OwnedType {
    OwnedType::Record {
        fields: fields
            .into_iter()
            .enumerate()
            .map(|(index, (name, typ))| OwnedRecordField {
                index: index as u16,
                name,
                typ,
            })
            .collect(),
        ext: None,
    }
}

impl Route {
    pub fn params_type(&self) -> OwnedType {
        record(
            self.segments
                .iter()
                .filter_map(|segment| {
                    let (name, optional) = match segment {
                        Segment::Static(_) => return None,
                        Segment::Param(n) | Segment::Rest(n) => (n, false),
                        Segment::Optional(n) => (n, true),
                    };
                    let mut typ = builtin("String");
                    if optional {
                        let mut option = builtin("Option");
                        if let OwnedType::Named { args, .. } = &mut option {
                            args.push(located(typ));
                        }
                        typ = option;
                    }
                    Some((name.clone(), located(typ)))
                })
                .collect(),
        )
    }

    /// Missing interfaces are an error: callers must finish ancestor loads before
    /// generating a child's PageData. Child fields replace parent fields.
    pub fn generate_types(
        &self,
        interfaces: &BTreeMap<String, InterfaceFile>,
    ) -> Result<RouteTypes, RouteDiagnostic> {
        self.generate_types_before(interfaces, None)
    }

    /// Generate ancestor/server data before solving a universal page or layout.
    /// Stops immediately before `pending_uri` so its own interface is not needed.
    /// A second call after solving the universal load yields the final PageData.
    pub fn generate_types_before(
        &self,
        interfaces: &BTreeMap<String, InterfaceFile>,
        pending_uri: Option<&str>,
    ) -> Result<RouteTypes, RouteDiagnostic> {
        let mut fields = BTreeMap::new();
        for source in &self.option_sources {
            if pending_uri == Some(source.uri.as_str()) {
                break;
            }
            let interface = interfaces.get(&source.uri).ok_or_else(|| {
                diagnostic(source, "missing solved interface for route type generation")
            })?;
            let Some(load) = interface
                .values
                .iter()
                .find(|value| value.exported_as == "load")
            else {
                continue;
            };
            let OwnedType::Fn { ret, .. } = &load.scheme.typ.typ else {
                return Err(diagnostic(source, "load must be a function"));
            };
            let mut output = &ret.typ;
            loop {
                match output {
                    OwnedType::Alias { target, .. } => output = &target.typ,
                    OwnedType::Named { reference, args }
                        if reference.module.package == OwnedPackageId::Builtin
                            && matches!(reference.name.as_str(), "Task" | "Result")
                            && !args.is_empty() =>
                    {
                        output = &args[0].typ
                    }
                    _ => break,
                }
            }
            let OwnedType::Record {
                fields: data,
                ext: None,
            } = output
            else {
                return Err(diagnostic(
                    source,
                    "load must return a closed record, optionally wrapped in Task or Result",
                ));
            };
            for field in data {
                fields.insert(field.name.clone(), field.typ.clone());
            }
        }
        let params = self.params_type();
        let load_event = record(BTreeMap::from([("params".into(), located(params.clone()))]));
        Ok(RouteTypes {
            route_id: self.id.clone(),
            params,
            load_event,
            page_data: record(fields),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_roots_and_native_separators_discover_routes() {
        let root = std::env::current_dir()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("src");
        let files = ["+page.ald", "+page.server.ald", "nested/+page.ald"].map(|name| {
            let uri = url::Url::from_file_path(root.join("routes").join(name)).unwrap();
            SourceFile {
                path: uri.to_file_path().unwrap(),
                uri: uri.to_string(),
            }
        });
        let tree = discover(&root, &files).unwrap();
        assert_eq!(tree.routes.len(), 2);
        let route = tree.routes.iter().find(|route| route.id == "/").unwrap();
        assert!(route.page.is_some());
        assert!(route.page_server.is_some());
    }

    fn source(path: &str) -> SourceFile {
        SourceFile {
            path: Path::new("/app/src").join(path),
            uri: format!("file:///app/src/{path}"),
        }
    }
    fn manifest(paths: &[&str]) -> RouteManifest {
        discover(
            Path::new("/app/src"),
            &paths.iter().map(|p| source(p)).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    fn params(values: &[(&str, &str)]) -> Params {
        values
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn discovers_all_conventions_and_inherits_layouts_and_errors() {
        let tree = manifest(&[
            "routes/+layout.server.ald",
            "routes/+layout.ald",
            "routes/+error.ald",
            "routes/+page.ald",
            "routes/(app)/users/+layout.ald",
            "routes/(app)/users/+error.ald",
            "routes/(app)/users/[id]/+page.ald",
            "routes/(app)/users/[id]/+page.server.ald",
            "routes/(app)/users/[id]/+server.ald",
            "routes/api/health/+server.ald",
            "hooks.server.ald",
            "hooks.client.ald",
            "lib/users.remote.ald",
            "routes/readme.ald",
        ]);
        assert_eq!(tree.routes.len(), 3);
        let matched = tree.match_path("/users/42").unwrap().unwrap();
        assert_eq!(matched.route.id, "/(app)/users/[id]");
        assert_eq!(matched.params, params(&[("id", "42")]));
        assert_eq!(matched.route.layouts.len(), 2);
        assert_eq!(
            matched.route.errors.last().unwrap().path,
            source("routes/(app)/users/+error.ald").path
        );
        assert_eq!(matched.route.option_sources.len(), 5);
        assert!(matched.route.page_server.is_some());
        assert!(matched.route.endpoint.is_some());
        assert!(tree.hooks_server.is_some() && tree.hooks_client.is_some());
        assert_eq!(tree.remotes, vec![source("lib/users.remote.ald")]);
    }

    #[test]
    fn discovery_is_input_order_independent() {
        let a = source("routes/[id]/+page.ald");
        let b = source("routes/new/+page.ald");
        let root = Path::new("/app/src");
        assert_eq!(
            discover(root, &[a.clone(), b.clone()]),
            discover(root, &[b, a])
        );
    }

    #[test]
    fn static_required_optional_rest_precedence() {
        let tree = manifest(&[
            "routes/[...rest]/+page.ald",
            "routes/[[optional]]/+page.ald",
            "routes/[id]/+page.ald",
            "routes/new/+page.ald",
        ]);
        assert_eq!(tree.match_path("/new").unwrap().unwrap().route.id, "/new");
        assert_eq!(tree.match_path("/42").unwrap().unwrap().route.id, "/[id]");
        assert_eq!(
            tree.match_path("/").unwrap().unwrap().route.id,
            "/[[optional]]"
        );
        assert_eq!(
            tree.match_path("/a/b").unwrap().unwrap().params,
            params(&[("rest", "a/b")])
        );
    }

    #[test]
    fn rest_and_optional_backtrack_to_static_suffix() {
        let tree = manifest(&[
            "routes/files/[...path]/edit/+page.ald",
            "routes/[[lang]]/about/+page.ald",
        ]);
        assert_eq!(
            tree.match_path("/files/a/b/edit").unwrap().unwrap().params,
            params(&[("path", "a/b")])
        );
        assert_eq!(
            tree.match_path("/about").unwrap().unwrap().params,
            Params::new()
        );
        assert_eq!(
            tree.match_path("/en/about").unwrap().unwrap().params,
            params(&[("lang", "en")])
        );
    }

    #[test]
    fn duplicate_patterns_include_both_source_uris() {
        for paths in [
            ["routes/[id]/+page.ald", "routes/[name]/+page.ald"],
            ["routes/(a)/+page.ald", "routes/(b)/+page.ald"],
        ] {
            let errors = discover(Path::new("/app/src"), &paths.map(source)).unwrap_err();
            assert_eq!(errors.len(), 1);
            assert!(errors[0].related_uri.is_some());
            assert!(errors[0].source_uri.starts_with("file:///"));
        }
    }

    #[test]
    fn malformed_names_and_unknown_conventions_are_diagnostics() {
        for path in [
            "routes/[id/+page.ald",
            "routes/[id]/[id]/+page.ald",
            "routes/[]/+page.ald",
            "routes/[[...rest]]/+page.ald",
            "routes/(group/+page.ald",
            "routes/+unknown.ald",
            "routes/[...a]/[...b]/+page.ald",
            "routes/[...rest]/[[optional]]/+page.ald",
        ] {
            assert!(
                discover(Path::new("/app/src"), &[source(path)]).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn duplicate_hooks_rejected() {
        assert!(
            discover(
                Path::new("/app/src"),
                &[
                    source("hooks.server.ald"),
                    source("routes/hooks.server.ald")
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn href_round_trips_unicode_and_reserved_characters() {
        let tree = manifest(&["routes/users/[id]/+page.ald"]);
        let route = &tree.routes[0];
        let values = params(&[("id", "café ?#%")]);
        let href = route.href(&values, TrailingSlash::Always).unwrap();
        assert_eq!(href, "/users/caf%C3%A9%20%3F%23%25/");
        assert_eq!(route.match_path(&href).unwrap(), Some(values));
        assert_eq!(
            route
                .canonical_path("/users/%61/", TrailingSlash::Never)
                .unwrap()
                .unwrap(),
            "/users/a"
        );
        assert_eq!(
            route
                .canonical_path("/users/%61/", TrailingSlash::Ignore)
                .unwrap()
                .unwrap(),
            "/users/a/"
        );
    }

    #[test]
    fn href_validates_required_unknown_and_unsafe_params() {
        let tree = manifest(&["routes/[id]/+page.ald"]);
        let route = &tree.routes[0];
        for values in [
            Params::new(),
            params(&[("id", "a"), ("other", "b")]),
            params(&[("id", "..")]),
            params(&[("id", "a/b")]),
        ] {
            assert!(route.href(&values, TrailingSlash::Never).is_err());
        }
    }

    #[test]
    fn invalid_percent_utf8_and_path_separators_are_rejected() {
        let tree = manifest(&["routes/[...path]/+page.ald"]);
        for path in [
            "/%", "/%GG", "/%FF", "/%2F", "/%2E%2E", "/%5c", "/a//b", "//", "/a//", "/a?b",
        ] {
            assert!(tree.match_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn options_inherit_and_child_exports_override() {
        let tree = manifest(&["routes/+layout.ald", "routes/child/+page.ald"]);
        let exports = BTreeMap::from([
            (
                source("routes/+layout.ald").uri,
                PageOptionExports {
                    ssr: Some(false),
                    trailing_slash: Some(TrailingSlash::Always),
                    ..Default::default()
                },
            ),
            (
                source("routes/child/+page.ald").uri,
                PageOptionExports {
                    ssr: Some(true),
                    csr: Some(false),
                    ..Default::default()
                },
            ),
        ]);
        let options = tree.routes[0].resolve_options(&exports).unwrap();
        assert!(options.ssr && !options.csr);
        assert_eq!(options.trailing_slash, TrailingSlash::Always);
    }

    #[test]
    fn conflicting_render_options_and_missing_dynamic_entries_fail() {
        let tree = manifest(&["routes/[id]/+page.ald"]);
        let uri = tree.routes[0].source().uri.clone();
        for options in [
            PageOptionExports {
                ssr: Some(false),
                csr: Some(false),
                ..Default::default()
            },
            PageOptionExports {
                prerender: Some(true),
                ..Default::default()
            },
        ] {
            assert!(
                tree.routes[0]
                    .resolve_options(&BTreeMap::from([(uri.clone(), options)]))
                    .is_err()
            );
        }
    }

    #[test]
    fn prerender_entries_are_validated_and_duplicates_fail() {
        let tree = manifest(&["routes/[id]/+page.ald"]);
        let route = &tree.routes[0];
        for entries in [
            vec![Params::new()],
            vec![params(&[("id", "a")]), params(&[("id", "a")])],
        ] {
            let exports = BTreeMap::from([(
                route.source().uri.clone(),
                PageOptionExports {
                    prerender: Some(true),
                    entries: Some(entries),
                    ..Default::default()
                },
            )]);
            assert!(route.resolve_options(&exports).is_err());
        }
        let exports = BTreeMap::from([(
            route.source().uri.clone(),
            PageOptionExports {
                prerender: Some(true),
                entries: Some(vec![params(&[("id", "a")])]),
                ..Default::default()
            },
        )]);
        assert!(route.resolve_options(&exports).unwrap().prerender);
    }

    fn solved(source: &str) -> InterfaceFile {
        let bump = bumpalo::Bump::new();
        let source = bump.alloc_str(source);
        let parsed = alder_parse::parse_module(&bump, source).unwrap();
        let canonical = alder_can::canonicalize(
            &bump,
            alder_can::Context {
                home: alder_ast::ModuleId {
                    package: alder_ast::PackageId::Application,
                    path: &["test"],
                },
                imports: &[],
                interfaces: &[],
            },
            &parsed,
        )
        .unwrap();
        let constraints = alder_constrain::constrain(&bump, canonical.module);
        let database = alder_solve::TraitDatabase::build(&bump, canonical.module, &[]);
        let solved = alder_solve::solve(&bump, &constraints, &database).unwrap();
        InterfaceFile::dehydrate(&alder_can::from_module(
            &bump,
            canonical.module,
            &solved.annotations,
            &[],
        ))
        .unwrap()
    }

    #[test]
    fn generated_params_use_canonical_builtin_identities() {
        let tree = manifest(&["routes/[id]/[[lang]]/[...tail]/+page.ald"]);
        let OwnedType::Record { fields, ext: None } = tree.routes[0].params_type() else {
            panic!("record");
        };
        assert_eq!(
            fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["id", "lang", "tail"]
        );
        assert_eq!(fields[0].typ.typ, builtin("String"));
        let OwnedType::Named { reference, args } = &fields[1].typ.typ else {
            panic!("Option");
        };
        assert_eq!(reference.name, "Option");
        assert_eq!(reference.module.package, OwnedPackageId::Builtin);
        assert_eq!(args[0].typ, builtin("String"));
    }

    #[test]
    fn page_data_merges_two_layouts_and_server_and_universal_loads() {
        let tree = manifest(&[
            "routes/+layout.server.ald",
            "routes/a/+layout.ald",
            "routes/a/[id]/+page.server.ald",
            "routes/a/[id]/+page.ald",
        ]);
        let interfaces = BTreeMap::from([
            (
                source("routes/+layout.server.ald").uri,
                solved("pub fn load(event) { { root: 1, overwritten: 1 } }"),
            ),
            (
                source("routes/a/+layout.ald").uri,
                solved("pub fn load(event) { { nested: true, overwritten: true } }"),
            ),
            (
                source("routes/a/[id]/+page.server.ald").uri,
                solved("pub fn load(event) Result[{ user: String }] { Ok({ user: \"Ada\" }) }"),
            ),
            (
                source("routes/a/[id]/+page.ald").uri,
                solved("pub fn load(event) { { overwritten: \"final\" } }"),
            ),
        ]);
        let types = tree.routes[0].generate_types(&interfaces).unwrap();
        let OwnedType::Record { fields, .. } = types.page_data else {
            panic!("record");
        };
        assert_eq!(
            fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["nested", "overwritten", "root", "user"]
        );
        assert_eq!(fields[1].typ.typ, builtin("String"));
    }

    #[test]
    fn generated_data_can_stop_before_pending_page_inference() {
        let tree = manifest(&["routes/+layout.ald", "routes/+page.ald"]);
        let interfaces = BTreeMap::from([(
            source("routes/+layout.ald").uri,
            solved("pub fn load(event) { { value: 42 } }"),
        )]);
        let route = &tree.routes[0];
        assert!(route.generate_types(&interfaces).is_err());
        let types = route
            .generate_types_before(&interfaces, Some(&source("routes/+page.ald").uri))
            .unwrap();
        let OwnedType::Record { fields, .. } = types.page_data else {
            panic!("record");
        };
        assert_eq!(fields.len(), 1);
    }

    #[test]
    fn non_record_load_returns_report_the_source() {
        let tree = manifest(&["routes/+page.server.ald"]);
        let uri = source("routes/+page.server.ald").uri;
        let interfaces = BTreeMap::from([(uri.clone(), solved("pub fn load(event) { 42 }"))]);
        assert_eq!(
            tree.routes[0]
                .generate_types(&interfaces)
                .unwrap_err()
                .source_uri,
            uri
        );
    }

    #[test]
    fn generated_route_links_preserve_identity_and_parameter_contracts() {
        let tree = manifest(&["routes/+page.ald", "routes/users/[id]/+page.ald"]);
        let links = tree.route_links();
        assert_eq!(links[0].export_name, "route_2f");
        assert_eq!(links[1].route_id, "/users/[id]");
        let OwnedType::Record { fields, .. } = tree.routes_type() else {
            panic!("record");
        };
        assert_eq!(fields.len(), 2);
        let OwnedType::Fn { params, ret } = &fields[1].typ.typ else {
            panic!("function");
        };
        assert_eq!(params[0].typ, links[1].params);
        assert_eq!(ret.typ, builtin("String"));
    }

    #[test]
    fn optional_href_cannot_silently_change_parameter_names() {
        let tree = manifest(&["routes/[[a]]/[[b]]/+page.ald"]);
        assert!(
            tree.routes[0]
                .href(&params(&[("b", "value")]), TrailingSlash::Never)
                .is_err()
        );
        assert_eq!(
            tree.routes[0]
                .href(&params(&[("a", "value")]), TrailingSlash::Never)
                .unwrap(),
            "/value"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_matching_and_href_agree_with_compiler_route_algorithms() {
        use serde_json::{Value, json};
        let tree = manifest(&[
            "routes/new/+page.ald",
            "routes/[id]/+page.ald",
            "routes/[[optional]]/+page.ald",
            "routes/[...rest]/+page.ald",
            "routes/files/[...path]/edit/+page.ald",
            "routes/[[lang]]/about/+page.ald",
            "routes/choice/[[a]]/[[b]]/+page.ald",
            "routes/users/[id]/+page.ald",
            "routes/café !'/+page.ald",
            "routes/(group)/docs/[id]/+page.ald",
        ]);
        let descriptors = tree
            .routes
            .iter()
            .map(|route| {
                let segments = route
                    .segments
                    .iter()
                    .map(|segment| match segment {
                        Segment::Static(name) => json!(["static", name]),
                        Segment::Param(name) => json!(["param", name]),
                        Segment::Optional(name) => json!(["optional", name]),
                        Segment::Rest(name) => json!(["rest", name]),
                    })
                    .collect::<Vec<_>>();
                json!({"id":route.id,"segments":segments})
            })
            .collect::<Vec<_>>();
        let expected_params = |route: &Route, values: Params| {
            let mut value = serde_json::to_value(values).unwrap();
            for segment in &route.segments {
                if let Segment::Optional(name) = segment {
                    value
                        .as_object_mut()
                        .unwrap()
                        .entry(name.clone())
                        .or_insert(Value::Null);
                }
            }
            json!({"route":route.id,"params":value})
        };
        let paths = [
            "/",
            "/new",
            "/new/",
            "/42",
            "/a/b",
            "/files/edit",
            "/files/a/b/edit",
            "/files/a/edit/",
            "/files/a/b/no",
            "/about",
            "/en/about",
            "/choice/value",
            "/choice/one/two",
            "/docs/alder",
            "/(group)/docs/alder",
            "/users/caf%C3%A9%20%3F%23%25%21%27%28%29%2A",
            "/caf%C3%A9%20%21%27%28%29%2A",
            "/caf%C3%A9%20%21%27",
            "/%252F",
            "/%61/",
            "/raw café",
            "/%",
            "/%GG",
            "/%FF",
            "/%C0%AF",
            "/%ED%A0%80",
            "/%2F",
            "/%5c",
            "/%00",
            "/%2E",
            "/%2e%2e",
            "/.",
            "/..",
            "/a//b",
            "//",
            "/a//",
            "/a?b",
            "/a#b",
            "/a\\b",
            "relative",
            "",
        ];
        let mut matches = Vec::new();
        for path in paths {
            let expected = match tree.match_path(path) {
                Ok(Some(found)) => expected_params(found.route, found.params),
                Ok(None) => Value::Null,
                Err(_) => json!({"error":true}),
            };
            matches.push(json!({"index":Value::Null,"path":path,"expected":expected}));
            for (index, route) in tree.routes.iter().enumerate() {
                let expected = match route.match_path(path) {
                    Ok(Some(values)) => expected_params(route, values),
                    Ok(None) => Value::Null,
                    Err(_) => json!({"error":true}),
                };
                matches.push(json!({"index":index,"path":path,"expected":expected}));
            }
        }
        let mut hrefs = Vec::new();
        for (index, route) in tree.routes.iter().enumerate() {
            let names = route
                .segments
                .iter()
                .filter_map(|segment| match segment {
                    Segment::Static(_) => None,
                    Segment::Param(name) | Segment::Optional(name) | Segment::Rest(name) => {
                        Some(name)
                    }
                })
                .collect::<Vec<_>>();
            let base = names
                .iter()
                .map(|name| ((*name).clone(), "value".to_owned()))
                .collect::<Params>();
            let mut candidates = vec![base.clone(), Params::new()];
            let mut unknown = base.clone();
            unknown.insert("unexpected".into(), "extra".into());
            candidates.push(unknown);
            for name in names {
                let mut missing = base.clone();
                missing.remove(name);
                candidates.push(missing);
                for value in [
                    "",
                    "café ?#%!'()*",
                    "a/b",
                    "a//b",
                    "/a",
                    "a/",
                    ".",
                    "..",
                    "a\\b",
                    "a\0b",
                ] {
                    let mut candidate = base.clone();
                    candidate.insert(name.clone(), value.to_owned());
                    candidates.push(candidate);
                }
            }
            for candidate in candidates {
                let (expected, matched) = match route.href(&candidate, TrailingSlash::Never) {
                    Ok(path) => {
                        let matched =
                            expected_params(route, route.match_path(&path).unwrap().unwrap());
                        (json!(path), matched)
                    }
                    Err(_) => (json!({"error":true}), Value::Null),
                };
                hrefs.push(
                    json!({"index":index,"params":candidate,"expected":expected,"matched":matched}),
                );
            }
        }
        let fixtures = json!({"routes":descriptors,"matches":matches,"hrefs":hrefs});
        let harness = r#"
          const canonical=value=>JSON.stringify(value,(_key,item)=>item && typeof item==="object" && !Array.isArray(item) ? Object.fromEntries(Object.entries(item).sort(([left],[right])=>left.localeCompare(right))) : item);
          for(const test of fixtures.matches){
            let actual;
            try {const found=$webMatch(test.index===null ? fixtures.routes : [fixtures.routes[test.index]],test.path);actual=found ? {route:found.route.id,params:found.params} : null;}
            catch {actual={error:true};}
            if(canonical(actual)!==canonical(test.expected))throw new Error("match parity: "+JSON.stringify({test,actual}));
          }
          for(const test of fixtures.hrefs){
            let actual;
            try {actual=$webHref(fixtures.routes[test.index].segments,test.params);}catch {actual={error:true};}
            if(canonical(actual)!==canonical(test.expected))throw new Error("href parity: "+JSON.stringify({test,actual}));
            if(typeof actual==="string"){
              const found=$webMatch([fixtures.routes[test.index]],actual);
              if(canonical(found && {route:found.route.id,params:found.params})!==canonical(test.matched))throw new Error("href round-trip parity: "+JSON.stringify({test,found}));
            }
          }
          const optional=[["optional","a"],["optional","b"]];
          let rejected=false;try{$webHref(optional,{a:null,b:"value"});}catch{rejected=true;}
          $assert(rejected && $webHref(optional,{a:"value",b:null})==="/value");
          let reads=0;rejected=false;
          try{$webHref([["param","id"]],{get id(){reads++;return "unsafe";}});}catch{rejected=true;}
          $assert(rejected && reads===0);
        "#;
        alder_runtime::execute(
            format!(
                "{}\nconst fixtures={fixtures};\n{harness}",
                alder_kernel::KERNEL_JS
            ),
            vec![],
        )
        .await
        .unwrap();
    }
}
