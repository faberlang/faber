"""Tests for the swarm reference. Run from this directory: python3 -m unittest test_swarm"""
import hashlib
import os
import random
import unittest
from functools import lru_cache
from math import sqrt

import swarm
from d64 import D, Q, S, Trap, I64_MAX, div8, rne_div, show, st

HERE = os.path.dirname(os.path.abspath(__file__))
GOLDEN = os.path.join(HERE, "golden")
GOLDEN_HASHES_SHA256 = "131c68e5cf0a9df62e8cb9247f59c593d321e6e1cb7625889273105e1738da7b"


def golden(name):
    with open(os.path.join(GOLDEN, name)) as f:
        return f.read()


@lru_cache(maxsize=None)
def full_run():
    """One 500-tick run: hash lines, summary rows, snapshots, per-tick populations."""
    hashes, summary, snaps, pops = [], [swarm.SUMMARY_HEADER], {}, []
    for t, w in swarm.simulate(swarm.TICKS):
        hashes.append("%d %s" % (t, w.digest()))
        pops.append((w.prey.n, w.pred.n, w.food.n))
        if t % 50 == 0:
            summary.append(",".join(map(str, w.summary_row())))
        if t in (0, 100, 500):
            snaps[t] = w.snapshot_csv()
    return hashes, summary, snaps, pops


def hashes_of(ticks, shuffle_seed=None):
    return ["%d %s" % (t, w.digest()) for t, w in swarm.simulate(ticks, shuffle_seed)]


class D64Helpers(unittest.TestCase):
    def test_literals(self):
        self.assertEqual(D("4.2"), 420000000)
        self.assertEqual(D("-0.00000001"), -1)
        self.assertEqual(D("92233720368.54775807"), I64_MAX)
        with self.assertRaises(ValueError):
            D("1")                                  # integer literal in a decimal context
        with self.assertRaises(ValueError):
            D("0.123456789")                        # more than 8 fraction digits

    def test_exact_add_sub_and_shortest_display(self):
        a, b = Q.lit("4.2"), Q.lit("0.1")
        self.assertEqual(show((a + b).store()), "4.3")
        self.assertEqual(show((a - b).store()), "4.1")
        self.assertEqual(show((a / b).store()), "42")           # never 42.0
        self.assertEqual([show(D(x)) for x in ("12.5", "12.0", "-0.5", "0.0", "0.00000001")],
                         ["12.5", "12", "-0.5", "0", "0.00000001"])

    def test_product_is_exact_at_scale_16(self):
        p = Q.lit("0.1") * Q.lit("0.1")
        self.assertEqual((p.s, p.v), (16, 10**14))             # 0.01 exactly at scale 16
        self.assertEqual(show((Q.lit("4.2") * Q.lit("0.1")).store()), "0.42")

    def test_store_rounds_half_even_once(self):
        s, s3, half, n = (Q.lit(x) for x in ("0.00000001", "0.00000003", "0.5", "-0.00000001"))
        self.assertEqual((s3 * half).store(), 2)                # 1.5e-8 -> 2e-8 (tie to even)
        self.assertEqual((s * half).store(), 0)                 # 5e-9 -> 0 (tie to even, not half-up)
        self.assertEqual((s * Q.lit("1.5")).store(), 2)
        self.assertEqual((n * half).store(), 0)
        self.assertEqual((n * Q.lit("1.5")).store(), -2)
        # one rounding for the whole expression, not one per operation
        self.assertEqual((s * half + s * half).store(), 1)      # 5e-9 + 5e-9 = 1e-8 exactly
        self.assertEqual(((s * half) * Q.lit("3.0")).store(), 2)

    def test_division_rounds_half_even_to_the_larger_scale(self):
        one, three = Q.lit("1.0"), Q.lit("3.0")
        q = one / three
        self.assertEqual((q.s, q.v), (8, 33333333))
        self.assertEqual((q * three).store(), 99999999)         # EBNF: 1.0 / 3.0 * 3.0 stores 0.99999999
        self.assertEqual((Q.lit("2.0") / three).v, 66666667)
        self.assertEqual((Q.lit("0.00000001") / Q.lit("2.0")).v, 0)    # 0.5 unit: tie to even
        self.assertEqual((Q.lit("0.00000003") / Q.lit("2.0")).v, 2)    # 1.5 units: tie to even
        self.assertEqual(div8(D("-0.00000003"), D("2.0")), -2)
        wide = (Q.lit("0.00000001") * Q.lit("0.5")) / Q.lit("2.0")     # scale 16 / scale 8 -> scale 16
        self.assertEqual((wide.s, wide.v), (16, rne_div(5 * 10**7, 2)))
        for n, d, want in ((5, 2, 2), (7, 2, 4), (-5, 2, -2), (-7, 2, -4), (5, -2, -2), (1, 3, 0), (2, 3, 1)):
            self.assertEqual(rne_div(n, d), want)

    def test_overflow_traps(self):
        top = Q.lit("92233720368.54775807")
        self.assertEqual((top + Q.lit("0.00000001") - Q.lit("0.00000001")).store(), I64_MAX)   # exact until the store
        with self.assertRaises(Trap):
            (top + Q.lit("0.00000001")).store()                 # one step past the range
        with self.assertRaises(Trap):
            (Q.lit("-92233720368.54775808") - Q.lit("0.00000001")).store()
        self.assertEqual(st(-(1 << 63)), -(1 << 63))
        _ = top * top                                           # scale 16, 8.5e37 < 2^127: fits the carrier
        with self.assertRaises(Trap):
            _ = top * top * top                                 # past the 128-bit carrier
        with self.assertRaises(Trap):
            Q.lit("1.0") / Q.lit("0.0")
        with self.assertRaises(Trap):                           # the Newton update as ONE expression is scale 40
            y, x = Q.lit("2.0"), Q.lit("0.25")
            (y * (Q.lit("3.0") - x * y * y) * Q.lit("0.5")).store()

    def test_store_of_a_product_checks_range_after_rounding(self):
        self.assertEqual((Q.lit("92233720368.0") * Q.lit("1.0")).store(), 92233720368 * S)
        with self.assertRaises(Trap):
            (Q.lit("92233720368.54775807") * Q.lit("1.00000001")).store()


