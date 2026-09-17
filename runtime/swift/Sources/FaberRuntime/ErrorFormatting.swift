/// Error formatting surface for Faber-generated Swift code.
///
/// Generated `catch` bindings call `FaberRuntimeError.message(from:)`.
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
    /// falls back to `String(describing:)`.
    public static func message(from error: Error) -> String {
        (error as? FaberRuntimeError)?.message ?? String(describing: error)
    }
}
