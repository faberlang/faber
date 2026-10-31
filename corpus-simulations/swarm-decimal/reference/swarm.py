"""Swarm: a 3D predator/prey/food simulation in fixed-point d64 (see ../README.md).

Naming: an int named `*16` is an exact scale-16 intermediate (a product of two d64 raws);
every other int is a d64 raw (value * 10**8). `st(x, 16)` is a STORE point: the one place a
value is rounded half-even to scale 8 and range-checked. Python standard library only.
"""
import hashlib
import random
import sys
from itertools import accumulate
from struct import pack

from d64 import S, D, st, div8, show

M64 = (1 << 64) - 1
PREY_STATES = ("wander", "flock", "flee", "graze")
PRED_STATES = ("wander", "chase", "eat", "rest")
WANDER, FLOCK, FLEE, GRAZE = 0, 1, 2, 3          # prey states
CHASE, EAT, REST = 1, 2, 3                       # predator states (WANDER = 0 too)

# ---- parameters (flat; the Faber port embeds them verbatim) ----
SEED, TICKS = 20261028, 500
NPREY, NPRED, NFOOD = 800, 150, 50
PREY_MAX, PRED_MAX = 1600, 400                   # births past the cap are skipped, in id order
WX, WY, WZ = D("200.0"), D("200.0"), D("50.0")   # box [0,W] metres
DT = D("0.25")
CELL = D("10.0")                                 # grid cell edge; NX*NY*NZ cells cover the box
NX, NY, NZ = 20, 20, 5
NCELL = NX * NY * NZ
ZERO, HALF, ONE, TWO, THREE, FOUR = D("0.0"), D("0.5"), D("1.0"), D("2.0"), D("3.0"), D("4.0")
QUARTER = D("0.25")
RS_A, RS_B = D("2.33333333"), D("1.33333333")    # rsqrt initial guess y0 = A - B*x on [0.25, 1)
TURN = D("0.4")                                  # velocity moves 40% of the way to the desired one
# radii (metres); squared values are exact scale-16 constants, compared against exact d2
R_FLEE, R_FLOCK, R_SEP = D("8.0"), D("10.0"), D("3.0")
R_FOOD, R_GRAZE = D("30.0"), D("3.0")
R_SENSE, R_CONTACT = D("14.0"), D("1.5")
R_FLEE2, R_FLOCK2, R_SEP2 = R_FLEE * R_FLEE, R_FLOCK * R_FLOCK, R_SEP * R_SEP
R_FOOD2, R_GRAZE2, R_SENSE2, R_CONTACT2 = R_FOOD * R_FOOD, R_GRAZE * R_GRAZE, R_SENSE * R_SENSE, R_CONTACT * R_CONTACT
# steering weights, speeds (m/s), speed caps
W_SEP, W_ALI, W_COH, W_FOOD = D("1.5"), D("1.0"), D("0.5"), D("1.5")
KEEP, JITTER = D("1.0"), D("4.0")
PREY_WANDER_V, PREY_FLOCK_V, PREY_FLEE_V, PREY_VMAX = D("2.0"), D("4.0"), D("6.0"), D("6.5")
PRED_WANDER_V, PRED_CHASE_V, PRED_VMAX = D("2.5"), D("7.0"), D("7.5")
PREY_VMAX2, PRED_VMAX2 = PREY_VMAX * PREY_VMAX, PRED_VMAX * PRED_VMAX
# energy, food
PREY_E0, PREY_E0_SPAN, PRED_E0, PRED_E0_SPAN = D("40.0"), D("20.0"), D("50.0"), D("30.0")
PREY_COST, PREY_COSTV, E_HUNGRY, PREY_REPRO = D("0.10"), D("0.002"), D("100.0"), D("90.0")
PRED_COST, PRED_REST_COST, PRED_COSTV = D("0.22"), D("0.10"), D("0.001")
E_SATED, PRED_REPRO, MEAL = D("80.0"), D("100.0"), D("40.0")
FOOD_CAP, FOOD_REGROW, BITE, FOOD_ENERGY = D("60.0"), D("0.8"), D("2.0"), D("5.0")
SPREAD, BIRTH_V = D("2.0"), D("1.0")


