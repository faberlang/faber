//! Rust binding for `http:http.identitas_novum`.
//!
//! Uses the unified frame substrate request-id generator so framework packages
//! share identity with API1 transport (`x-faber-request-id`).

use faber::frame;

pub fn identitas_novum() -> String {
    frame::next_frame_id()
}
