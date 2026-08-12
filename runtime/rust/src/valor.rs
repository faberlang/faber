//! Canonical dynamic value carrier for Faber `valor` and `ignotum`.

use crate::{Ascii, Instans, InstansPraecisio};
use std::collections::{BTreeMap, HashMap};

/// Canonical dynamic value for Faber `valor` / `ignotum` lowering.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Valor {
    #[default]
    Nihil,
    Bivalens(bool),
    Numerus(i64),
    Fractus(f64),
    Textus(String),
    Octeti(Vec<u8>),
    Lista(Vec<Valor>),
    Tabula(BTreeMap<String, Valor>),
    /// RFC3339 datetime wire form — not HAL `tempus`; typed extraction is `instans`.
    Instans(String),
}

impl Valor {
    #[must_use]
    pub fn is_nihil(&self) -> bool {
        matches!(self, Valor::Nihil)
    }
}

/// Runtime extraction trait for `valor ↦ T` lowering.
///
/// WHY: aggregate and genus walks need a shared per-type hook codegen can call
/// without inlining arbitrary-depth `match` trees. Scalar impls live here;
/// `valor ↦ instans` remains on `Instans::try_from_valor`.
///
/// Collection impls are **atomic**: one failed element makes the whole extraction
/// return `None`.
pub trait FromValor: Sized {
    fn from_valor(v: &Valor) -> Option<Self>;
}

impl FromValor for () {
    fn from_valor(v: &Valor) -> Option<Self> {
        match v {
            Valor::Nihil => Some(()),
            _ => None,
        }
    }
}

impl FromValor for bool {
    fn from_valor(v: &Valor) -> Option<Self> {
        match v {
            Valor::Bivalens(b) => Some(*b),
            _ => None,
        }
    }
}

impl FromValor for i64 {
    fn from_valor(v: &Valor) -> Option<Self> {
        match v {
            Valor::Numerus(n) => Some(*n),
            _ => None,
        }
    }
}

impl FromValor for u8 {
    fn from_valor(v: &Valor) -> Option<Self> {
        match v {
            Valor::Numerus(n) => u8::try_from(*n).ok(),
            _ => None,
        }
    }
}

impl FromValor for f64 {
    fn from_valor(v: &Valor) -> Option<Self> {
        match v {
            Valor::Fractus(f) => Some(*f),
            // SAFETY: intentional f64 conversion from integer; precision loss acceptable.
            #[allow(clippy::cast_precision_loss)]
            Valor::Numerus(n) => Some(*n as f64),
            _ => None,
        }
    }
}

impl FromValor for String {
    fn from_valor(v: &Valor) -> Option<Self> {
        match v {
            Valor::Textus(s) | Valor::Instans(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl FromValor for Ascii {
    fn from_valor(v: &Valor) -> Option<Self> {
        let text = String::from_valor(v)?;
        Ascii::try_from_textus(&text)
    }
}

impl FromValor for Valor {
    fn from_valor(v: &Valor) -> Option<Self> {
        Some(v.clone())
    }
}

impl FromValor for Instans {
    fn from_valor(v: &Valor) -> Option<Self> {
        Instans::try_from_valor(v, InstansPraecisio::Secunda)
    }
}

impl<T> FromValor for Vec<T>
where
    T: FromValor,
{
    fn from_valor(v: &Valor) -> Option<Self> {
        let Valor::Lista(items) = v else {
            return None;
        };
        items.iter().map(T::from_valor).collect()
    }
}

impl<V> FromValor for HashMap<String, V>
where
    V: FromValor,
{
    fn from_valor(v: &Valor) -> Option<Self> {
        let Valor::Tabula(tab) = v else {
            return None;
        };
        tab.iter()
            .map(|(key, value)| Some((key.clone(), V::from_valor(value)?)))
            .collect()
    }
}

impl From<()> for Valor {
    fn from(_: ()) -> Self {
        Valor::Nihil
    }
}

impl From<bool> for Valor {
    fn from(value: bool) -> Self {
        Valor::Bivalens(value)
    }
}

impl From<i64> for Valor {
    fn from(value: i64) -> Self {
        Valor::Numerus(value)
    }
}

impl From<f64> for Valor {
    fn from(value: f64) -> Self {
        Valor::Fractus(value)
    }
}

impl From<String> for Valor {
    fn from(value: String) -> Self {
        Valor::Textus(value)
    }
}

impl From<&str> for Valor {
    fn from(value: &str) -> Self {
        Valor::Textus(value.to_owned())
    }
}

impl From<Ascii> for Valor {
    fn from(value: Ascii) -> Self {
        Valor::Textus(value.to_textus())
    }
}

impl From<Option<Valor>> for Valor {
    fn from(value: Option<Valor>) -> Self {
        value.unwrap_or(Valor::Nihil)
    }
}

impl<T> From<Vec<T>> for Valor
where
    T: Into<Valor>,
{
    fn from(value: Vec<T>) -> Self {
        Valor::Lista(value.into_iter().map(Into::into).collect())
    }
}

impl<T> From<BTreeMap<String, T>> for Valor
where
    T: Into<Valor>,
{
    fn from(value: BTreeMap<String, T>) -> Self {
        Valor::Tabula(
            value
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        )
    }
}

impl<T> From<HashMap<String, T>> for Valor
where
    T: Into<Valor>,
{
    fn from(value: HashMap<String, T>) -> Self {
        Valor::Tabula(
            value
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        )
    }
}