# ---- counter-based RNG: SplitMix64 finalizer keyed by (seed, tick, entity id, purpose) ----
def mix(z):
    z = (z + 0x9E3779B97F4A7C15) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def rnd(tick, ident, purpose):
    return mix(mix(mix(mix(SEED) ^ tick) ^ ident) ^ purpose)


def unit(h):
    """d64 raw uniform over the 2*10**8 + 1 raw values of [-1.0, 1.0] (integer remainder)."""
    return h % (2 * S + 1) - S


# purposes: 1-3 wander jitter | 4-6 birth offset | 7-9 birth velocity | 10-15 init x y z vx vy vz | 16 init energy
P_WANDER, P_OFFSET, P_BIRTHV, P_INIT, P_INIT_E = 1, 4, 7, 10, 16


# ---- numeric kernels ----
def rsqrt(x):
    """~1/sqrt(x) for a d64 x > 0: reduce by 4 into [0.25, 1), linear guess, 3 Newton steps
    y <- y*(3 - x*y*y)/2, undo the reduction. Relative error < 3e-5. Stores: every `x`/`y`/`h` assignment.
    (Written as one expression the update is scale 40 and its raw value 3.5e40 passes the 2^127 carrier: a trap.)"""
    k = 0
    while x >= ONE:
        x, k = st(x * QUARTER, 16), k + 1            # x <- x * 0.25
    while x < QUARTER:
        x, k = st(x * FOUR, 16), k - 1               # x <- x * 4.0
    y = st(RS_A * S - RS_B * x, 16)                  # y <- 2.33333333 - 1.33333333 * x
    for _ in range(3):
        h = st(x * y * y, 24)                        # h <- x*y*y   (stored: x*y*y*y would be scale 32+)
        y = st(y * (THREE - h) * HALF, 24)           # y <- y * (3.0 - h) * 0.5
    for _ in range(k):
        y = st(y * HALF, 16)                         # y <- y * 0.5
    for _ in range(-k):
        y = st(y * TWO, 16)                          # y <- y * 2.0
    return y


def aim(dx, dy, dz, speed):
    """Desired velocity: the direction of (dx,dy,dz) at `speed`. d2 and s are stores; a zero d2 gives rest."""
    d2 = st(dx * dx + dy * dy + dz * dz, 16)
    if d2 == 0:
        return 0, 0, 0
    s = st(speed * rsqrt(d2), 16)                    # s <- speed * rsqrt(d2)
    return st(dx * s, 16), st(dy * s, 16), st(dz * s, 16)


def cap_speed(vx, vy, vz, vmax, vmax2):
    """If |v|^2 > vmax^2 (exact scale-16 compare), v <- v * (vmax * rsqrt(v2)); else unchanged."""
    v2 = vx * vx + vy * vy + vz * vz
    if v2 <= vmax2:
        return vx, vy, vz
    f = st(vmax * rsqrt(st(v2, 16)), 16)
    return st(vx * f, 16), st(vy * f, 16), st(vz * f, 16)


def wall(p, v, w):
    """Reflect at 0 and w (exact); one reflection suffices since |v|*dt << w."""
    if p < 0:
        return -p, -v
    if p > w:
        return st(w + w - p), -v
    return p, v


def clamp(p, w):
    return 0 if p < 0 else w if p > w else p


# ---- state ----
class Pop:
    """One kind, struct of arrays, always sorted by id (ids ascend with index)."""
    FIELDS = ("ids", "px", "py", "pz", "vx", "vy", "vz", "en", "st")

    def __init__(self, n=0):
        for f in self.FIELDS:
            setattr(self, f, [0] * n)

    @property
    def n(self):
        return len(self.ids)

    def add(self, *row):
        for f, v in zip(self.FIELDS, row):
            getattr(self, f).append(v)

    def compact(self, alive):
        """Order-preserving stream compaction: exclusive scan of the alive flags, then scatter."""
        pos = [0] + list(accumulate(alive))
        out = Pop(pos[-1])
        for f in self.FIELDS:
            src, dst = getattr(self, f), getattr(out, f)
            for i, a in enumerate(alive):
                if a:
                    dst[pos[i]] = src[i]
        return out


