//! Rust bindings for `packages/http` (G9 API2–API5).
//!
//! - `identitas_novum` — frame substrate request ids (API2)
//! - route table / match / extract — router engine (API3)

#![allow(unused_imports)] // staged API4/API5 exports are intentionally present before all callers land.

use faber::frame;
use faber::Valor;

#[path = "concurrency.rs"]
mod concurrency;
#[path = "router.rs"]
mod router;
#[path = "service.rs"]
mod service;

pub use concurrency::{
    ApplicationState, HandlerWorkers, ResponseCompletion, ResponseTicket, StateError,
};
pub use router::{
    add_get, add_group, add_middleware, add_post, error_response, header_value, json_body,
    match_route, path_param, query_param, route_table, to_response,
};
pub use service::ServiceFixture;

pub fn identitas_novum() -> String {
    frame::next_frame_id()
}

// ---- Valor binding wrappers (signatures match Faber bodyless decls) ----

pub fn route_table_empty() -> Valor {
    route_table()
}

pub fn route_add_get(table: Valor, path: String, handler: String) -> Result<Valor, String> {
    add_get(table, path, handler)
}

pub fn route_add_post(table: Valor, path: String, handler: String) -> Result<Valor, String> {
    add_post(table, path, handler)
}

pub fn route_add_group(table: Valor, prefix: String, steps: Vec<Valor>) -> Result<Valor, String> {
    add_group(table, prefix, steps)
}

pub fn route_add_middleware(table: Valor, name: String) -> Result<Valor, String> {
    add_middleware(table, name)
}

pub fn route_match(table: Valor, method: String, path: String) -> Result<Option<Valor>, String> {
    match_route(table, method, path)
}

pub fn extract_path_param(matched: Valor, name: String) -> Option<String> {
    path_param(matched, name)
}

pub fn extract_query_param(query: String, name: String) -> Option<String> {
    query_param(query, name)
}

pub fn extract_header(headers: Valor, name: String) -> Option<String> {
    header_value(headers, name)
}

pub fn extract_json(corpus: String) -> Result<Valor, String> {
    json_body(corpus)
}

pub fn map_error(status: i64, nuntius: String) -> Valor {
    error_response(status, nuntius)
}

pub fn map_response(status: i64, corpus: String) -> Valor {
    to_response(status, corpus)
}
