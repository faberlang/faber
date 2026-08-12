/// Runtime error surface for Faber-generated Swift code.
///
/// Extracted from the HIR-Swift emitter's inline surface (faber-target-runtime
/// S6-U1): the compiler previously synthesized this enum into every generated
/// file that threw a primitive value. Generated code now imports the
/// `FaberRuntime` package (S6-U2) and uses this type directly.
///
/// Faber programs can `throw` primitive values (textus, numerus, …) that do
/// not conform to Swift's `Error` protocol. The `Throw` emit arm wraps such a
/// value in a `FaberRuntimeError` via the `error` case; `catch` bindings
/// extract the payload back through the formatting and value helpers in this
/// package.
public enum FaberRuntimeError: Error {
    /// A primitive payload wrapped at a `throw` site by generated code.
    case error(String)
}
