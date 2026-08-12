/// Value helpers for the `FaberRuntimeError` surface.
///
/// Faber-generated `throw` sites convert values that do not conform to
/// `Error` into `FaberRuntimeError` (the emitter spells the wrap as
/// `FaberRuntimeError.error(...)`); `catch` sites convert back through
/// `FaberRuntimeError.message(from:)`. These helpers name both directions of
/// that value round-trip.
extension FaberRuntimeError {
    /// Wrap a payload value into the `FaberRuntimeError` surface so it can be
    /// thrown from generated code.
    ///
    /// Equivalent to the `error` case initializer; provided as the named
    /// value-helper entry point for the wrap direction.
    public static func wrap(_ payload: String) -> FaberRuntimeError {
        .error(payload)
    }

    /// Extract the wrapped payload from a thrown error.
    ///
    /// `nil` when the error is not a `FaberRuntimeError`. For the formatted
    /// message (with `String(describing:)` fallback) use
    /// `FaberRuntimeError.message(from:)`.
    public static func payload(of error: Error) -> String? {
        if case .error(let msg) = (error as? FaberRuntimeError) {
            return msg
        }
        return nil
    }
}
