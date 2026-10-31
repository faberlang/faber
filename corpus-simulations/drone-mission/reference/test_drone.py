"""Unit tests for the scalar reference. Run from this directory: python3 -m unittest test_drone"""
import hashlib
import os
import unittest
from math import sqrt

import drone
import scenarios as S

HERE = os.path.dirname(os.path.abspath(__file__))
QUAD, OCTO, M = S.AIRCRAFT["quad"], S.AIRCRAFT["octo"], S.MISSION
GOLDEN_TRACE_SHA256 = "e0c11425244a4579a9cb1ebbe3f60be9f937d82f3b5b71b7cd3bf7090fbcedf3"


class Physics(unittest.TestCase):
    def test_isa_density_sea_level(self):
        self.assertAlmostEqual(drone.air_density(0.0, 15.0, 0.0), 1.225, delta=2e-4)

    def test_density_falls_with_altitude_and_heat(self):
        self.assertLess(drone.air_density(1000.0, 15.0, 0.0), drone.air_density(0.0, 15.0, 0.0))
        self.assertLess(drone.air_density(0.0, 35.0, 0.0), drone.air_density(0.0, 15.0, 0.0))

    def test_hover_power_hand_value(self):
        # T = 10 N, rho = 1.225, A = 0.1 m^2, FM = 0.7: 10 * sqrt(10 / 0.245) / 0.7
        hand = 10.0 * 6.388765649999399 / 0.7        # sqrt(10/0.245) = 6.3887656...
        self.assertAlmostEqual(drone.hover_power(10.0, 1.225, 0.1, 0.7), hand, places=9)
        self.assertAlmostEqual(drone.hover_power(10.0, 1.225, 0.1, 0.7), 10.0 ** 1.5 / sqrt(2 * 1.225 * 0.1) / 0.7, places=9)

    def test_glauert_hover_is_momentum_value_and_forward_flight_lowers_it(self):
        vh = 5.0
        self.assertAlmostEqual(drone.induced_velocity(vh, 0.0, 0.0), vh, places=9)
        self.assertLess(drone.induced_velocity(vh, 15.0, 0.0), 0.5 * vh)
        # fixed point: v * sqrt(Va^2 + (Vc + v)^2) == vh^2
        v = drone.induced_velocity(vh, 8.0, 1.0)
        self.assertAlmostEqual(v * sqrt(64.0 + (1.0 + v) ** 2), vh * vh, places=11)

    def test_ocv_curve(self):
        self.assertEqual(drone.ocv(1.0), 4.2)
        self.assertEqual(drone.ocv(0.0), 3.3)
        self.assertAlmostEqual(drone.ocv(0.35), 0.5 * (3.72 + 3.85), places=12)
        prev = 0.0
        for i in range(101):
            v = drone.ocv(i / 100.0)
            self.assertGreaterEqual(v, prev)
            prev = v


class Mission(unittest.TestCase):
    def test_soc_falls_monotonically_and_conserves_charge(self):
        rows = []
        st = drone.fly(QUAD, M, 2.0, 1500.0, None, rows)
        soc = [float(r.split(",")[9]) for r in rows]
        self.assertTrue(all(b < a for a, b in zip([1.0] + soc, soc)))
        self.assertEqual(soc[-1], st["final_soc"])
        # coulomb counting: integrated current equals the charge drawn
        d = drone.derived(QUAD)
        t_prev, charge = 0.0, 0.0
        for r in rows:
            f = r.split(",")
            charge += float(f[7]) * (float(f[0]) - t_prev)
            t_prev = float(f[0])
        self.assertAlmostEqual(charge / (3600.0 * d["cap_ah"]), 1.0 - st["final_soc"], places=9)

    def test_mass_drops_by_payload_after_drop(self):
        rows = [r.split(",") for r in self._rows()]
        before = [float(r[4]) for r in rows if r[1] in ("cruise", "drop_payload")]
        after = [float(r[4]) for r in rows if r[1].startswith("return")]
        self.assertAlmostEqual(before[0] - after[0], 2.0, places=12)

    def _rows(self):
        rows = []
        drone.fly(QUAD, M, 2.0, 1500.0, None, rows)
        return rows

    def test_phase_order(self):
        names = []
        for r in self._rows():
            n = r.split(",")[1]
            if not names or names[-1] != n:
                names.append(n)
        self.assertEqual(names, [p[0] for p in drone.phase_table(M, 1500.0)])

    def test_motor_out_lowers_margin(self):
        nom = drone.fly(QUAD, M, 2.0, 1500.0, None)
        out = drone.fly(QUAD, M, 2.0, 1500.0, S.FAIL_T_S)
        self.assertEqual(nom["min_margin_fail"], float("inf"))
        self.assertLess(out["min_margin_fail"], nom["min_margin"])

    def test_strong_headwind_costs_charge(self):
        calm, gale = dict(M, headwind_mps=0.0), dict(M, headwind_mps=12.0)
        self.assertGreater(drone.fly(QUAD, calm, 2.0, 2000.0)["final_soc"], drone.fly(QUAD, gale, 2.0, 2000.0)["final_soc"])

    def test_error_paths(self):
        with self.assertRaises(drone.MissionError):   # far too far: pack runs empty
            drone.fly(QUAD, M, 2.0, 60000.0)
        weak = dict(QUAD, r_cell_ohm=1.0)             # absurd internal resistance: pack collapses
        with self.assertRaises(drone.MissionError):
            drone.fly(weak, M, 2.0, 1000.0)

    def test_rules_name_their_reason(self):
        res = drone.evaluate(QUAD, M, 12.0, 1000.0)
        bad = [(n, why) for n, ok, why in res if not ok]
        self.assertTrue(bad)
        self.assertTrue(all(why for _, why in bad))
        self.assertTrue(all(ok for _, ok, _ in drone.evaluate(QUAD, M, 1.0, 1000.0)))


class Searches(unittest.TestCase):
    def test_max_payload_is_the_boundary(self):
        for ac in (QUAD, OCTO):
            for dist in (1000.0, 3000.0):
                best = drone.max_payload(ac, M, dist)
                self.assertTrue(drone.passes(ac, M, best, dist))
                self.assertFalse(drone.passes(ac, M, best + S.PAYLOAD_RES_KG, dist))

    def test_max_range_is_the_boundary(self):
        for ac in (QUAD, OCTO):
            best = drone.max_range(ac, M, 2.0)
            self.assertTrue(drone.passes(ac, M, 2.0, best))
            self.assertFalse(drone.passes(ac, M, 2.0, best + S.RANGE_RES_M))

    def test_payload_search_none_when_zero_fails(self):
        self.assertIsNone(drone.max_payload(QUAD, M, 60000.0))


class Golden(unittest.TestCase):
    def test_trace_matches_golden_file_and_hash(self):
        text = drone.trace_csv()
        with open(os.path.join(HERE, "golden", "trace.csv")) as f:
            self.assertEqual(text, f.read())
        self.assertEqual(hashlib.sha256(text.encode()).hexdigest(), GOLDEN_TRACE_SHA256)

    def test_summary_matches_golden_file(self):
        with open(os.path.join(HERE, "golden", "summary.csv")) as f:
            self.assertEqual(drone.summary_csv(), f.read())


if __name__ == "__main__":
    unittest.main()
