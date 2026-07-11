//! H3 HTTP service fixture composed from the G9 transport and framework seams.
//!
//! TARGET: API5 product proof for grouped routes, middleware, extraction,
//! structured errors, shared state, and overlapping requests on real loopback.

#![allow(dead_code)] // API5 fixture is a product proof surface used by targeted tests and later callers.

use super::{
    add_group, add_middleware, header_value, json_body, match_route, path_param, query_param,
    ApplicationState,
};
use bytes::Bytes;
use faber::Valor;
use faber_http_transport::{HttpRequest, HttpResponse, HttpTransport, TransportConfig};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

const JSON_CONTENT_TYPE: (&str, &str) = ("content-type", "application/json");

/// Running API5 proof service. The transport owns accept/correlation while the
/// fixture owns route dispatch and explicitly shared application state.
pub struct ServiceFixture {
    transport: HttpTransport,
    state: ApplicationState,
}

impl ServiceFixture {
    pub async fn serve() -> Result<Self, String> {
        let routes = fixture_routes()?;
        let state = ApplicationState::new();
        let handler_state = state.clone();
        let transport = HttpTransport::serve(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            TransportConfig {
                request_timeout: Duration::from_secs(2),
                ..TransportConfig::default()
            },
            move |request| dispatch(routes.clone(), handler_state.clone(), request),
        )
        .await
        .map_err(|error| error.to_string())?;
        Ok(Self { transport, state })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.transport.local_addr()
    }

    pub fn counter(&self) -> Result<i64, String> {
        match self.state.get("requests").map_err(state_error)? {
            Some(Valor::Numerus(value)) => Ok(value),
            Some(_) => Err("shared counter is not numeric".to_owned()),
            None => Ok(0),
        }
    }

    pub async fn shutdown(self) {
        self.transport.shutdown_and_join().await;
    }
}

async fn dispatch(routes: Valor, state: ApplicationState, request: HttpRequest) -> HttpResponse {
    let matched = match match_route(routes, request.method.clone(), request.path.clone()) {
        Ok(Some(matched)) => matched,
        Ok(None) => return json_error(404, "route_not_found"),
        Err(_) => return json_error(500, "ambiguous_route"),
    };
    let middleware = valor_text_list(&matched, "middleware");
    let handler = valor_text(&matched, "handler").unwrap_or_default();
    let mut response = match handler.as_str() {
        "show_item" => show_item(matched, request),
        "create_item" => create_item(request),
        "slow" => slow(state).await,
        "fail" => json_error(500, "fixture_failure"),
        _ => json_error(500, "unknown_handler"),
    };
    response
        .headers
        .push(("x-faber-middleware".to_owned(), middleware.join(",")));
    response
}

fn show_item(matched: Valor, request: HttpRequest) -> HttpResponse {
    let Some(id) = path_param(matched, "id".to_owned()) else {
        return json_error(400, "missing_path_id");
    };
    let verbose = request
        .query
        .and_then(|query| query_param(query, "verbose".to_owned()))
        .unwrap_or_else(|| "false".to_owned());
    let client = header_value(headers_to_valor(request.headers), "x-client".to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    json_response(
        200,
        format!(
            r#"{{"id":{},"verbose":{},"client":{}}}"#,
            json_string(&id),
            json_string(&verbose),
            json_string(&client)
        ),
    )
}

fn create_item(request: HttpRequest) -> HttpResponse {
    let body = String::from_utf8_lossy(&request.body).into_owned();
    let parsed = match json_body(body) {
        Ok(value) => value,
        Err(_) => return json_error(400, "invalid_json_object"),
    };
    let Some(name) = valor_text(&parsed, "name") else {
        return json_error(400, "missing_name");
    };
    json_response(201, format!(r#"{{"name":{}}}"#, json_string(&name)))
}

async fn slow(state: ApplicationState) -> HttpResponse {
    tokio::time::sleep(Duration::from_millis(120)).await;
    match state.increment("requests") {
        Ok(count) => json_response(200, format!(r#"{{"count":{count}}}"#)),
        Err(_) => json_error(500, "state_failure"),
    }
}

fn fixture_routes() -> Result<Valor, String> {
    let routes = add_middleware(super::route_table(), "request-id".to_owned())?;
    add_group(
        routes,
        "/api".to_owned(),
        vec![
            route("GET", "/items/{id}", "show_item"),
            route("POST", "/items", "create_item"),
            route("GET", "/slow", "slow"),
            route("GET", "/fail", "fail"),
        ],
    )
}

fn route(method: &str, path: &str, handler: &str) -> Valor {
    Valor::Tabula(BTreeMap::from([
        ("method".to_owned(), Valor::Textus(method.to_owned())),
        ("path".to_owned(), Valor::Textus(path.to_owned())),
        ("handler".to_owned(), Valor::Textus(handler.to_owned())),
    ]))
}

fn valor_text(value: &Valor, key: &str) -> Option<String> {
    let Valor::Tabula(fields) = value else {
        return None;
    };
    match fields.get(key) {
        Some(Valor::Textus(value)) => Some(value.clone()),
        _ => None,
    }
}

fn valor_text_list(value: &Valor, key: &str) -> Vec<String> {
    let Valor::Tabula(fields) = value else {
        return Vec::new();
    };
    let Some(Valor::Lista(values)) = fields.get(key) else {
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| match value {
            Valor::Textus(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn headers_to_valor(headers: Vec<(String, String)>) -> Valor {
    Valor::Tabula(
        headers
            .into_iter()
            .map(|(name, value)| (name, Valor::Textus(value)))
            .collect(),
    )
}

fn json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character if character.is_control() => {
                encoded.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

fn json_response(status: u16, body: String) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![(
            JSON_CONTENT_TYPE.0.to_owned(),
            JSON_CONTENT_TYPE.1.to_owned(),
        )],
        body: Bytes::from(body),
    }
}

fn json_error(status: u16, issue: &str) -> HttpResponse {
    json_response(status, format!(r#"{{"error":true,"issue":"{issue}"}}"#))
}

fn state_error(error: super::StateError) -> String {
    format!("state error: {error:?}")
}

#[cfg(test)]
#[path = "service_test.rs"]
mod service_test;
