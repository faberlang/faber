//! Shared `@`-parameter hint vocabulary used by codegen and the MIR runner.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EndianHint {
    Be,
    Le,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HintKind {
    Be,
    Le,
    Bits,
    Other,
}

#[must_use]
pub fn classify(name: &str) -> HintKind {
    match name {
        "Be" => HintKind::Be,
        "Le" => HintKind::Le,
        "Bits" => HintKind::Bits,
        _ => HintKind::Other,
    }
}

#[must_use]
pub fn endian_hint<S, F>(params: &[S], mut classify_param: F) -> Option<EndianHint>
where
    F: FnMut(&S) -> HintKind,
{
    params.iter().find_map(|param| match classify_param(param) {
        HintKind::Be => Some(EndianHint::Be),
        HintKind::Le => Some(EndianHint::Le),
        HintKind::Bits | HintKind::Other => None,
    })
}

#[must_use]
pub fn has_bits_hint<S, F>(params: &[S], mut classify_param: F) -> bool
where
    F: FnMut(&S) -> HintKind,
{
    params
        .iter()
        .any(|param| matches!(classify_param(param), HintKind::Bits))
}

#[cfg(test)]
mod tests {
    use super::{EndianHint, HintKind, classify, endian_hint, has_bits_hint};

    fn classify_names(names: &[&str]) -> (Option<EndianHint>, bool) {
        (
            endian_hint(names, |name| classify(name)),
            has_bits_hint(names, |name| classify(name)),
        )
    }

    #[test]
    fn recognizes_endian_and_bits_hints() {
        assert_eq!(
            classify_names(&["Unused", "Le", "Bits"]),
            (Some(EndianHint::Le), true)
        );
    }
}
