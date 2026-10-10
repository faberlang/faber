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
    Other,
}

#[must_use]
pub fn classify(name: &str) -> HintKind {
    match name {
        "Be" => HintKind::Be,
        "Le" => HintKind::Le,
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
        HintKind::Other => None,
    })
}
