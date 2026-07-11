//! HTTP route table: static/dynamic paths, groups, method dispatch, middleware order.
//!
//! TARGET: G9 API3 framework matching engine behind Faber valor bindings.
//! WHY: fail-closed duplicate/ambiguous routes; keep radix free of HTTP semantics.

use faber::Valor;
use std::collections::BTreeMap;

const KEY_ROUTES: &str = "routes";
const KEY_MIDDLEWARE: &str = "middleware";
const KEY_METHOD: &str = "method";
const KEY_PATH: &str = "path";
const KEY_HANDLER: &str = "handler";
const KEY_GROUP: &str = "group";
const KEY_PARAMS: &str = "params";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Route {
    method: String,
    path: String,
    handler: String,
    group: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RouteTable {
    routes: Vec<Route>,
    middleware: Vec<String>,
}

impl RouteTable {
    fn empty() -> Self {
        Self {
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }

    fn to_valor(&self) -> Valor {
        let routes = self
            .routes
            .iter()
            .map(|route| {
                Valor::Tabula(BTreeMap::from([
                    (KEY_METHOD.to_owned(), Valor::Textus(route.method.clone())),
                    (KEY_PATH.to_owned(), Valor::Textus(route.path.clone())),
                    (KEY_HANDLER.to_owned(), Valor::Textus(route.handler.clone())),
                    (KEY_GROUP.to_owned(), Valor::Textus(route.group.clone())),
                ]))
            })
            .collect();
        let middleware = self
            .middleware
            .iter()
            .map(|name| Valor::Textus(name.clone()))
            .collect();
        Valor::Tabula(BTreeMap::from([
            (KEY_ROUTES.to_owned(), Valor::Lista(routes)),
            (KEY_MIDDLEWARE.to_owned(), Valor::Lista(middleware)),
        ]))
    }

    fn from_valor(value: &Valor) -> Result<Self, String> {
        let Valor::Tabula(fields) = value else {
            return Err("route table must be a tabula".to_owned());
        };
        let routes = match fields.get(KEY_ROUTES) {
            Some(Valor::Lista(items)) => items
                .iter()
                .map(route_from_valor)
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err("routes must be a lista".to_owned()),
            None => Vec::new(),
        };
        let middleware = match fields.get(KEY_MIDDLEWARE) {
            Some(Valor::Lista(items)) => items
                .iter()
                .map(|item| match item {
                    Valor::Textus(name) => Ok(name.clone()),
                    _ => Err("middleware entries must be textus".to_owned()),
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err("middleware must be a lista".to_owned()),
            None => Vec::new(),
        };
        Ok(Self { routes, middleware })
    }
}

fn route_from_valor(value: &Valor) -> Result<Route, String> {
    let Valor::Tabula(fields) = value else {
        return Err("route must be a tabula".to_owned());
    };
    Ok(Route {
        method: text_field(fields, KEY_METHOD)?.to_ascii_uppercase(),
        path: normalize_path(text_field(fields, KEY_PATH)?),
        handler: text_field(fields, KEY_HANDLER)?.to_owned(),
        group: fields
            .get(KEY_GROUP)
            .and_then(|v| match v {
                Valor::Textus(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default(),
    })
}

fn text_field<'a>(fields: &'a BTreeMap<String, Valor>, key: &str) -> Result<&'a str, String> {
    match fields.get(key) {
        Some(Valor::Textus(s)) => Ok(s.as_str()),
        Some(_) => Err(format!("{key} must be textus")),
        None => Err(format!("missing route field {key}")),
    }
}

/// Normalize path: ensure leading `/`, collapse empty segments except root.
pub fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return "/".to_owned();
    }
    let with_slash = if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    };
    let parts: Vec<&str> = with_slash
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn path_segments(path: &str) -> Vec<String> {
    normalize_path(path)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn decoded_request_segments(path: &str) -> Vec<String> {
    path_segments(path)
        .into_iter()
        .map(|segment| percent_decode_bytes(&segment))
        .collect()
}

fn is_param(segment: &str) -> bool {
    segment.starts_with('{') && segment.ends_with('}') && segment.len() > 2
}

fn param_name(segment: &str) -> Option<&str> {
    if is_param(segment) {
        Some(&segment[1..segment.len() - 1])
    } else {
        None
    }
}

/// Empty route table valor.
pub fn route_table() -> Valor {
    RouteTable::empty().to_valor()
}

/// Register GET route; rejects duplicate method+path.
pub fn add_get(table: Valor, path: String, handler: String) -> Result<Valor, String> {
    add_route(table, "GET", path, handler, String::new())
}

/// Register POST route; rejects duplicate method+path.
pub fn add_post(table: Valor, path: String, handler: String) -> Result<Valor, String> {
    add_route(table, "POST", path, handler, String::new())
}

fn add_route(
    table: Valor,
    method: &str,
    path: String,
    handler: String,
    group: String,
) -> Result<Valor, String> {
    if handler.trim().is_empty() {
        return Err("handler id must not be empty".to_owned());
    }
    let mut tab = RouteTable::from_valor(&table)?;
    let route = Route {
        method: method.to_ascii_uppercase(),
        path: normalize_path(&path),
        handler,
        group,
    };
    if tab
        .routes
        .iter()
        .any(|existing| existing.method == route.method && existing.path == route.path)
    {
        return Err(format!("duplicate route {} {}", route.method, route.path));
    }
    // Template shape collision with different param names is still the same path string
    // for static templates; dynamic ambiguity is checked at match time.
    tab.routes.push(route);
    Ok(tab.to_valor())
}

/// Append relative routes under a path prefix (group).
///
/// Each step is a tabula `{ method, path, handler }`. Paths are joined with the prefix.
pub fn add_group(table: Valor, prefix: String, steps: Vec<Valor>) -> Result<Valor, String> {
    let mut tab = RouteTable::from_valor(&table)?;
    let prefix = normalize_path(&prefix);
    for (index, step) in steps.into_iter().enumerate() {
        let route = route_from_valor(&step).map_err(|err| format!("group step {index}: {err}"))?;
        let joined = join_paths(&prefix, &route.path);
        let next = Route {
            method: route.method,
            path: joined,
            handler: route.handler,
            group: if route.group.is_empty() {
                prefix.trim_start_matches('/').to_owned()
            } else {
                route.group
            },
        };
        if tab
            .routes
            .iter()
            .any(|existing| existing.method == next.method && existing.path == next.path)
        {
            return Err(format!("duplicate route {} {}", next.method, next.path));
        }
        tab.routes.push(next);
    }
    Ok(tab.to_valor())
}

fn join_paths(prefix: &str, relative: &str) -> String {
    let rel = normalize_path(relative);
    if prefix == "/" {
        return rel;
    }
    if rel == "/" {
        return prefix.to_owned();
    }
    format!("{prefix}{rel}")
}

/// Append middleware name (ordered; duplicates allowed only once — second add rejected).
pub fn add_middleware(table: Valor, name: String) -> Result<Valor, String> {
    if name.trim().is_empty() {
        return Err("middleware name must not be empty".to_owned());
    }
    let mut tab = RouteTable::from_valor(&table)?;
    if tab.middleware.iter().any(|existing| existing == &name) {
        return Err(format!("duplicate middleware `{name}`"));
    }
    tab.middleware.push(name);
    Ok(tab.to_valor())
}

/// Match method+path. Returns match tabula or nihil. Errors on ambiguous multi-match.
pub fn match_route(table: Valor, method: String, path: String) -> Result<Option<Valor>, String> {
    let tab = RouteTable::from_valor(&table)?;
    let method = method.to_ascii_uppercase();
    let path = normalize_path(&path);
    let request_segments = decoded_request_segments(&path);

    let mut matches: Vec<(Route, BTreeMap<String, String>)> = Vec::new();
    for route in &tab.routes {
        if route.method != method {
            continue;
        }
        if let Some(params) = match_path(&route.path, &request_segments) {
            matches.push((route.clone(), params));
        }
    }

    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() > 1 {
        let paths: Vec<String> = matches
            .iter()
            .map(|(route, _)| format!("{} {}", route.method, route.path))
            .collect();
        return Err(format!("ambiguous routes: {}", paths.join(", ")));
    }

    let (route, params) = matches.remove(0);
    let params_valor = Valor::Tabula(
        params
            .into_iter()
            .map(|(k, v)| (k, Valor::Textus(v)))
            .collect(),
    );
    let middleware = tab
        .middleware
        .iter()
        .map(|name| Valor::Textus(name.clone()))
        .collect();
    Ok(Some(Valor::Tabula(BTreeMap::from([
        (KEY_HANDLER.to_owned(), Valor::Textus(route.handler)),
        (KEY_METHOD.to_owned(), Valor::Textus(route.method)),
        (KEY_PATH.to_owned(), Valor::Textus(route.path)),
        (KEY_GROUP.to_owned(), Valor::Textus(route.group)),
        (KEY_PARAMS.to_owned(), params_valor),
        (KEY_MIDDLEWARE.to_owned(), Valor::Lista(middleware)),
    ]))))
}

fn match_path(template: &str, request: &[String]) -> Option<BTreeMap<String, String>> {
    let template_segments = path_segments(template);
    if template_segments.len() != request.len() {
        return None;
    }
    let mut params = BTreeMap::new();
    for (templ, req) in template_segments.iter().zip(request.iter()) {
        if let Some(name) = param_name(templ) {
            if name.is_empty() || name.contains('{') || name.contains('}') {
                return None;
            }
            params.insert(name.to_owned(), req.clone());
        } else if templ != req {
            return None;
        }
    }
    Some(params)
}

/// Extract named path param from a match valor.
pub fn path_param(match_valor: Valor, name: String) -> Option<String> {
    let Valor::Tabula(fields) = match_valor else {
        return None;
    };
    let Some(Valor::Tabula(params)) = fields.get(KEY_PARAMS) else {
        return None;
    };
    match params.get(&name) {
        Some(Valor::Textus(value)) => Some(value.clone()),
        _ => None,
    }
}

/// Extract query parameter from `a=1&b=2` wire form.
pub fn query_param(query: String, name: String) -> Option<String> {
    let query = query.strip_prefix('?').unwrap_or(query.as_str());
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        if key == name {
            return Some(percent_decode_query_component(value));
        }
    }
    None
}

/// Case-insensitive header lookup from a tabula of textus values.
pub fn header_value(headers: Valor, name: String) -> Option<String> {
    let Valor::Tabula(fields) = headers else {
        return None;
    };
    let target = name.to_ascii_lowercase();
    for (key, value) in fields {
        if key.to_ascii_lowercase() == target {
            return match value {
                Valor::Textus(text) => Some(text),
                _ => None,
            };
        }
    }
    None
}

/// Parse JSON object body into valor (object-root only).
pub fn json_body(corpus: String) -> Result<Valor, String> {
    faber::json::Json::parse(&corpus)
        .map(|doc| doc.into_valor())
        .map_err(|err| err.to_string())
}

/// Structured error → response tabula `{ status, corpus, error: true }`.
pub fn error_response(status: i64, nuntius: String) -> Valor {
    Valor::Tabula(BTreeMap::from([
        ("status".to_owned(), Valor::Numerus(status)),
        ("corpus".to_owned(), Valor::Textus(nuntius)),
        ("error".to_owned(), Valor::Bivalens(true)),
    ]))
}

/// Convert a successful match or generic handler payload into a response tabula.
pub fn to_response(status: i64, corpus: String) -> Valor {
    Valor::Tabula(BTreeMap::from([
        ("status".to_owned(), Valor::Numerus(status)),
        ("corpus".to_owned(), Valor::Textus(corpus)),
        ("error".to_owned(), Valor::Bivalens(false)),
    ]))
}

fn percent_decode_query_component(input: &str) -> String {
    percent_decode_bytes(&input.replace('+', " "))
}

fn percent_decode_bytes(input: &str) -> String {
    // Decode on bytes so multi-byte UTF-8 survives; malformed escapes stay raw.
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Some(value) = percent_decode_hex_pair(bytes[i + 1], bytes[i + 2]) {
                    out.push(value);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_owned())
}

fn percent_decode_hex_pair(hi: u8, lo: u8) -> Option<u8> {
    Some((hex_nibble(hi)? << 4) | hex_nibble(lo)?)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "router_test.rs"]
mod router_test;
