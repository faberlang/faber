package rt

import (
	"fmt"
	"runtime"
	"strconv"
)

// The sermo handle, its directional views and the conversation router for
// generated Go programs (codegen-readability T1-G9, moved verbatim from the Go
// emitter's frame shim; A4/U7, D6.9).
//
// `SermoDispatch` answers each opened conversation in a fixed order:
//
//  1. Static Faber table: the route table the generated program installs with
//     `SermoInstallRoutes`, built from the `@ ad` handlers of the emitted
//     module. The handler runs as its own goroutine (D6.13) and streams frames
//     through the conversation queue.
//  2. Builtins: `runtime:echo` echoes the opener as one item frame and a done
//     frame.
//  3. Host hook: `SermoHostDispatch`, nil unless a Go host installs one.
//  4. Fail closed: one `error` terminal carrying `unsupported ad route`.

// SermoSermo is the handle a `call` / `ad` expression returns.
type SermoSermo struct {
	conversationID string
	route          string
	conv           *SermoConversation
	caller         *frameCaller
	outgoing       []frameRecord
	meusClosed     bool
	detached       bool
}

// SermoMeus is the outbound directional view of a sermo.
type SermoMeus[T any] struct{ sermo *SermoSermo }

// SermoTuus is the inbound directional view of a sermo.
type SermoTuus[T any] struct{ sermo *SermoSermo }

func SermoAsMeus[T any](sermo *SermoSermo) SermoMeus[T] {
	return SermoMeus[T]{sermo: sermo}
}

func SermoAsTuus[T any](sermo *SermoSermo) SermoTuus[T] {
	return SermoTuus[T]{sermo: sermo}
}

func (view SermoMeus[T]) Da(data T) {
	if view.sermo == nil || view.sermo.meusClosed {
		panic("meus half-stream is closed")
	}
	view.sermo.outgoing = append(view.sermo.outgoing, frameRecord{id: frameNextID(), parentID: view.sermo.conversationID, call: view.sermo.route, code: codeItem, data: data})
}

func (view SermoMeus[T]) Fini() SermoStatus {
	if view.sermo == nil {
		panic("meus view has no sermo")
	}
	if !view.sermo.meusClosed {
		view.sermo.outgoing = append(view.sermo.outgoing, frameRecord{id: frameNextID(), parentID: view.sermo.conversationID, call: view.sermo.route, code: codeDone})
		view.sermo.meusClosed = true
		if view.sermo.conv != nil {
			convCloseOutbound(view.sermo.conv)
		}
	}
	return SermoDone{}
}

func (view SermoTuus[T]) Cursor() []SermoScrinium[T] {
	return sermoCursor[T](view.sermo)
}

func (view SermoTuus[T]) Accipe() *SermoScrinium[T] {
	return sermoAccipe[T](view.sermo)
}

func (view SermoTuus[T]) Fini() SermoStatus {
	return sermoFini(view.sermo)
}

func sermoTypedFrame[T any](frame frameRecord) *SermoScrinium[T] {
	data, ok := frame.data.(T)
	if !ok {
		panic("tuus frame payload has the wrong type")
	}
	return &SermoScrinium[T]{Data: data}
}

// sermoNext receives the next inbound frame, blocking until the answering tier
// sends one. It reports false once the terminal was read.
func sermoNext(sermo *SermoSermo) (frameRecord, bool) {
	conv := sermo.conv
	if conv == nil || conv.incomingDrained {
		return frameRecord{}, false
	}
	frame := <-conv.frames
	if frameIsTerminal(frame.code) {
		conv.incomingDrained = true
		conv.incomingTerminal = frame.code
	}
	return frame, true
}

func sermoCursor[T any](sermo *SermoSermo) []SermoScrinium[T] {
	if sermo == nil {
		panic("tuus view has no sermo")
	}
	frames := make([]SermoScrinium[T], 0)
	for {
		frame, ok := sermoNext(sermo)
		if !ok {
			return frames
		}
		switch frame.code {
		case codeItem, codeByte, codeBulk:
			frames = append(frames, *sermoTypedFrame[T](frame))
		case codeDone, codeError, codeCancel:
			return frames
		}
	}
}

