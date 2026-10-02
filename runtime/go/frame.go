package rt

import (
	"fmt"
	"reflect"
	"sync"
	"sync/atomic"
)

// The frame conversation runtime for generated Go programs (codegen-readability
// T1-G9, moved verbatim from the Go emitter's frame shim; A4/U7, D6.9).
//
// A conversation is the record shared by every copy of one sermo and by the
// goroutine answering it. Its frame queue is a buffered channel, so a handler
// goroutine and its consumer interleave: a cursor handler blocks when the queue
// is full and observes the caller's cancel (`tuus.fini()` on an undrained
// stream) at its next `cede`. The opener and every item cross the boundary
// through `deepCopy` (D6.14).
//
// The conversation record keeps the section 2.7 facts: id and route, answering
// tier, whether a handler task is live, inbound and outbound terminals, and a
// single release point.
//
// Lifecycle (D8.3): a conversation takes a router slot (`liveConversations`) at
// open and releases it at `convRelease`, which runs the moment it completes:
// the handler goroutine returns, fails or observes a cancel, or a builtin or
// fail-closed answer is queued. The handle lives on as a finished record whose
// queued frames still read as its result. `SermoClose` closes early: it cancels
// a live handler, discards unread frames and reports an `error` terminal. A
// `runtime.SetFinalizer` on the caller token is the last-reference net: an
// abandoned conversation is cancelled eventually, with no timing promise, and
// any error it carried is lost.

var frameSeq uint64

func frameNextID() string {
	return fmt.Sprintf("frame-%d", atomic.AddUint64(&frameSeq, 1))
}

// frameCode is the internal tag of a frame; `SermoStatus` is the user-visible
// status enum it maps to.
type frameCode int

const (
	codeRequest frameCode = iota
	codeItem
	codeByte
	codeBulk
	codeDone
	codeError
	codeCancel
)

func frameIsTerminal(code frameCode) bool {
	return code == codeDone || code == codeError || code == codeCancel
}

// SermoStatus is the `status` enum a sermo view reports; one struct per
// variant, in the shape the Go emitter gives any Faber enum.
type SermoStatus interface{ isStatus() }
type SermoRequest struct{}

func (SermoRequest) isStatus() {}

type SermoItem struct{}

func (SermoItem) isStatus() {}

type SermoByte struct{}

func (SermoByte) isStatus() {}

type SermoBulk struct{}

func (SermoBulk) isStatus() {}

type SermoDone struct{}

func (SermoDone) isStatus() {}

type SermoError struct{}

func (SermoError) isStatus() {}

type SermoCancel struct{}

func (SermoCancel) isStatus() {}

// SermoScrinium is the typed frame (`scrinium<T>`) a tuus view yields.
type SermoScrinium[T any] struct {
	Id        *string
	Parent_id *interface{}
	Call      *string
	Status    SermoStatus
	Data      T
}

type frameRecord struct {
	id       string
	parentID string
	call     string
	code     frameCode
	data     interface{}
}

func frameStatusToCode(s SermoStatus) frameCode {
	switch s.(type) {
	case SermoRequest:
		return codeRequest
	case SermoItem:
		return codeItem
	case SermoByte:
		return codeByte
	case SermoBulk:
		return codeBulk
	case SermoDone:
		return codeDone
	case SermoError:
		return codeError
	case SermoCancel:
		return codeCancel
	default:
		return codeItem
	}
}

func frameRecordFromUser[T any](frame SermoScrinium[T]) frameRecord {
	id := ""
	if frame.Id != nil {
		id = *frame.Id
	}
	parentID := ""
	if frame.Parent_id != nil {
		if text, ok := (*frame.Parent_id).(string); ok {
			parentID = text
		}
	}
	call := ""
	if frame.Call != nil {
		call = *frame.Call
	}
	return frameRecord{
		id:       id,
		parentID: parentID,
		call:     call,
		code:     frameStatusToCode(frame.Status),
		data:     frame.Data,
	}
}

type frameTier int

const (
	tierNone frameTier = iota
	tierStatic
	tierBuiltin
	tierHost
)

// SermoConversation is the record shared by every copy of one sermo and by the
// goroutine answering it. The frame queue is a buffered channel; the record
// fields under mu are the facts A7 closes over.
type SermoConversation struct {
	id               string
	route            string
	frames           chan frameRecord
	cancelled        chan struct{}
	cancelOnce       sync.Once
	releaseOnce      sync.Once
	incomingDrained  bool
	incomingTerminal frameCode
	mu               sync.Mutex
	tier             frameTier
	task             bool
	finished         bool
	inboundTerminal  frameCode
	inboundData      interface{}
	outboundClosed   bool
	released         bool
	closed           bool
}

// liveConversations counts router slots held by unreleased conversations
// (D8.3).
var liveConversations int64

const frameQueueDepth = 64

type frameCancelled struct{}

func newConversation(id string, route string) *SermoConversation {
	atomic.AddInt64(&liveConversations, 1)
	return &SermoConversation{
		id:               id,
		route:            route,
		frames:           make(chan frameRecord, frameQueueDepth),
		cancelled:        make(chan struct{}),
		incomingTerminal: codeRequest,
		inboundTerminal:  codeRequest,
	}
}

func convSetTier(conv *SermoConversation, tier frameTier, task bool) {
	conv.mu.Lock()
	conv.tier = tier
	conv.task = task
	conv.mu.Unlock()
}

// convCancel signals the answering task; it is idempotent.
func convCancel(conv *SermoConversation) {
	conv.cancelOnce.Do(func() { close(conv.cancelled) })
}