class Grid:
    """Uniform grid: count per cell, exclusive prefix sum, scatter of indices; ascending id within a cell."""
    def __init__(self, pop):
        cell = [self.cell(pop.px[i], pop.py[i], pop.pz[i]) for i in range(pop.n)]
        cnt = [0] * NCELL
        for c in cell:
            cnt[c] += 1
        start = [0] + list(accumulate(cnt))
        cur = start[:-1]
        items = [0] * pop.n
        for i, c in enumerate(cell):
            items[cur[c]] = i
            cur[c] += 1
        self.start, self.items = start, items

    @staticmethod
    def cell(x, y, z):
        return (min(z // CELL, NZ - 1) * NY + min(y // CELL, NY - 1)) * NX + min(x // CELL, NX - 1)

    def near(self, x, y, z, r):
        """Indices in cells overlapping the box [p-r, p+r], cell order z, y, x, then id."""
        xs, ys, zs = (range((p - r) // CELL if p > r else 0, min((p + r) // CELL, n - 1) + 1)
                      for p, n in ((x, NX), (y, NY), (z, NZ)))
        for cz in zs:
            for cy in ys:
                for cx in xs:
                    c = (cz * NY + cy) * NX + cx
                    yield from self.items[self.start[c]:self.start[c + 1]]


class World:
    def __init__(self):
        self.tick, self.next_id = 0, 0
        self.prey, self.pred, self.food = Pop(), Pop(), Pop()
        for i in range(NPREY + NPRED):
            pop, e0, span = (self.prey, PREY_E0, PREY_E0_SPAN) if i < NPREY else (self.pred, PRED_E0, PRED_E0_SPAN)
            self.spawn_init(pop, e0 + rnd(0, i, P_INIT_E) % (span + 1))
        for _ in range(NFOOD):
            i = self.next_id
            self.food.add(i, *(rnd(0, i, P_INIT + a) % (w + 1) for a, w in enumerate((WX, WY, WZ))), 0, 0, 0, FOOD_CAP, 0)
            self.next_id += 1

    def spawn_init(self, pop, energy):
        i = self.next_id
        pos = [rnd(0, i, P_INIT + a) % (w + 1) for a, w in enumerate((WX, WY, WZ))]
        vel = [st(unit(rnd(0, i, P_INIT + 3 + a)) * TWO, 16) for a in range(3)]
        pop.add(i, *pos, *vel, energy, WANDER)
        self.next_id += 1

    # -- phase 2: sense and decide. Reads only the snapshot arrays and grids; returns the entity's own intent.
    def decide_prey(self, t, i):
        P, Q, F = self.prey, self.pred, self.food
        x, y, z, vx, vy, vz = P.px[i], P.py[i], P.pz[i], P.vx[i], P.vy[i], P.vz[i]
        best = None                                   # nearest predator in range: min (d2, id)
        for j in self.gq.near(x, y, z, R_FLEE):
            dx, dy, dz = x - Q.px[j], y - Q.py[j], z - Q.pz[j]
            d2 = dx * dx + dy * dy + dz * dz          # exact scale 16, no store
            if d2 <= R_FLEE2 and (best is None or (d2, Q.ids[j]) < best[0]):
                best = ((d2, Q.ids[j]), dx, dy, dz)
        if best:
            return FLEE, aim(best[1], best[2], best[3], PREY_FLEE_V), -1
        fbest = None                                  # nearest non-depleted food in sense range (hungry only)
        if P.en[i] < E_HUNGRY:
            for j in self.gf.near(x, y, z, R_FOOD):
                dx, dy, dz = F.px[j] - x, F.py[j] - y, F.pz[j] - z
                d2 = dx * dx + dy * dy + dz * dz
                if F.en[j] >= BITE and d2 <= R_FOOD2 and (fbest is None or (d2, F.ids[j]) < fbest[0]):
                    fbest = ((d2, F.ids[j]), dx, dy, dz, j)
            if fbest and fbest[0][0] <= R_GRAZE2:
                return GRAZE, (0, 0, 0), fbest[4]
        n = sx = sy = sz = svx = svy = svz = qx = qy = qz = 0   # neighbours in fixed order: cell, then id
        for j in self.gp.near(x, y, z, R_FLOCK):
            dx, dy, dz = x - P.px[j], y - P.py[j], z - P.pz[j]
            d2 = dx * dx + dy * dy + dz * dz
            if j != i and d2 <= R_FLOCK2:
                n += 1
                sx, sy, sz = sx + P.px[j], sy + P.py[j], sz + P.pz[j]
                svx, svy, svz = svx + P.vx[j], svy + P.vy[j], svz + P.vz[j]
                if d2 <= R_SEP2:
                    qx, qy, qz = qx + dx, qy + dy, qz + dz
        fx, fy, fz = (fbest[1], fbest[2], fbest[3]) if fbest else (0, 0, 0)
        if n:
            nd = n * S                                # n ↦ d64
            ax, ay, az = div8(svx, nd) - vx, div8(svy, nd) - vy, div8(svz, nd) - vz   # mean neighbour v, minus own
            cx, cy, cz = div8(sx, nd) - x, div8(sy, nd) - y, div8(sz, nd) - z         # centre of mass, minus own
            dx = st(W_SEP * qx + W_ALI * ax + W_COH * cx + W_FOOD * fx, 16)
            dy = st(W_SEP * qy + W_ALI * ay + W_COH * cy + W_FOOD * fy, 16)
            dz = st(W_SEP * qz + W_ALI * az + W_COH * cz + W_FOOD * fz, 16)
            return FLOCK, aim(dx, dy, dz, PREY_FLOCK_V), -1
        dx = st(KEEP * vx + JITTER * unit(rnd(t, P.ids[i], P_WANDER)) + W_FOOD * fx, 16)
        dy = st(KEEP * vy + JITTER * unit(rnd(t, P.ids[i], P_WANDER + 1)) + W_FOOD * fy, 16)
        dz = st(KEEP * vz + JITTER * unit(rnd(t, P.ids[i], P_WANDER + 2)) + W_FOOD * fz, 16)
        return WANDER, aim(dx, dy, dz, PREY_WANDER_V), -1

    def decide_pred(self, t, i):
        P, Q = self.prey, self.pred
        x, y, z = Q.px[i], Q.py[i], Q.pz[i]
        best = None                                   # nearest prey in range: min (d2, id)
        for j in self.gp.near(x, y, z, R_SENSE):
            dx, dy, dz = P.px[j] - x, P.py[j] - y, P.pz[j] - z
            d2 = dx * dx + dy * dy + dz * dz
            if d2 <= R_SENSE2 and (best is None or (d2, P.ids[j]) < best[0]):
                best = ((d2, P.ids[j]), dx, dy, dz, j)
        if best and best[0][0] <= R_CONTACT2:
            return EAT, (0, 0, 0), best[4], best[0][0]
        if Q.en[i] >= E_SATED:
            return REST, (0, 0, 0), -1, 0
        if best:
            return CHASE, aim(best[1], best[2], best[3], PRED_CHASE_V), -1, 0
        dx = st(KEEP * Q.vx[i] + JITTER * unit(rnd(t, Q.ids[i], P_WANDER)), 16)
        dy = st(KEEP * Q.vy[i] + JITTER * unit(rnd(t, Q.ids[i], P_WANDER + 1)), 16)
        dz = st(KEEP * Q.vz[i] + JITTER * unit(rnd(t, Q.ids[i], P_WANDER + 2)), 16)
        return WANDER, aim(dx, dy, dz, PRED_WANDER_V), -1, 0

    # -- phase 4: integrate one mover from its own intent and its resolved gain
    @staticmethod
    def move(pop, i, dv, gain, cost0, costv, vmax, vmax2):
        v = [st(getattr(pop, f)[i] * S + (d - getattr(pop, f)[i]) * TURN, 16) for f, d in zip(("vx", "vy", "vz"), dv)]
        v = list(cap_speed(*v, vmax, vmax2))
        p = [st(getattr(pop, f)[i] * S + vv * DT, 16) for f, vv in zip(("px", "py", "pz"), v)]
        for a, w in enumerate((WX, WY, WZ)):
            p[a], v[a] = wall(p[a], v[a], w)
        v2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2]  # exact scale 16
        pop.en[i] = st((pop.en[i] - cost0) * S * S + gain * S - costv * v2, 24)   # gain is scale 16; one store
        pop.px[i], pop.py[i], pop.pz[i] = p
        pop.vx[i], pop.vy[i], pop.vz[i] = v

    def step(self, shuffle=None):
        """One tick. `shuffle(list)` (tests only) permutes the order phase 2 visits entities."""
        t = self.tick + 1
        P, Q, F = self.prey, self.pred, self.food
        # 1. grids (one per kind)
        self.gp, self.gq, self.gf = Grid(P), Grid(Q), Grid(F)
        # 2. sense and decide: each entity writes only its own intent slot
        order_p, order_q = list(range(P.n)), list(range(Q.n))
        if shuffle:
            shuffle(order_p), shuffle(order_q)
        ip, iq = [None] * P.n, [None] * Q.n
        for i in order_p:
            ip[i] = self.decide_prey(t, i)
        for i in order_q:
            iq[i] = self.decide_pred(t, i)
        # 3. resolve conflicts. Predation first: a prey goes to the claimant with min (d2, predator id).
        win = {}
        for j in range(Q.n):
            if iq[j][2] >= 0 and (iq[j][2] not in win or (iq[j][3], Q.ids[j]) < win[iq[j][2]][0]):
                win[iq[j][2]] = ((iq[j][3], Q.ids[j]), j)
        eaten = [i in win for i in range(P.n)]
        meal = [ZERO] * Q.n
        for i, (_, j) in win.items():
            meal[j] = MEAL
        # food units go out in prey id order; an eaten prey receives nothing
        left, bite = F.en[:], [ZERO] * P.n
        for i in range(P.n):
            f = ip[i][2]
            if f >= 0 and not eaten[i]:
                bite[i] = min(BITE, left[f])
                left[f] -= bite[i]
        # 4. integrate (own slot only), then food regrowth
        for i in range(P.n):
            P.st[i] = ip[i][0]
            self.move(P, i, ip[i][1], bite[i] * FOOD_ENERGY, PREY_COST, PREY_COSTV, PREY_VMAX, PREY_VMAX2)
        for j in range(Q.n):
            Q.st[j] = iq[j][0]
            cost0 = PRED_REST_COST if iq[j][0] == REST else PRED_COST
            self.move(Q, j, iq[j][1], meal[j] * ONE, cost0, PRED_COSTV, PRED_VMAX, PRED_VMAX2)
        for f in range(F.n):
            F.en[f] = min(FOOD_CAP, left[f] + FOOD_REGROW)
        # 5. births (parent id order: prey, then predators), deaths, order-preserving compaction
        alive_p = [int(not eaten[i] and P.en[i] > 0) for i in range(P.n)]
        alive_q = [int(Q.en[j] > 0) for j in range(Q.n)]
        kids = []
        for pop, alive, repro, cap, count in ((P, alive_p, PREY_REPRO, PREY_MAX, sum(alive_p)),
                                              (Q, alive_q, PRED_REPRO, PRED_MAX, sum(alive_q))):
            born = Pop()
            for i in range(pop.n):
                if alive[i] and pop.en[i] >= repro and count < cap:
                    count += 1
                    self.birth(t, pop, i, born)
            kids.append(born)
        self.prey, self.pred = P.compact(alive_p), Q.compact(alive_q)
        for pop, born in ((self.prey, kids[0]), (self.pred, kids[1])):
            for f in Pop.FIELDS:
                getattr(pop, f).extend(getattr(born, f))
        self.tick = t

    def birth(self, t, pop, i, born):
        """Child next to the parent; energy split half-even (parent keeps e/2.0, child gets the rest)."""
        pid, cid = pop.ids[i], self.next_id
        self.next_id += 1
        pos = [clamp(p + st(unit(rnd(t, pid, P_OFFSET + a)) * SPREAD, 16), w)
               for a, (p, w) in enumerate(zip((pop.px[i], pop.py[i], pop.pz[i]), (WX, WY, WZ)))]
        vel = [st(unit(rnd(t, pid, P_BIRTHV + a)) * BIRTH_V, 16) for a in range(3)]
        half = div8(pop.en[i], TWO)                   # e / 2.0
        born.add(cid, *pos, *vel, pop.en[i] - half, WANDER)
        pop.en[i] = half

    # -- outputs
    def digest(self):
        h = hashlib.sha256()
        for kind, pop in ((0, self.prey), (1, self.pred), (2, self.food)):
            h.update(pack("<BI", kind, pop.n))
            for i in range(pop.n):
                row = (pop.ids[i], kind, pop.st[i], pop.px[i], pop.py[i], pop.pz[i])
                row += (pop.vx[i], pop.vy[i], pop.vz[i], pop.en[i]) if kind < 2 else (pop.en[i],)
                h.update(pack("<IBB" + "q" * (len(row) - 3), *row))
        return h.hexdigest()

    def summary_row(self):
        occ = lambda pop, k: sum(1 for s in pop.st if s == k)
        cols = [self.tick, self.prey.n, self.pred.n, show(sum(self.food.en))]
        return cols + [occ(self.prey, k) for k in range(4)] + [occ(self.pred, k) for k in range(4)]

    def snapshot_csv(self):
        rows = ["id,kind,state,x,y,z,vx,vy,vz,energy"]
        for kind, names, pop in (("prey", PREY_STATES, self.prey), ("predator", PRED_STATES, self.pred), ("food", None, self.food)):
            for i in range(pop.n):
                vel = [show(v) for v in (pop.vx[i], pop.vy[i], pop.vz[i])] if names else ["", "", ""]
                rows.append(",".join([str(pop.ids[i]), kind, names[pop.st[i]] if names else "grown",
                                      show(pop.px[i]), show(pop.py[i]), show(pop.pz[i])] + vel + [show(pop.en[i])]))
        return "\n".join(rows) + "\n"


SUMMARY_HEADER = "tick,prey,predators,food,prey_wander,prey_flock,prey_flee,prey_graze,pred_wander,pred_chase,pred_eat,pred_rest"


def simulate(ticks=TICKS, shuffle_seed=None):
    """Yield (tick, world) for ticks 0..ticks. With shuffle_seed, phase 2 visits entities in shuffled order."""
    w = World()
    shuffle = random.Random(shuffle_seed).shuffle if shuffle_seed is not None else None
    yield 0, w
    for _ in range(ticks):
        w.step(shuffle)
        yield w.tick, w


def main(argv):
    ticks = int(argv[1]) if len(argv) > 1 else TICKS
    out = argv[2] if len(argv) > 2 else None
    hashes, summary, snaps = [], [SUMMARY_HEADER], {}
    for t, w in simulate(ticks):
        hashes.append("%d %s" % (t, w.digest()))
        if t % 50 == 0:
            summary.append(",".join(map(str, w.summary_row())))
        if t in (0, 100, 500):
            snaps[t] = w.snapshot_csv()
    files = {"hashes.txt": "\n".join(hashes) + "\n", "summary.csv": "\n".join(summary) + "\n"}
    files.update({"snapshot_%d.csv" % t: s for t, s in snaps.items()})
    for name, text in files.items() if out else ():
        with open("%s/%s" % (out, name), "w") as f:
            f.write(text)
    if not out:
        sys.stdout.write(files["hashes.txt"])


if __name__ == "__main__":
    main(sys.argv)
