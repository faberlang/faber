"""Faber `d64` in Python integers. A d64 is an i64 `raw` meaning raw * 10**-8.

Rules reproduced (grammar/EBNF.md "d64 is the one decimal width"):
  + and -  exact (aligned to the larger scale);   *  exact, scales add (8+8 -> 16);
  /        half-even at the larger operand scale; every intermediate is bounded by an
           i128 carrier (past it: Trap);
  store    half-even to scale 8, then the i64 range check (past it: Trap). Only trapping.
Expressions are never rounded per operation: hot code holds exact ints at a known
scale and calls `st` once per assignment. `Q` is the same rules as an object, used
by the tests to cross-check the raw-integer formulas.
"""
S = 10**8
I64_MIN, I64_MAX = -(1 << 63), (1 << 63) - 1
I128_MIN, I128_MAX = -(1 << 127), (1 << 127) - 1


class Trap(ArithmeticError):
    pass


def rne_div(n, d):
    """n / d rounded half-even to an integer."""
    if d == 0:
        raise Trap("division by zero")
    if d < 0:
        n, d = -n, -d
    q, r = divmod(n, d)
    if 2 * r > d or (2 * r == d and q & 1):
        q += 1
    return q


def carrier(v):
    if not I128_MIN <= v <= I128_MAX:
        raise Trap("128-bit carrier")
    return v


def st(v, scale=8):
    """A store: exact `v` at `scale` lands in a d64 slot (half-even to scale 8, range checked)."""
    carrier(v)
    if scale > 8:
        v = rne_div(v, 10 ** (scale - 8))
    if not I64_MIN <= v <= I64_MAX:
        raise Trap("d64 range")
    return v


def D(text):
    """A decimal literal. Needs a '.', at most 8 fraction digits (never silently rounded)."""
    if "." not in text:
        raise ValueError("integer literal in a decimal context: write " + text + ".0")
    neg, t = text.startswith("-"), text.lstrip("+-")
    ip, fp = t.split(".")
    if len(fp) > 8:
        raise ValueError("decimal literal with more than 8 fraction digits")
    return st((-1 if neg else 1) * (int(ip or "0") * S + int((fp + "0" * 8)[:8])))


def div8(a, b):
    """d64 / d64 stored to d64 (rounded at scale 8, the larger operand scale)."""
    return st(rne_div(a * S, b))


def show(raw):
    """Faber display form: shortest decimal, trailing zeros dropped (12.5, 12; never 12.50 or 12.0)."""
    ip, fp = divmod(abs(raw), S)
    frac = ("%08d" % fp).rstrip("0")
    return ("-" if raw < 0 else "") + str(ip) + ("." + frac if frac else "")


class Q:
    """An exact expression value: integer `v` at decimal scale `s`."""
    def __init__(self, v, s=8):
        self.v, self.s = carrier(v), s

    @staticmethod
    def lit(text):
        return Q(D(text))

    def _al(self, o):
        s = max(self.s, o.s)
        return s, self.v * 10 ** (s - self.s), o.v * 10 ** (s - o.s)

    def __add__(self, o):
        s, a, b = self._al(o)
        return Q(a + b, s)

    def __sub__(self, o):
        s, a, b = self._al(o)
        return Q(a - b, s)

    def __mul__(self, o):
        return Q(self.v * o.v, self.s + o.s)

    def __truediv__(self, o):
        s = max(self.s, o.s)
        return Q(rne_div(self.v * 10 ** (o.s + s), o.v * 10 ** self.s), s)

    def __neg__(self):
        return Q(-self.v, self.s)

    def cmp(self, o):
        _, a, b = self._al(o)
        return (a > b) - (a < b)

    def store(self):
        return st(self.v, self.s)