// SermoConvItem pushes one deep-copied item frame. When the caller has
// cancelled, it unwinds the handler task with frameCancelled instead.
func SermoConvItem(conv *SermoConversation, data interface{}) {
	frame := frameRecord{id: frameNextID(), parentID: conv.id, call: conv.route, code: codeItem, data: deepCopy(data)}
	select {
	case <-conv.cancelled:
		panic(frameCancelled{})
	default:
	}
	select {
	case conv.frames <- frame:
	case <-conv.cancelled:
		panic(frameCancelled{})
	}
}

// convTerminal sends the single inbound terminal; a done after a cancel
// becomes a cancel terminal.
func convTerminal(conv *SermoConversation, code frameCode, data interface{}) {
	conv.mu.Lock()
	if conv.finished {
		conv.mu.Unlock()
		return
	}
	select {
	case <-conv.cancelled:
		if code == codeDone {
			code = codeCancel
			data = nil
		}
	default:
	}
	conv.finished = true
	conv.inboundTerminal = code
	conv.inboundData = data
	conv.mu.Unlock()
	conv.frames <- frameRecord{id: frameNextID(), parentID: conv.id, call: conv.route, code: code, data: data}
}

// convRelease is the single release point: it drops the task handle and gives
// the router slot back.
func convRelease(conv *SermoConversation) {
	conv.releaseOnce.Do(func() {
		conv.mu.Lock()
		conv.task = false
		conv.released = true
		conv.mu.Unlock()
		atomic.AddInt64(&liveConversations, -1)
	})
}

// convDiscard ends the inbound direction for a caller that will read no more:
// a live handler is cancelled, and the remaining frames are drained in the
// background so the handler never blocks on a full queue.
func convDiscard(conv *SermoConversation) {
	convCancel(conv)
	conv.incomingDrained = true
	go func() {
		for frame := range conv.frames {
			if frameIsTerminal(frame.code) {
				return
			}
		}
	}()
}

// frameCaller is the caller's token for one conversation, shared by every copy
// of the sermo. The handler goroutine never holds it, so its finalizer is the
// last-reference net (D8.3): eventual, with no timing promise.
type frameCaller struct {
	conv *SermoConversation
}

func callerAbandoned(caller *frameCaller) {
	conv := caller.conv
	conv.mu.Lock()
	closed := conv.closed
	conv.closed = true
	conv.mu.Unlock()
	if !closed && !conv.incomingDrained {
		convDiscard(conv)
	}
	if conv.tier != tierStatic && conv.tier != tierHost {
		convRelease(conv)
	}
}

func convCloseOutbound(conv *SermoConversation) {
	conv.mu.Lock()
	conv.outboundClosed = true
	conv.mu.Unlock()
}

// convStartTask runs a tier-1 handler as its own goroutine. A panic becomes an
// error terminal, a cancel unwinds to a cancel terminal.
func convStartTask(conv *SermoConversation, opener interface{}, handler func(*SermoConversation, interface{}) interface{}) {
	convSetTier(conv, tierStatic, true)
	go func() {
		defer convRelease(conv)
		defer func() {
			recovered := recover()
			if recovered == nil {
				return
			}
			if _, ok := recovered.(frameCancelled); ok {
				convTerminal(conv, codeCancel, nil)
				return
			}
			convTerminal(conv, codeError, fmt.Sprint(recovered))
		}()
		if failure := handler(conv, deepCopy(opener)); failure != nil {
			convTerminal(conv, codeError, failure)
			return
		}
		convTerminal(conv, codeDone, nil)
	}()
}

// deepCopy copies a value across the conversation boundary (D6.14). Exported
// struct fields, pointers, slices, maps, arrays and interfaces are copied;
// aliasing and cycles through pointers are preserved.
func deepCopy(value interface{}) interface{} {
	if value == nil {
		return nil
	}
	return deepCopyValue(reflect.ValueOf(value), map[uintptr]reflect.Value{}).Interface()
}

func deepCopyValue(value reflect.Value, seen map[uintptr]reflect.Value) reflect.Value {
	switch value.Kind() {
	case reflect.Pointer:
		if value.IsNil() {
			return value
		}
		if copied, ok := seen[value.Pointer()]; ok {
			return copied
		}
		out := reflect.New(value.Elem().Type())
		seen[value.Pointer()] = out
		out.Elem().Set(deepCopyValue(value.Elem(), seen))
		return out
	case reflect.Slice:
		if value.IsNil() {
			return value
		}
		out := reflect.MakeSlice(value.Type(), value.Len(), value.Len())
		for i := 0; i < value.Len(); i++ {
			out.Index(i).Set(deepCopyValue(value.Index(i), seen))
		}
		return out
	case reflect.Map:
		if value.IsNil() {
			return value
		}
		out := reflect.MakeMapWithSize(value.Type(), value.Len())
		entries := value.MapRange()
		for entries.Next() {
			out.SetMapIndex(deepCopyValue(entries.Key(), seen), deepCopyValue(entries.Value(), seen))
		}
		return out
	case reflect.Array:
		out := reflect.New(value.Type()).Elem()
		for i := 0; i < value.Len(); i++ {
			out.Index(i).Set(deepCopyValue(value.Index(i), seen))
		}
		return out
	case reflect.Struct:
		out := reflect.New(value.Type()).Elem()
		out.Set(value)
		for i := 0; i < value.NumField(); i++ {
			if out.Field(i).CanSet() {
				out.Field(i).Set(deepCopyValue(value.Field(i), seen))
			}
		}
		return out
	case reflect.Interface:
		if value.IsNil() {
			return value
		}
		out := reflect.New(value.Type()).Elem()
		out.Set(deepCopyValue(value.Elem(), seen))
		return out
	default:
		return value
	}
}
