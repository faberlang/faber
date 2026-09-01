//! Faber language runtime types for generated Rust code.
//!
//! Standalone public package (faber/runtime/rust) — no private Radix/Hosts
//! dependency. Contract material Radix owns is committed in
//! [`contract`]; concrete built-in effects split to Hosts in S1-U3.

pub mod arena;
pub mod ascii;
pub mod ascii_bounded;
pub mod contract;
pub mod cursor_stream;
pub mod display;
pub mod failable;
pub mod frame;
pub mod instans;
pub mod intervallum;
pub mod json;
pub mod lista_bounded;
pub mod octeti_bounded;
pub mod or_recovery;
pub mod regex;
pub mod sparsa;
pub mod tensor;
pub mod textus;
pub mod textus_bounded;
pub mod valor;

pub use arena::{Arena, ArenaHandle};
pub use ascii::Ascii;
pub use ascii_bounded::{AsciiN, AsciiNOverflow};
pub use cursor_stream::{CursorStreamSink, materialize_cursor_stream};
pub use display::{
    FractusDisplay, display_bivalens, display_fractus, display_option, display_option_bivalens,
    display_option_fractus, display_option_vacuum, display_text_payload, display_valor,
};
pub use frame::{
    Cancellation, DispatchError, FrameStatus, HostDispatch, IntoFrameStatus, IntoScrinium, Meus,
    ResponseSender, Scrinium, Sermo, SermoRequest, Tuus, install_host_dispatch,
    sermo_open_with_dispatch,
};
pub use instans::{Instans, InstansPraecisio};
pub use intervallum::{Intervallum, IntervallumKind, IntervallumNumeric, IntervallumWalk};
pub use json::{Json, JsonError, JsonErrorKind};
pub use lista_bounded::{ListaN, ListaNOverflow};
pub use octeti_bounded::{OctetiN, OctetiNOverflow};
pub use or_recovery::{
    instans_from_text_or, instans_from_valor_or, octeti_get_ascii_or, octeti_get_text_or,
    valor_get_array_or, valor_get_ascii_or, valor_get_f64_or, valor_get_genus_or, valor_get_i1_or,
    valor_get_i64_or, valor_get_map_or, valor_get_octeti_or, valor_get_text_or,
};
pub use regex::Regex;
pub use sparsa::Sparsa;
pub use tensor::Tensor;
pub use textus::unicode_scalar_value;
pub use textus_bounded::{TextusN, TextusNOverflow};
pub use valor::{FromValor, Valor};

#[cfg(test)]
#[path = "display_test.rs"]
mod display_test;

#[cfg(test)]
#[path = "textus_test.rs"]
mod textus_test;

#[cfg(test)]
#[path = "instans_test.rs"]
mod instans_test;

#[cfg(test)]
#[path = "regex_test.rs"]
mod regex_test;

#[cfg(test)]
#[path = "intervallum_test.rs"]
mod intervallum_test;

#[cfg(test)]
#[path = "valor_from_valor_test.rs"]
mod valor_from_valor_test;

#[cfg(test)]
#[path = "valor_aggregate_test.rs"]
mod valor_aggregate_test;

#[cfg(test)]
#[path = "json_test.rs"]
mod json_test;

#[cfg(test)]
#[path = "frame_test.rs"]
mod frame_test;

#[cfg(test)]
#[path = "frame_live_test.rs"]
mod frame_live_test;