def q_rsqrt(x):
    """The Faber text of rsqrt, expression by expression, through Q (every assignment is a store)."""
    four, quarter, half, two, three = (Q.lit(v) for v in ("4.0", "0.25", "0.5", "2.0", "3.0"))
    x, k = x, 0
    while x >= D("1.0"):
        x, k = (Q(x) * quarter).store(), k + 1
    while x < D("0.25"):
        x, k = (Q(x) * four).store(), k - 1
    y = (Q.lit("2.33333333") - Q.lit("1.33333333") * Q(x)).store()
    for _ in range(3):
        h = (Q(x) * Q(y) * Q(y)).store()
        y = (Q(y) * (three - Q(h)) * half).store()
    for _ in range(k):
        y = (Q(y) * half).store()
    for _ in range(-k):
        y = (Q(y) * two).store()
    return y


class Kernels(unittest.TestCase):
    def test_rsqrt_matches_the_expression_semantics_and_is_accurate(self):
        xs = [D(t) for t in ("0.00000001", "0.0001", "0.25", "0.5", "0.99999999", "1.0", "2.0", "3.14159265",
                             "100.0", "4096.0", "65536.5", "1000000.0")] + [random.Random(3).randrange(1, 10**14) for _ in range(300)]
        worst = 0.0
        for x in xs:
            y = swarm.rsqrt(x)
            self.assertEqual(y, q_rsqrt(x))
            worst = max(worst, abs(y / S * sqrt(x / S) - 1.0))
        self.assertLess(worst, 3e-5)

    def test_aim_has_the_requested_speed_and_direction(self):
        for d in ((3, 4, 0), (-10, 0, 5), (1, 1, 1), (0, 0, 7)):
            v = swarm.aim(*(c * S for c in d), D("4.0"))
            speed = sqrt(sum((c / S) ** 2 for c in v))
            self.assertAlmostEqual(speed, 4.0, delta=1e-3)
            cross = [v[1] * d[2] - v[2] * d[1], v[2] * d[0] - v[0] * d[2], v[0] * d[1] - v[1] * d[0]]
            self.assertTrue(all(abs(c) / S < 1e-3 * sqrt(sum(x * x for x in d)) for c in cross))   # v parallel to d
        self.assertEqual(swarm.aim(0, 0, 0, D("4.0")), (0, 0, 0))

    def test_cap_speed(self):
        self.assertEqual(swarm.cap_speed(D("3.0"), D("4.0"), 0, D("6.5"), swarm.PREY_VMAX2), (D("3.0"), D("4.0"), 0))
        vx, vy, vz = swarm.cap_speed(D("30.0"), D("40.0"), 0, D("6.5"), swarm.PREY_VMAX2)
        self.assertAlmostEqual(sqrt((vx / S) ** 2 + (vy / S) ** 2), 6.5, delta=2e-3)

    def test_walls_reflect_exactly(self):
        self.assertEqual(swarm.wall(D("-0.5"), D("-2.0"), swarm.WX), (D("0.5"), D("2.0")))
        self.assertEqual(swarm.wall(D("200.5"), D("2.0"), swarm.WX), (D("199.5"), D("-2.0")))
        self.assertEqual(swarm.wall(D("10.0"), D("2.0"), swarm.WX), (D("10.0"), D("2.0")))


class Rng(unittest.TestCase):
    def test_splitmix64_reference_vector(self):
        self.assertEqual(swarm.mix(0), 0xE220A8397B1DCDAF)      # first SplitMix64 output for seed 0
        self.assertEqual(swarm.mix(1234567), 0x599ED017FB08FC85)

    def test_counter_based_and_in_range(self):
        self.assertEqual(swarm.rnd(5, 17, 2), swarm.rnd(5, 17, 2))
        self.assertEqual(len({swarm.rnd(t, i, p) for t in range(4) for i in range(4) for p in range(4)}), 64)
        us = [swarm.unit(swarm.rnd(1, i, 1)) for i in range(5000)]
        self.assertTrue(all(-S <= u <= S for u in us))
        self.assertLess(abs(sum(us) / len(us)) / S, 0.03)


