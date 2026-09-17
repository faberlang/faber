/// Runtime error surface for Faber-generated Swift code.
///
/// Generated code imports the `FaberRuntime` package and uses this type
/// directly.
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
