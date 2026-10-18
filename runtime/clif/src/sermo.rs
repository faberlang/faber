//! Conversations over the shared native leaves (`faber-leaf`).
//!
//! A CLIF program is single-threaded and synchronous, so a route is a value:
//! [`Conversation::open`] records the route and the opener args, and
//! `faber_leaf::serve` runs on the first materialization only. The reply, or
//! the fault, is therefore the one the leaf produced once.
//!
//! Nothing here is exported: the ABI table in `crates/radix-mir-clif` owns
//! export identity, and its `tests/abi_symbols.rs` keeps table and export in
//! agreement.
//!
//! A fault carries the two facts that cross into Faber: the leaf kind (the
//! Norma `IoFault` variant name) and the error path. The platform message
//! never crosses.

use faber_leaf::{self as leaf, Arg, LeafError, LeafErrorKind, Reply};

/// A run-time opener value.
///
/// The scalar subset a leaf slot reads. A leaf that wants a list, a map or
/// raw bytes is not yet reachable from a CLIF conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Opener {
    /// `nihil`.
    Nil,
    /// `numerus`.
    Int(i64),
    /// `bivalens`.
    Bool(bool),
    /// `textus`.
    Str(String),
}

/// The leaf argument a scalar opener crosses as.
///
/// The mirror of `runtime/rust`'s `valor_to_arg` over the scalar subset.
fn valor_to_arg(value: &Opener) -> Arg {
    match value {
        Opener::Nil => Arg::Nil,
        Opener::Int(value) => Arg::Int(*value),
        Opener::Bool(value) => Arg::Bool(*value),
        Opener::Str(value) => Arg::Str(value.clone()),
    }
}

/// One failed leaf call, reduced to the two facts that cross into Faber.
///
/// A route no leaf owns fails closed with itself as the path, and a reply of
/// the wrong shape fails closed the same way, so a fault always names the
/// route it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    kind: LeafErrorKind,
    path: String,
}

impl Fault {
    /// The kind identity.
    #[must_use]
    pub fn kind(&self) -> LeafErrorKind {
        self.kind
    }

    /// The kind as its Norma `IoFault` variant name.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        self.kind.variant_name()
    }

    /// The path the failing call was given; the route for a fault that no
    /// leaf call produced.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// A leaf error as its two crossing facts. The platform text is dropped.
    fn from_leaf(error: &LeafError) -> Self {
        Self {
            kind: error.kind(),
            path: error.path().to_owned(),
        }
    }

    /// A fail-closed fault that names `route` as its path.
    fn closed(kind: LeafErrorKind, route: &str) -> Self {
        Self {
            kind,
            path: route.to_owned(),
        }
    }
}

/// One conversation over a leaf route.
///
/// Opening is lazy: [`Conversation::open`] stores the route and its opener
/// args, and the leaf runs on the first materialization only.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    route: String,
    opener: Vec<Arg>,
    served: Option<Result<Reply, Fault>>,
}

impl Conversation {
    /// Open the conversation a leaf route names. Nothing is served yet.
    #[must_use]
    pub fn open(route: &str) -> Self {
        Self {
            route: route.to_owned(),
            opener: Vec::new(),
            served: None,
        }
    }

    /// Open with the positional opener values the leaf slot reads.
    #[must_use]
    pub fn with_opener(mut self, opener: &[Opener]) -> Self {
        self.opener = opener.iter().map(valor_to_arg).collect();
        self.served = None;
        self
    }

    /// The route this conversation was opened on.
    #[must_use]
    pub fn route(&self) -> &str {
        &self.route
    }

    /// The reply, serving the leaf on the first call.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when no
    /// leaf owns it.
    pub fn materialize(&mut self) -> Result<&Reply, Fault> {
        if self.served.is_none() {
            let opened = self.open_reply();
            self.served = Some(opened);
        }
        match self.served.as_ref() {
            Some(Ok(reply)) => Ok(reply),
            Some(Err(fault)) => Err(fault.clone()),
            None => Err(Fault::closed(LeafErrorKind::Unavailable, &self.route)),
        }
    }

    /// A unit reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `nihil`.
    pub fn unit(&mut self) -> Result<(), Fault> {
        match self.materialize()? {
            Reply::Unit => Ok(()),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `bivalens` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `bivalens`.
    pub fn bool(&mut self) -> Result<bool, Fault> {
        match self.materialize()? {
            Reply::Bool(value) => Ok(*value),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `numerus` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `numerus`.
    pub fn int(&mut self) -> Result<i64, Fault> {
        match self.materialize()? {
            Reply::Int(value) => Ok(*value),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `fractus` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `fractus`.
    pub fn float(&mut self) -> Result<f64, Fault> {
        match self.materialize()? {
            Reply::Float(value) => Ok(*value),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// A `textus` reply.
    ///
    /// # Errors
    ///
    /// Returns the leaf fault, or a fail-closed fault naming the route when
    /// the reply is not `textus`.
    pub fn text(&mut self) -> Result<String, Fault> {
        match self.materialize()? {
            Reply::Str(value) => Ok(value.clone()),
            _ => Err(Fault::closed(LeafErrorKind::InvalidArgument, &self.route)),
        }
    }

    /// Serve the route once.
    fn open_reply(&self) -> Result<Reply, Fault> {
        match leaf::serve(&self.route, &self.opener) {
            Some(Ok(reply)) => Ok(reply),
            Some(Err(error)) => Err(Fault::from_leaf(&error)),
            None => Err(Fault::closed(LeafErrorKind::Unavailable, &self.route)),
        }
    }
}

#[cfg(test)]
#[path = "sermo_test.rs"]
mod sermo_test;