func sermoAccipe[T any](sermo *SermoSermo) *SermoScrinium[T] {
	if sermo == nil {
		panic("tuus view has no sermo")
	}
	for {
		frame, ok := sermoNext(sermo)
		if !ok {
			return nil
		}
		switch frame.code {
		case codeItem, codeByte, codeBulk:
			return sermoTypedFrame[T](frame)
		case codeDone, codeError, codeCancel:
			return nil
		}
	}
}

// sermoFini closes the inbound direction. On an undrained stream it cancels the
// answering task first, then drains to the terminal.
func sermoFini(sermo *SermoSermo) SermoStatus {
	if sermo == nil {
		panic("tuus view has no sermo")
	}
	if sermo.conv == nil {
		return SermoError{}
	}
	if !sermo.conv.incomingDrained {
		convCancel(sermo.conv)
		sermoDrainToTerminal(sermo)
	}
	switch sermo.conv.incomingTerminal {
	case codeError:
		return SermoError{}
	case codeCancel:
		return SermoCancel{}
	case codeRequest:
		return SermoError{}
	default:
		return SermoDone{}
	}
}

func sermoDrainToTerminal(sermo *SermoSermo) {
	for {
		frame, ok := sermoNext(sermo)
		if !ok || frameIsTerminal(frame.code) {
			return
		}
	}
}

// SermoClose closes the conversation early (D8.3 close()). A finished handler's
// terminal is read; a live one is cancelled. It reports whether this call
// closed the handle, or the error the conversation ended with.
func SermoClose(sermo *SermoSermo) (bool, interface{}) {
	if sermo == nil || sermo.conv == nil {
		return false, nil
	}
	conv := sermo.conv
	conv.mu.Lock()
	if conv.closed {
		conv.mu.Unlock()
		return false, nil
	}
	conv.closed = true
	finished := conv.finished
	conv.mu.Unlock()
	if !conv.incomingDrained {
		if finished {
			sermoDrainToTerminal(sermo)
		} else {
			convDiscard(conv)
		}
	}
	conv.mu.Lock()
	terminal := conv.inboundTerminal
	data := conv.inboundData
	conv.mu.Unlock()
	if finished && terminal == codeError {
		return false, fmt.Sprint(data)
	}
	return true, nil
}

func SermoSetOpener(sermo *SermoSermo, data interface{}) {
	if len(sermo.outgoing) > 0 && sermo.outgoing[0].code == codeRequest {
		sermo.outgoing[0].data = data
		SermoDispatch(sermo)
	}
}

// staticRoutes is tier 1: the program's route table, installed by the
// generated program when it serves `@ ad` handlers.
var staticRoutes func(route string) func(*SermoConversation, interface{}) interface{}

// SermoInstallRoutes installs the program's static route table: the handler
// adapter for a route, or nil when the program does not serve it.
func SermoInstallRoutes(table func(route string) func(*SermoConversation, interface{}) interface{}) {
	staticRoutes = table
}

// SermoHostDispatch is tier 3: a Go host installs it to answer routes the
// program does not serve. It reports whether it took the conversation.
var SermoHostDispatch func(conv *SermoConversation, opener interface{}) bool

func SermoDispatch(sermo *SermoSermo) {
	if len(sermo.outgoing) == 0 || sermo.outgoing[0].code != codeRequest {
		return
	}
	request := sermo.outgoing[0]
	sermo.outgoing = sermo.outgoing[1:]
	conv := sermo.conv
	if staticRoutes != nil {
		if handler := staticRoutes(sermo.route); handler != nil {
			convStartTask(conv, request.data, handler)
			return
		}
	}
	switch sermo.route {
	case "runtime:echo":
		convSetTier(conv, tierBuiltin, false)
		SermoConvItem(conv, request.data)
		convTerminal(conv, codeDone, nil)
		convRelease(conv)
		return
	}
	if SermoHostDispatch != nil && SermoHostDispatch(conv, request.data) {
		convSetTier(conv, tierHost, false)
		return
	}
	convTerminal(conv, codeError, fmt.Sprintf("unsupported ad route `%s`", sermo.route))
	convRelease(conv)
}

