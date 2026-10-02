package rt

import (
	"strings"
	"testing"
	"time"
)

// installRoutes installs a static route table for one test and restores the
// previous table and host hook on cleanup.
func installRoutes(t *testing.T, table func(string) func(*FrameConversation, interface{}) interface{}) {
	t.Helper()
	previousRoutes, previousHost := staticRoutes, SermoHostDispatch
	t.Cleanup(func() {
		staticRoutes, SermoHostDispatch = previousRoutes, previousHost
	})
	SermoInstallRoutes(table)
	SermoHostDispatch = nil
}

func openWith(route string, opener interface{}) SermoSermo {
	s := SermoOpen(route)
	SermoSetOpener(&s, opener)
	return s
}

// TestSermoEchoRoundTrip pins the builtin tier: `runtime:echo` answers one item
// frame and a done frame through the directional views.
func TestSermoEchoRoundTrip(t *testing.T) {
	installRoutes(t, nil)
	s := openWith("runtime:echo", "salve")
	out := SermoAsMeus[string](&s)
	out.Da("ping")
	if _, ok := out.Fini().(FrameDone); !ok {
		t.Fatalf("meus fini is not done")
	}
	in := SermoAsTuus[string](&s)
	frames := in.Cursor()
	if len(frames) != 1 || frames[0].Data != "salve" {
		t.Fatalf("echo frames = %v", frames)
	}
	if _, ok := in.Fini().(FrameDone); !ok {
		t.Fatalf("tuus fini is not done")
	}
	second := openWith("runtime:echo", "salve")
	got := SermoAsTuus[string](&second).Accipe()
	if got == nil || got.Data != "salve" {
		t.Fatalf("accipe = %v", got)
	}
}

// TestSermoRouterOrder pins the router order: static table, then builtin, then
// host hook, then fail closed.
func TestSermoRouterOrder(t *testing.T) {
	installRoutes(t, func(route string) func(*FrameConversation, interface{}) interface{} {
		if route == "runtime:echo" || route == "salve:dic" {
			return func(conv *FrameConversation, opener interface{}) interface{} {
				FrameConvItem(conv, "static:"+opener.(string))
				return nil
			}
		}
		return nil
	})
	// A static route shadows the builtin of the same name.
	if got := SermoReadText(ptr(openWith("runtime:echo", "x"))); got != "static:x" {
		t.Fatalf("static tier lost to builtin: %q", got)
	}
	// An unserved builtin still answers.
	installRoutes(t, nil)
	if got := SermoReadText(ptr(openWith("runtime:echo", "y"))); got != "y" {
		t.Fatalf("builtin tier = %q", got)
	}
	// The host hook answers what the program does not serve.
	SermoHostDispatch = func(conv *FrameConversation, opener interface{}) bool {
		FrameConvItem(conv, "host")
		convTerminal(conv, codeDone, nil)
		convRelease(conv)
		return true
	}
	if got := SermoReadText(ptr(openWith("other:route", nil))); got != "host" {
		t.Fatalf("host tier = %q", got)
	}
	// With no hook the conversation fails closed.
	SermoHostDispatch = nil
	failed := openWith("other:route", nil)
	defer func() {
		if recovered := recover(); recovered == nil || !strings.Contains(recovered.(string), "unsupported ad route `other:route`") {
			t.Fatalf("fail-closed terminal = %v", recovered)
		}
	}()
	SermoReadText(&failed)
}

func ptr[T any](value T) *T { return &value }

// TestSermoHandlerRunsAsGoroutineAndStreams pins a cursor handler: each item
// is its own frame, and the opener is deep copied across the boundary.
func TestSermoHandlerRunsAsGoroutineAndStreams(t *testing.T) {
	opener := []int{1, 2, 3}
	installRoutes(t, func(route string) func(*FrameConversation, interface{}) interface{} {
		if route != "num:numeros" {
			return nil
		}
		return func(conv *FrameConversation, got interface{}) interface{} {
			list := got.([]int)
			list[0] = 99 // writes the copy, never the caller's slice
			for _, n := range list {
				FrameConvItem(conv, n)
			}
			return nil
		}
	})
	s := openWith("num:numeros", opener)
	frames := SermoAsTuus[int](&s).Cursor()
	if len(frames) != 3 || frames[0].Data != 99 || frames[2].Data != 3 {
		t.Fatalf("frames = %v", frames)
	}
	if opener[0] != 1 {
		t.Fatalf("opener was aliased across the boundary: %v", opener)
	}
}

