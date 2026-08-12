import XCTest
import FaberRuntime

/// Proves the extracted error/format/value surface preserves the behavior of
/// the compiler's inline `FaberRuntimeError` (faber-target-runtime S6-U1):
/// payload round-trip through wrap → throw → catch, verbatim payload
/// formatting, and `String(describing:)` fallback for non-FaberRuntimeError
/// thrown values.
final class FaberRuntimeErrorTests: XCTestCase {
    /// A thrown error that is not a `FaberRuntimeError`.
    private struct PlainError: Error, CustomStringConvertible {
        let detail: String

        var description: String { detail }
    }

    func testWrapThrowsAndCatchExtractsPayload() {
        let thrown: Error = FaberRuntimeError.wrap("oops")

        do {
            throw thrown
        } catch {
            // Same unwrap the generated catch binding performs.
            let message = FaberRuntimeError.message(from: error)
            XCTAssertEqual(message, "oops")
        }
    }

    func testErrorCaseConstructsDirectly() {
        let err = FaberRuntimeError.error("direct")
        XCTAssertEqual(err.message, "direct")
    }

    func testMessageFromFaberRuntimeErrorReturnsPayloadVerbatim() {
        let err: Error = FaberRuntimeError.error("payload")
        XCTAssertEqual(FaberRuntimeError.message(from: err), "payload")
    }

    func testMessageFromOtherErrorFallsBackToDescribing() {
        let err: Error = PlainError(detail: "plain")
        XCTAssertEqual(FaberRuntimeError.message(from: err), "plain")
    }

    func testPayloadExtraction() {
        XCTAssertEqual(FaberRuntimeError.payload(of: FaberRuntimeError.error("x")), "x")
        XCTAssertNil(FaberRuntimeError.payload(of: PlainError(detail: "plain")))
    }
}
