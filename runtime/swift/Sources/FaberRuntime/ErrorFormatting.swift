/// Error formatting surface for Faber-generated Swift code.
///
/// Extracted from the HIR-Swift emitter's inline catch-binding closure
/// (faber-target-runtime S6-U1): generated `catch` bindings used to carry
/// `{ e in if case .error(let msg) = (e as? FaberRuntimeError) { return msg };
/// return String(describing: e) }`. The behavior is preserved verbatim here,
/// and the emitter now calls `FaberRuntimeError.message(from:)` instead.
extension FaberRuntimeError {
    /// The message a Faber `catch` binding observes for this error.
    ///
    /// The wrapped payload when this is a `FaberRuntimeError`; Swift's default
    /// `String(describing:)` rendering otherwise.
    public var message: String {
        if case .error(let msg) = self {
            return msg
        }
        return String(describing: self)
    }

    /// Format any thrown error into the message a Faber `catch` binding
    /// observes.
    ///
    /// A `FaberRuntimeError` payload is returned verbatim; any other error
    /// falls back to `String(describing:)` — matching the behavior of the
    /// inline closure the compiler previously emitted at catch sites.
    public static func message(from error: Error) -> String {
        (error as? FaberRuntimeError)?.message ?? String(describing: error)
    }
}