func SermoReadText(sermo *SermoSermo) string {
	if sermo == nil {
		panic("sermo textus materializer has no sermo")
	}
	text := ""
	contentCount := 0
	for {
		frame, ok := sermoNext(sermo)
		if !ok {
			break
		}
		switch frame.code {
		case codeItem:
			value, ok := frame.data.(string)
			if !ok {
				sermoDrainToTerminal(sermo)
				panic("ad textus materialization requires a text frame")
			}
			text = text + value
			contentCount++
		case codeDone:
			if contentCount == 0 {
				panic("ad textus materialization received no item frame")
			}
			return text
		case codeError:
			panic(fmt.Sprint(frame.data))
		case codeCancel:
			panic("ad textus materialization was cancelled")
		case codeByte, codeBulk:
			sermoDrainToTerminal(sermo)
			panic("ad textus materialization requires an item frame")
		}
	}
	panic("ad textus materialization received no terminal frame")
}

func SermoReadNumerus(sermo *SermoSermo) (int, interface{}) {
	if sermo == nil {
		return 0, fmt.Errorf("sermo numerus materializer has no sermo")
	}
	value := 0
	contentCount := 0
	var conversionError interface{}
	for {
		frame, ok := sermoNext(sermo)
		if !ok {
			break
		}
		switch frame.code {
		case codeItem:
			contentCount++
			switch frameValue := frame.data.(type) {
			case int:
				if contentCount == 1 {
					value = frameValue
				}
			case string:
				if contentCount == 1 {
					parsed, err := strconv.Atoi(frameValue)
					value = parsed
					conversionError = err
				}
			default:
				if contentCount == 1 {
					conversionError = fmt.Errorf("ad numerus materialization requires a numeric frame")
				}
			}
		case codeByte, codeBulk:
			contentCount++
			if contentCount == 1 {
				conversionError = fmt.Errorf("ad numerus materialization requires an item frame")
			}
		case codeError:
			return 0, fmt.Errorf("%v", frame.data)
		case codeCancel:
			return 0, fmt.Errorf("ad numerus materialization was cancelled")
		case codeDone:
			if contentCount == 0 {
				return 0, fmt.Errorf("ad numerus materialization received no item frame")
			}
			if contentCount > 1 {
				return 0, fmt.Errorf("ad numerus materialization received multiple item frames")
			}
			if conversionError != nil {
				return 0, conversionError
			}
			return value, nil
		}
	}
	return 0, fmt.Errorf("ad numerus materialization received no terminal frame")
}

func SermoDrain(sermo *SermoSermo) {
	if sermo == nil {
		panic("sermo vacuum materializer has no sermo")
	}
	for {
		frame, ok := sermoNext(sermo)
		if !ok {
			break
		}
		switch frame.code {
		case codeDone:
			return
		case codeError:
			panic(fmt.Sprint(frame.data))
		case codeCancel:
			panic("ad vacuum materialization was cancelled")
		}
	}
	panic("ad vacuum materialization received no terminal frame")
}

func SermoOpen(route string) SermoSermo {
	conversationID := frameNextID()
	conv := newConversation(conversationID, route)
	caller := &frameCaller{conv: conv}
	runtime.SetFinalizer(caller, callerAbandoned)
	return SermoSermo{
		conversationID: conversationID,
		route:          route,
		conv:           conv,
		caller:         caller,
		outgoing: []frameRecord{{
			id:   conversationID,
			call: route,
			code: codeRequest,
			data: nil,
		}},
	}
}
