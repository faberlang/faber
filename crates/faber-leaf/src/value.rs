//! Shared opener and reply carriers for every native leaf.

/// A positional opener value after the host flattened a bare valor or a list.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Nil,
    Str(String),
    Int(i64),
    Bytes(Vec<u8>),
    Strs(Vec<String>),
    Map(Vec<(String, String)>),
}

/// Successful leaf result.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Strs(Vec<String>),
    Map(Vec<(String, Reply)>),
}
