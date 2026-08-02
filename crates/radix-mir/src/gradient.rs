/// Shared gradient operation classification for backend codegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirGradientCall {
    Create,
    Accumulate,
    Read,
    Zero,
}
