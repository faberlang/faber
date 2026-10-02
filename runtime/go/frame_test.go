package rt

import (
	"reflect"
	"testing"
)

// TestFrameStatusCodes pins the status-to-code map for every variant and the
// terminal set (done, error, cancel).
func TestFrameStatusCodes(t *testing.T) {
	cases := []struct {
		status   SermoStatus
		code     frameCode
		terminal bool
	}{
		{SermoRequest{}, codeRequest, false},
		{SermoItem{}, codeItem, false},
		{SermoByte{}, codeByte, false},
		{SermoBulk{}, codeBulk, false},
		{SermoDone{}, codeDone, true},
		{SermoError{}, codeError, true},
		{SermoCancel{}, codeCancel, true},
	}
	for _, c := range cases {
		if got := frameStatusToCode(c.status); got != c.code {
			t.Fatalf("%T maps to %d, want %d", c.status, got, c.code)
		}
		if frameIsTerminal(c.code) != c.terminal {
			t.Fatalf("%T terminal = %v, want %v", c.status, !c.terminal, c.terminal)
		}
	}
}

// TestFrameRecordFromUser pins the user-frame conversion: absent optional
// fields read as empty and a non-text parent id is dropped.
func TestFrameRecordFromUser(t *testing.T) {
	id, call := "a", "route"
	var parent interface{} = "p"
	record := frameRecordFromUser(SermoScrinium[int]{Id: &id, Parent_id: &parent, Call: &call, Status: SermoDone{}, Data: 4})
	if record.id != "a" || record.parentID != "p" || record.call != "route" || record.code != codeDone || record.data != 4 {
		t.Fatalf("record = %+v", record)
	}
	var number interface{} = 3
	record = frameRecordFromUser(SermoScrinium[int]{Parent_id: &number})
	if record.id != "" || record.parentID != "" || record.call != "" {
		t.Fatalf("empty record = %+v", record)
	}
}

type deepNode struct {
	Name string
	Next *deepNode
	List []int
	Meta map[string]int
}

// TestDeepCopyCopiesAndPreservesAliasing pins the boundary copy (D6.14):
// pointers, slices and maps are copied, and a cycle stays a cycle.
func TestDeepCopyCopiesAndPreservesAliasing(t *testing.T) {
	node := &deepNode{Name: "a", List: []int{1, 2}, Meta: map[string]int{"k": 1}}
	node.Next = node
	copied := deepCopy(node).(*deepNode)
	if copied == node || copied.Next != copied {
		t.Fatalf("copy is aliased or lost its cycle")
	}
	copied.List[0] = 9
	copied.Meta["k"] = 9
	if node.List[0] != 1 || node.Meta["k"] != 1 {
		t.Fatalf("copy shares storage with the original")
	}
	if !reflect.DeepEqual(deepCopy([]string{"x"}), []string{"x"}) || deepCopy(nil) != nil {
		t.Fatalf("plain values do not round trip")
	}
}

// TestConversationReleaseIsASinglePoint pins D8.3: the release drops the task
// handle and marks the record released, however many times it is reached.
func TestConversationReleaseIsASinglePoint(t *testing.T) {
	conv := newConversation("c", "route")
	convSetTier(conv, tierStatic, true)
	if !conv.task || conv.tier != tierStatic {
		t.Fatalf("tier and task were not recorded")
	}
	convRelease(conv)
	convRelease(conv)
	if !conv.released || conv.task {
		t.Fatalf("release did not drop the task handle")
	}
}

// TestConversationRoute pins the route accessor a generated adapter uses.
func TestConversationRoute(t *testing.T) {
	if got := newConversation("c", "salve:dic").Route(); got != "salve:dic" {
		t.Fatalf("route = %q", got)
	}
}