// TestSermoFailureBecomesErrorTerminal pins a handler failure payload and a
// handler panic as an error terminal; ReadNumerus reports it as an error.
func TestSermoFailureBecomesErrorTerminal(t *testing.T) {
	installRoutes(t, func(route string) func(*FrameConversation, interface{}) interface{} {
		switch route {
		case "x:div":
			return func(conv *FrameConversation, opener interface{}) interface{} { return "nulla" }
		case "x:boom":
			return func(conv *FrameConversation, opener interface{}) interface{} { panic("boom") }
		}
		return nil
	})
	for route, want := range map[string]string{"x:div": "nulla", "x:boom": "boom"} {
		s := openWith(route, nil)
		_, err := SermoReadNumerus(&s)
		if err == nil || err.(error).Error() != want {
			t.Fatalf("%s error = %v, want %s", route, err, want)
		}
	}
}

// TestSermoCancelReachesTheHandler pins the cancel path: tuus.fini() on an
// undrained stream cancels the answering task and reports a cancel status.
func TestSermoCancelReachesTheHandler(t *testing.T) {
	stopped := make(chan struct{})
	installRoutes(t, func(route string) func(*FrameConversation, interface{}) interface{} {
		if route != "num:endless" {
			return nil
		}
		return func(conv *FrameConversation, opener interface{}) interface{} {
			defer close(stopped)
			for i := 0; ; i++ {
				FrameConvItem(conv, i)
			}
		}
	})
	s := openWith("num:endless", nil)
	status := SermoAsTuus[int](&s).Fini()
	if _, ok := status.(FrameCancel); !ok {
		t.Fatalf("fini status = %T, want FrameCancel", status)
	}
	select {
	case <-stopped:
	case <-time.After(5 * time.Second):
		t.Fatalf("handler did not observe the cancel")
	}
}

// TestSermoMaterializers pins the textus, numerus and vacuum reads.
func TestSermoMaterializers(t *testing.T) {
	installRoutes(t, nil)
	if got := SermoReadText(ptr(openWith("runtime:echo", "salve"))); got != "salve" {
		t.Fatalf("text = %q", got)
	}
	if n, err := SermoReadNumerus(ptr(openWith("runtime:echo", 7))); n != 7 || err != nil {
		t.Fatalf("numerus = %d, %v", n, err)
	}
	if n, err := SermoReadNumerus(ptr(openWith("runtime:echo", "12"))); n != 12 || err != nil {
		t.Fatalf("numerus from text = %d, %v", n, err)
	}
	if _, err := SermoReadNumerus(ptr(openWith("runtime:echo", 1.5))); err == nil {
		t.Fatalf("numerus from a float frame did not fail")
	}
	SermoDrain(ptr(openWith("runtime:echo", "x")))
}

// TestSermoClose pins close(): a finished handle reports true once, a second
// close reports false, and a failed handler's error is returned.
func TestSermoClose(t *testing.T) {
	installRoutes(t, func(route string) func(*FrameConversation, interface{}) interface{} {
		if route == "x:div" {
			return func(conv *FrameConversation, opener interface{}) interface{} { return "nulla" }
		}
		return nil
	})
	echo := openWith("runtime:echo", "x")
	if closed, err := SermoClose(&echo); !closed || err != nil {
		t.Fatalf("close = %v, %v", closed, err)
	}
	if closed, err := SermoClose(&echo); closed || err != nil {
		t.Fatalf("second close = %v, %v", closed, err)
	}
	failed := openWith("x:div", nil)
	for liveWait(&failed) {
	}
	if closed, err := SermoClose(&failed); closed || err != "nulla" {
		t.Fatalf("failed close = %v, %v", closed, err)
	}
	if closed, err := SermoClose(nil); closed || err != nil {
		t.Fatalf("nil close = %v, %v", closed, err)
	}
}

// liveWait spins until the handler goroutine has sent its terminal.
func liveWait(s *SermoSermo) bool {
	s.conv.mu.Lock()
	finished := s.conv.finished
	s.conv.mu.Unlock()
	if !finished {
		time.Sleep(time.Millisecond)
	}
	return !finished
}
