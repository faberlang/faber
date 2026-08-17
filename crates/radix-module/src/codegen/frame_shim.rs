//! In-process frame conversation shim for expression-form `ad` codegen.
//!
//! Provides Rust emission helpers (sermo_open, sermo_set_opener, etc.) and an
//! HIR visitor that detects whether analyzed code uses expression-form `ad` and
//! therefore needs frame shim helpers. Boundary: Rust codegen backend; other
//! backends implement frame shim in their own crate.

#[cfg(feature = "hir-go")]
use super::CodeWriter;

/// Generated local holding the in-flight `ad` conversation.
pub const RUST_SERMO_VAR: &str = "sermo_frame";

/// Rust opener for expression-form `ad`.
#[must_use]
pub fn rust_sermo_open(route_lit: &str) -> String {
    format!("let mut {RUST_SERMO_VAR} = faber::frame::sermo_open({route_lit});")
}

/// Rust patch for the stream opener expression on the Request frame.
#[must_use]
pub fn rust_sermo_set_opener_prefix() -> &'static str {
    "faber::frame::sermo_set_opener(&mut sermo_frame, "
}

/// Whether analyzed `HIR` contains expression-form `ad` and needs frame shim helpers.
#[must_use]
pub fn hir_needs_frame_shim(hir: &crate::hir::HirModule) -> bool {
    use crate::hir::visit::{walk_expr, HirVisitor};
    use crate::hir::HirExpressionKind;

    struct Detector {
        found: bool,
    }

    impl HirVisitor for Detector {
        fn visit_expr(&mut self, expr: &crate::hir::HirExpression) {
            if matches!(expr.kind, HirExpressionKind::Ad { .. }) {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }
    }

    let mut detector = Detector { found: false };
    detector.visit_program(hir);
    detector.found
}

#[cfg(feature = "hir-go")]
pub(super) fn generate_go_frame_shim_helpers(writer: &mut CodeWriter) {
    writer.writeln("var radixFrameSeq uint64");
    writer.writeln("func radixNextFrameID() string {");
    writer.indented(|writer| {
        writer.writeln("return fmt.Sprintf(\"frame-%d\", atomic.AddUint64(&radixFrameSeq, 1))");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("type radixFrameStatus int");
    writer.newline();
    writer.writeln("const (");
    writer.indented(|writer| {
        writer.writeln("radixFrameRequest radixFrameStatus = iota");
        writer.writeln("radixFrameItem");
        writer.writeln("radixFrameByte");
        writer.writeln("radixFrameBulk");
        writer.writeln("radixFrameDone");
        writer.writeln("radixFrameError");
        writer.writeln("radixFrameCancel");
    });
    writer.writeln(")");
    writer.newline();
    writer.writeln("type radixScrinium struct {");
    writer.indented(|writer| {
        writer.writeln("id string");
        writer.writeln("parentID string");
        writer.writeln("call string");
        writer.writeln("status radixFrameStatus");
        writer.writeln("data interface{}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixStatusToFrame(s status) radixFrameStatus {");
    writer.indented(|writer| {
        writer.writeln("switch s.(type) {");
        writer.indented(|writer| {
            writer.writeln("case request:");
            writer.writeln("\treturn radixFrameRequest");
            writer.writeln("case item:");
            writer.writeln("\treturn radixFrameItem");
            writer.writeln("case byte:");
            writer.writeln("\treturn radixFrameByte");
            writer.writeln("case bulk:");
            writer.writeln("\treturn radixFrameBulk");
            writer.writeln("case done:");
            writer.writeln("\treturn radixFrameDone");
            writer.writeln("case error:");
            writer.writeln("\treturn radixFrameError");
            writer.writeln("case cancel:");
            writer.writeln("\treturn radixFrameCancel");
            writer.writeln("default:");
            writer.writeln("\treturn radixFrameItem");
        });
        writer.writeln("}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixScriniumFromUser[T any](frame scrinium[T]) radixScrinium {");
    writer.indented(|writer| {
        writer.writeln("id := \"\"");
        writer.writeln("if frame.Id != nil {");
        writer.indented(|writer| writer.writeln("id = *frame.Id"));
        writer.writeln("}");
        writer.writeln("parentID := \"\"");
        writer.writeln("if frame.Parent_id != nil {");
        writer.indented(|writer| {
            writer.writeln("if text, ok := (*frame.Parent_id).(string); ok {");
            writer.indented(|writer| writer.writeln("parentID = text"));
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.writeln("call := \"\"");
        writer.writeln("if frame.Call != nil {");
        writer.indented(|writer| writer.writeln("call = *frame.Call"));
        writer.writeln("}");
        writer.writeln("return radixScrinium{");
        writer.indented(|writer| {
            writer.writeln("id: id,");
            writer.writeln("parentID: parentID,");
            writer.writeln("call: call,");
            writer.writeln("status: radixStatusToFrame(frame.Status),");
            writer.writeln("data: frame.Data,");
        });
        writer.writeln("}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("type radixSermo struct {");
    writer.indented(|writer| {
        writer.writeln("conversationID string");
        writer.writeln("route string");
        writer.writeln("outgoing []radixScrinium");
        writer.writeln("incoming []radixScrinium");
        writer.writeln("incomingDrained bool");
        writer.writeln("incomingTerminal radixFrameStatus");
        writer.writeln("meusClosed bool");
        writer.writeln("detached bool");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("type radixMeus[T any] struct { sermo *radixSermo }");
    writer.writeln("type radixTuus[T any] struct { sermo *radixSermo }");
    writer.newline();
    writer.writeln("func radixSermoMeus[T any](sermo *radixSermo) radixMeus[T] {");
    writer.indented(|writer| writer.writeln("return radixMeus[T]{sermo: sermo}"));
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoTuus[T any](sermo *radixSermo) radixTuus[T] {");
    writer.indented(|writer| writer.writeln("return radixTuus[T]{sermo: sermo}"));
    writer.writeln("}");
    writer.newline();
    writer.writeln("func (view radixMeus[T]) Da(data T) {");
    writer.indented(|writer| {
        writer.writeln("if view.sermo == nil || view.sermo.meusClosed { panic(\"meus half-stream is closed\") }");
        writer.writeln("view.sermo.outgoing = append(view.sermo.outgoing, radixScrinium{id: radixNextFrameID(), parentID: view.sermo.conversationID, call: view.sermo.route, status: radixFrameItem, data: data})");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func (view radixMeus[T]) Fini() status {");
    writer.indented(|writer| {
        writer.writeln("if view.sermo == nil { panic(\"meus view has no sermo\") }");
        writer.writeln("if !view.sermo.meusClosed {");
        writer.indented(|writer| {
            writer.writeln("view.sermo.outgoing = append(view.sermo.outgoing, radixScrinium{id: radixNextFrameID(), parentID: view.sermo.conversationID, call: view.sermo.route, status: radixFrameDone})");
            writer.writeln("view.sermo.meusClosed = true");
        });
        writer.writeln("}");
        writer.writeln("return done{}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func (view radixTuus[T]) Cursor() []scrinium[T] {");
    writer.indented(|writer| writer.writeln("return radixSermoCursor[T](view.sermo)"));
    writer.writeln("}");
    writer.newline();
    writer.writeln("func (view radixTuus[T]) Accipe() *scrinium[T] {");
    writer.indented(|writer| writer.writeln("return radixSermoAccipe[T](view.sermo)"));
    writer.writeln("}");
    writer.newline();
    writer.writeln("func (view radixTuus[T]) Fini() status {");
    writer.indented(|writer| writer.writeln("return radixSermoFini(view.sermo)"));
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoTypedFrame[T any](frame radixScrinium) *scrinium[T] {");
    writer.indented(|writer| {
        writer.writeln("data, ok := frame.data.(T)");
        writer.writeln("if !ok { panic(\"tuus frame payload has the wrong type\") }");
        writer.writeln("return &scrinium[T]{Data: data}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoCursor[T any](sermo *radixSermo) []scrinium[T] {");
    writer.indented(|writer| {
        writer.writeln("if sermo == nil { panic(\"tuus view has no sermo\") }");
        writer.writeln("frames := make([]scrinium[T], 0)");
        writer.writeln("for len(sermo.incoming) > 0 {");
        writer.indented(|writer| {
            writer.writeln("frame := sermo.incoming[0]");
            writer.writeln("sermo.incoming = sermo.incoming[1:]");
            writer.writeln("switch frame.status {");
            writer.indented(|writer| {
                writer.writeln("case radixFrameItem, radixFrameByte, radixFrameBulk:");
                writer.indented(|writer| writer.writeln("frames = append(frames, *radixSermoTypedFrame[T](frame))"));
                writer.writeln("case radixFrameDone, radixFrameError, radixFrameCancel:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("return frames");
                });
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.writeln("sermo.incomingDrained = true");
        writer.writeln("return frames");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoAccipe[T any](sermo *radixSermo) *scrinium[T] {");
    writer.indented(|writer| {
        writer.writeln("if sermo == nil { panic(\"tuus view has no sermo\") }");
        writer.writeln("for len(sermo.incoming) > 0 {");
        writer.indented(|writer| {
            writer.writeln("frame := sermo.incoming[0]");
            writer.writeln("sermo.incoming = sermo.incoming[1:]");
            writer.writeln("switch frame.status {");
            writer.indented(|writer| {
                writer.writeln("case radixFrameItem, radixFrameByte, radixFrameBulk:");
                writer.indented(|writer| writer.writeln("return radixSermoTypedFrame[T](frame)"));
                writer.writeln("case radixFrameDone, radixFrameError, radixFrameCancel:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("return nil");
                });
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.writeln("sermo.incomingDrained = true");
        writer.writeln("return nil");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoFini(sermo *radixSermo) status {");
    writer.indented(|writer| {
        writer.writeln("if sermo == nil { panic(\"tuus view has no sermo\") }");
        writer.writeln("if !sermo.incomingDrained {");
        writer.indented(|writer| writer.writeln("radixSermoDrainToTerminal(sermo)"));
        writer.writeln("}");
        writer.writeln("switch sermo.incomingTerminal {");
        writer.indented(|writer| {
            writer.writeln("case radixFrameError:");
            writer.indented(|writer| writer.writeln("return error{}"));
            writer.writeln("case radixFrameCancel:");
            writer.indented(|writer| writer.writeln("return cancel{}"));
            writer.writeln("case radixFrameRequest:");
            writer.indented(|writer| writer.writeln("return error{}"));
            writer.writeln("default:");
            writer.indented(|writer| writer.writeln("return done{}"));
        });
        writer.writeln("}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoDrainToTerminal(sermo *radixSermo) {");
    writer.indented(|writer| {
        writer.writeln("for len(sermo.incoming) > 0 {");
        writer.indented(|writer| {
            writer.writeln("frame := sermo.incoming[0]");
            writer.writeln("sermo.incoming = sermo.incoming[1:]");
            writer.writeln("if frame.status == radixFrameDone || frame.status == radixFrameError || frame.status == radixFrameCancel { sermo.incomingTerminal = frame.status; break }");
        });
        writer.writeln("}");
        writer.writeln("sermo.incomingDrained = true");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoSetOpener(sermo *radixSermo, data interface{}) {");
    writer.indented(|writer| {
        writer.writeln("if len(sermo.outgoing) > 0 && sermo.outgoing[0].status == radixFrameRequest {");
        writer.indented(|writer| {
            writer.writeln("sermo.outgoing[0].data = data");
            writer.writeln("radixSermoDispatch(sermo)");
        });
        writer.writeln("}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoDispatch(sermo *radixSermo) {");
    writer.indented(|writer| {
        writer.writeln("if len(sermo.outgoing) == 0 || sermo.outgoing[0].status != radixFrameRequest { return }");
        writer.writeln("request := sermo.outgoing[0]");
        writer.writeln("sermo.outgoing = sermo.outgoing[1:]");
        writer.writeln("switch sermo.route {");
        writer.indented(|writer| {
            writer.writeln("case \"runtime:echo\":");
            writer.indented(|writer| {
                writer.writeln("sermo.incoming = append(sermo.incoming, radixScrinium{id: radixNextFrameID(), call: sermo.route, status: radixFrameItem, data: request.data})");
                writer.writeln("sermo.incoming = append(sermo.incoming, radixScrinium{id: radixNextFrameID(), call: sermo.route, status: radixFrameDone})");
            });
            writer.writeln("default:");
            writer.indented(|writer| {
                writer.writeln("sermo.incoming = append(sermo.incoming, radixScrinium{id: radixNextFrameID(), call: sermo.route, status: radixFrameError, data: fmt.Sprintf(\"unsupported ad route `%s`\", sermo.route)})");
            });
        });
        writer.writeln("}");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoReadText(sermo *radixSermo) string {");
    writer.indented(|writer| {
        writer.writeln("if sermo == nil { panic(\"sermo textus materializer has no sermo\") }");
        writer.writeln("text := \"\"");
        writer.writeln("contentCount := 0");
        writer.writeln("for len(sermo.incoming) > 0 {");
        writer.indented(|writer| {
            writer.writeln("frame := sermo.incoming[0]");
            writer.writeln("sermo.incoming = sermo.incoming[1:]");
            writer.writeln("switch frame.status {");
            writer.indented(|writer| {
                writer.writeln("case radixFrameItem:");
                writer.indented(|writer| {
                    writer.writeln("value, ok := frame.data.(string)");
                    writer.writeln("if !ok { radixSermoDrainToTerminal(sermo); panic(\"ad textus materialization requires a text frame\") }");
                    writer.writeln("text = text + value");
                    writer.writeln("contentCount++");
                });
                writer.writeln("case radixFrameDone:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln(
                        "if contentCount == 0 { panic(\"ad textus materialization received no item frame\") }",
                    );
                    writer.writeln("return text");
                });
                writer.writeln("case radixFrameError:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("panic(fmt.Sprint(frame.data))");
                });
                writer.writeln("case radixFrameCancel:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("panic(\"ad textus materialization was cancelled\")");
                });
                writer.writeln("case radixFrameByte, radixFrameBulk:");
                writer.indented(|writer| {
                    writer.writeln("radixSermoDrainToTerminal(sermo)");
                    writer.writeln("panic(\"ad textus materialization requires an item frame\")");
                });
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.writeln("sermo.incomingDrained = true");
        writer.writeln("panic(\"ad textus materialization received no terminal frame\")");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoReadNumerus(sermo *radixSermo) (int, interface{}) {");
    writer.indented(|writer| {
        writer.writeln("if sermo == nil { return 0, fmt.Errorf(\"sermo numerus materializer has no sermo\") }");
        writer.writeln("value := 0");
        writer.writeln("contentCount := 0");
        writer.writeln("var conversionError interface{}");
        writer.writeln("for len(sermo.incoming) > 0 {");
        writer.indented(|writer| {
            writer.writeln("frame := sermo.incoming[0]");
            writer.writeln("sermo.incoming = sermo.incoming[1:]");
            writer.writeln("switch frame.status {");
            writer.indented(|writer| {
                writer.writeln("case radixFrameItem:");
                writer.indented(|writer| {
                    writer.writeln("contentCount++");
                    writer.writeln("switch frameValue := frame.data.(type) {");
                    writer.indented(|writer| {
                        writer.writeln("case int:");
                        writer.indented(|writer| writer.writeln("if contentCount == 1 { value = frameValue }"));
                        writer.writeln("case string:");
                        writer.indented(|writer| {
                            writer.writeln("if contentCount == 1 { parsed, err := strconv.Atoi(frameValue); value = parsed; conversionError = err }");
                        });
                        writer.writeln("default:");
                        writer.indented(|writer| {
                            writer.writeln("if contentCount == 1 { conversionError = fmt.Errorf(\"ad numerus materialization requires a numeric frame\") }");
                        });
                    });
                    writer.writeln("}");
                });
                writer.writeln("case radixFrameByte, radixFrameBulk:");
                writer.indented(|writer| {
                    writer.writeln("contentCount++");
                    writer.writeln("if contentCount == 1 { conversionError = fmt.Errorf(\"ad numerus materialization requires an item frame\") }");
                });
                writer.writeln("case radixFrameError:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("return 0, fmt.Errorf(\"%v\", frame.data)");
                });
                writer.writeln("case radixFrameCancel:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("return 0, fmt.Errorf(\"ad numerus materialization was cancelled\")");
                });
                writer.writeln("case radixFrameDone:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("if contentCount == 0 { return 0, fmt.Errorf(\"ad numerus materialization received no item frame\") }");
                    writer.writeln("if contentCount > 1 { return 0, fmt.Errorf(\"ad numerus materialization received multiple item frames\") }");
                    writer.writeln("if conversionError != nil { return 0, conversionError }");
                    writer.writeln("return value, nil");
                });
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.writeln("sermo.incomingDrained = true");
        writer.writeln("return 0, fmt.Errorf(\"ad numerus materialization received no terminal frame\")");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoDrain(sermo *radixSermo) {");
    writer.indented(|writer| {
        writer.writeln("if sermo == nil { panic(\"sermo vacuum materializer has no sermo\") }");
        writer.writeln("for len(sermo.incoming) > 0 {");
        writer.indented(|writer| {
            writer.writeln("frame := sermo.incoming[0]");
            writer.writeln("sermo.incoming = sermo.incoming[1:]");
            writer.writeln("switch frame.status {");
            writer.indented(|writer| {
                writer.writeln("case radixFrameDone:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("return");
                });
                writer.writeln("case radixFrameError:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("panic(fmt.Sprint(frame.data))");
                });
                writer.writeln("case radixFrameCancel:");
                writer.indented(|writer| {
                    writer.writeln("sermo.incomingDrained = true");
                    writer.writeln("sermo.incomingTerminal = frame.status");
                    writer.writeln("panic(\"ad vacuum materialization was cancelled\")");
                });
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.writeln("sermo.incomingDrained = true");
        writer.writeln("panic(\"ad vacuum materialization received no terminal frame\")");
    });
    writer.writeln("}");
    writer.newline();
    writer.writeln("func radixSermoOpen(route string) radixSermo {");
    writer.indented(|writer| {
        writer.writeln("conversationID := radixNextFrameID()");
        writer.writeln("return radixSermo{");
        writer.indented(|writer| {
            writer.writeln("conversationID: conversationID,");
            writer.writeln("route: route,");
            writer.writeln("outgoing: []radixScrinium{{");
            writer.indented(|writer| {
                writer.writeln("id: conversationID,");
                writer.writeln("call: route,");
                writer.writeln("status: radixFrameRequest,");
                writer.writeln("data: nil,");
            });
            writer.writeln("}},");
            writer.writeln("incoming: nil,");
            writer.writeln("incomingTerminal: radixFrameRequest,");
        });
        writer.writeln("}");
    });
    writer.writeln("}");
}
