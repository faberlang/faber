package rt

import (
	"reflect"
	"testing"
)

// TestIntervallumMembershipAndLength pins the half-open (kind 0) and inclusive
// (kind 1) bounds in both directions.
func TestIntervallumMembershipAndLength(t *testing.T) {
	up := IntervallumValue(0, 10, 0)
	if !up.Continet(0) || !up.Continet(9) || up.Continet(10) || up.Continet(-1) {
		t.Fatalf("half-open ascending membership is wrong")
	}
	if up.Longitudo() != 10 {
		t.Fatalf("half-open length = %d", up.Longitudo())
	}
	incl := IntervallumValue(0, 10, 1)
	if !incl.Continet(10) || incl.Longitudo() != 11 {
		t.Fatalf("inclusive membership or length is wrong")
	}
	down := IntervallumValue(10, 0, 0)
	if !down.Continet(10) || down.Continet(0) || down.Longitudo() != 10 {
		t.Fatalf("half-open descending interval is wrong")
	}
	if !reflect.DeepEqual(IntervallumValue(3, 6, 0).AdLista(), []int{3, 4, 5}) {
		t.Fatalf("AdLista ascending")
	}
	if !reflect.DeepEqual(IntervallumValue(5, 2, 0).AdLista(), []int{5, 4, 3}) {
		t.Fatalf("AdLista descending")
	}
	if len(IntervallumValue(4, 4, 0).AdLista()) != 0 {
		t.Fatalf("empty interval lists nothing")
	}
}

// TestIntervallumCoercere pins clamping into the valid values of each kind.
func TestIntervallumCoercere(t *testing.T) {
	half := IntervallumValue(0, 10, 0)
	if half.Coercere(-5) != 0 || half.Coercere(50) != 9 || half.Coercere(4) != 4 {
		t.Fatalf("half-open coercion is wrong")
	}
	incl := IntervallumValue(0, 10, 1)
	if incl.Coercere(50) != 10 {
		t.Fatalf("inclusive coercion is wrong")
	}
	desc := IntervallumValue(10, 0, 0)
	if desc.Coercere(-5) != 1 || desc.Coercere(50) != 10 {
		t.Fatalf("descending half-open coercion is wrong")
	}
	got := IntervallumValue(-5, 50, 1).CoercereIntervallum(half)
	// Both bounds clamp into the target and the result takes the target's kind.
	if got.Continet(9) || !got.Continet(8) || got.Longitudo() != 9 {
		t.Fatalf("CoercereIntervallum did not clamp both bounds into the target")
	}
}

// TestIntervallumInterAndUnion pins the set operations and the nil result for
// disjoint intervals.
func TestIntervallumInterAndUnion(t *testing.T) {
	a := IntervallumValue(0, 10, 1)
	b := IntervallumValue(5, 15, 1)
	inter := a.Inter(b)
	if inter == nil || !reflect.DeepEqual(inter.AdLista(), []int{5, 6, 7, 8, 9, 10}) {
		t.Fatalf("Inter = %v", inter)
	}
	union := a.Union(b)
	if union == nil || union.Longitudo() != 16 {
		t.Fatalf("Union = %v", union)
	}
	far := IntervallumValue(20, 30, 1)
	if a.Inter(far) != nil || a.Union(far) != nil {
		t.Fatalf("disjoint intervals must have no intersection and no union")
	}
	adjacent := IntervallumValue(11, 20, 1)
	if u := a.Union(adjacent); u == nil || u.Longitudo() != 21 {
		t.Fatalf("adjacent intervals union = %v", u)
	}
}