class Structures(unittest.TestCase):
    def test_grid_is_a_counting_sort_by_cell_then_id(self):
        w = swarm.World()
        g = swarm.Grid(w.prey)
        self.assertEqual(g.start[0], 0)
        self.assertEqual(g.start[-1], w.prey.n)
        self.assertEqual(sorted(g.items), list(range(w.prey.n)))
        for c in range(swarm.NCELL):
            items = g.items[g.start[c]:g.start[c + 1]]
            self.assertEqual(items, sorted(items))
            self.assertTrue(all(swarm.Grid.cell(w.prey.px[i], w.prey.py[i], w.prey.pz[i]) == c for i in items))

    def test_near_finds_everything_in_range(self):
        w = swarm.World()
        g, P = swarm.Grid(w.prey), w.prey
        for i in range(0, 800, 37):
            x, y, z = P.px[i], P.py[i], P.pz[i]
            got = set(g.near(x, y, z, swarm.R_FLOCK))
            want = {j for j in range(P.n) if (x - P.px[j]) ** 2 + (y - P.py[j]) ** 2 + (z - P.pz[j]) ** 2 <= swarm.R_FLOCK2}
            self.assertTrue(want <= got)

    def test_compaction_keeps_order(self):
        p = swarm.Pop()
        for i in range(10):
            p.add(i * 3, i, i, i, 0, 0, 0, i, 0)
        out = p.compact([1, 0, 1, 1, 0, 0, 1, 0, 0, 1])
        self.assertEqual(out.ids, [0, 6, 9, 18, 27])
        self.assertEqual(out.px, [0, 2, 3, 6, 9])

    def test_invariants_hold_each_tick(self):
        lim = 1 + 1e-4
        for t, w in swarm.simulate(100):
            for pop, vmax in ((w.prey, swarm.PREY_VMAX), (w.pred, swarm.PRED_VMAX)):
                self.assertEqual(pop.ids, sorted(set(pop.ids)))
                for i in range(pop.n):
                    self.assertTrue(0 <= pop.px[i] <= swarm.WX and 0 <= pop.py[i] <= swarm.WY and 0 <= pop.pz[i] <= swarm.WZ)
                    self.assertLessEqual(sqrt(pop.vx[i] ** 2 + pop.vy[i] ** 2 + pop.vz[i] ** 2), vmax * lim)
            self.assertTrue(all(0 <= a <= swarm.FOOD_CAP for a in w.food.en))
            self.assertLessEqual(w.prey.n, swarm.PREY_MAX)
            ids = w.prey.ids + w.pred.ids + w.food.ids
            self.assertEqual(len(ids), len(set(ids)))


class Determinism(unittest.TestCase):
    def test_two_runs_are_identical(self):
        self.assertEqual(hashes_of(100), hashes_of(100))
        self.assertEqual(hashes_of(100), full_run()[0][:101])

    def test_phase_2_order_independence(self):
        # phase 2 visits prey and predators in a shuffled order each tick: nothing may change
        want = full_run()[0][:101]
        self.assertEqual(hashes_of(100, shuffle_seed=1), want)
        self.assertEqual(hashes_of(60, shuffle_seed=99), want[:61])


class Ecosystem(unittest.TestCase):
    def test_populations_stay_above_zero_for_500_ticks(self):
        pops = full_run()[3]
        self.assertEqual(len(pops), swarm.TICKS + 1)
        self.assertGreater(min(p[0] for p in pops), 0)
        self.assertGreater(min(p[1] for p in pops), 0)
        self.assertEqual({p[2] for p in pops}, {swarm.NFOOD})
        self.assertGreater(pops[-1][0], 100)
        self.assertGreater(pops[-1][1], 100)

    def test_every_state_is_visited(self):
        rows = [r.split(",") for r in full_run()[1][1:]]
        for col in range(4, 12):
            self.assertGreater(sum(int(r[col]) for r in rows), 0, swarm.SUMMARY_HEADER.split(",")[col])


class Golden(unittest.TestCase):
    def test_hashes_reproduce(self):
        hashes = full_run()[0]
        self.assertEqual(len(hashes), swarm.TICKS + 1)
        self.assertEqual("\n".join(hashes) + "\n", golden("hashes.txt"))
        self.assertEqual(hashlib.sha256(golden("hashes.txt").encode()).hexdigest(), GOLDEN_HASHES_SHA256)

    def test_summary_and_snapshots_reproduce(self):
        _, summary, snaps, _ = full_run()
        self.assertEqual("\n".join(summary) + "\n", golden("summary.csv"))
        for t, text in snaps.items():
            self.assertEqual(text, golden("snapshot_%d.csv" % t))

    def test_snapshot_display_form(self):
        for line in golden("snapshot_100.csv").splitlines()[1:]:
            for cell in line.split(",")[3:]:
                self.assertNotRegex(cell, r"\.\d*0$")       # no 12.50, no 12.0
                self.assertNotRegex(cell, r"\.$")


if __name__ == "__main__":
    unittest.main()
