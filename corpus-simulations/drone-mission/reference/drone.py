"""Scalar reference: a time-stepped multirotor delivery-mission model.

Python standard library only. float64 throughout. Only + - * / and sqrt are used
(no pow/exp/log/trig), so every result is correctly rounded and a port can match
it bit for bit. The operation order written here IS the contract (see README).
Run:  python3 drone.py trace|summary|rules   (from this directory)
"""
import sys
from math import sqrt

import scenarios as S

PI = 3.141592653589793
G = 9.80665
P0 = 101325.0
T0 = 288.15
LAPSE = 0.0065
R_AIR = 287.05
RHO0 = 1.225
GLAUERT_ITERS = 48
MAX_STEPS = 20000
CSV_HEAD = "t_s,phase,alt_m,speed_mps,mass_kg,thrust_n,power_w,current_a,vterm_v,soc,tw_margin"


class MissionError(Exception):
    """Mission cannot continue; the message is the reason."""


def air_density(h_abs, temp_c, site_alt):
    """ISA pressure (exponent 21/4 as x^5 * sqrt(sqrt(x))), lapse-rate temperature
    anchored to the site temperature, ideal gas."""
    x = 1.0 - (LAPSE * h_abs) / T0
    x2 = x * x
    x4 = x2 * x2
    p = P0 * (x4 * x * sqrt(sqrt(x)))
    t = (temp_c + 273.15) - LAPSE * (h_abs - site_alt)
    return p / (R_AIR * t)


def ocv(soc):
    """Per-cell open-circuit voltage: piecewise linear in SoC, first segment with soc <= s1."""
    tab = S.OCV_TABLE
    if soc < 0.0:
        soc = 0.0
    if soc > 1.0:
        soc = 1.0
    for i in range(len(tab) - 1):
        s0, v0 = tab[i]
        s1, v1 = tab[i + 1]
        if soc <= s1:
            return v0 + (v1 - v0) * ((soc - s0) / (s1 - s0))
    return tab[-1][1]


def hover_power(thrust, rho, area, fm):
    """Momentum-theory hover power of one rotor: T^1.5 / sqrt(2 rho A) / FM."""
    return thrust * sqrt(thrust / ((2.0 * rho) * area)) / fm


def induced_velocity(vh, v_air, vc):
    """Glauert fixed point v = vh^2 / sqrt(Va^2 + (Vc + v)^2): fixed count, damped 0.5, start at vh."""
    vi = vh
    for _ in range(GLAUERT_ITERS):
        f = (vh * vh) / sqrt(v_air * v_air + (vc + vi) * (vc + vi))
        vi = 0.5 * (vi + f)
    return vi


def derived(ac):
    """Per-aircraft constants, computed once in this order."""
    d = dict(ac)
    d["area"] = PI * ac["rotor_radius_m"] * ac["rotor_radius_m"]
    d["base_kg"] = ac["airframe_kg"] + ac["pack_kg"]
    d["r_pack"] = (ac["ns"] * ac["r_cell_ohm"]) / ac["np"]
    d["cap_ah"] = ac["np"] * ac["cell_ah"]
    d["v_ref"] = ac["ns"] * S.V_NOMINAL_CELL
    d["i_limit"] = ac["c_cont"] * d["cap_ah"]
    return d


def phase_table(m, dist):
    """(name, carry, ground, quantity, rate, v_ground, v_climb, headwind); quantity is in
    rate units (m for climb/cruise/descend/land, s for hover/drop)."""
    h, la = m["cruise_alt_m"], m["land_alt_m"]
    cr, dr, lr = m["climb_rate_mps"], m["descent_rate_mps"], m["land_rate_mps"]
    so, sr, w = m["speed_out_mps"], m["speed_ret_mps"], m["headwind_mps"]
    return [
        ("takeoff_climb", 1.0, False, h, cr, 0.0, cr, 0.0),
        ("hover", 1.0, False, m["hover_s"], 1.0, 0.0, 0.0, 0.0),
        ("cruise", 1.0, False, dist, so, so, 0.0, w),
        ("descend", 1.0, False, h - la, dr, 0.0, -dr, 0.0),
        ("land", 1.0, False, la, lr, 0.0, -lr, 0.0),
        ("drop_payload", 1.0, True, m["drop_s"], 1.0, 0.0, 0.0, 0.0),
        ("return_climb", 0.0, False, h, cr, 0.0, cr, 0.0),
        ("return", 0.0, False, dist, sr, sr, 0.0, 0.0 - w),
        ("return_descend", 0.0, False, h - la, dr, 0.0, -dr, 0.0),
        ("return_land", 0.0, False, la, lr, 0.0, -lr, 0.0),
    ]


def fly(ac, m, payload, dist, fail_t=None, trace=None):
    """Fly one mission. fail_t: motor-out time (float) or None. trace: list to append CSV rows to.
    Returns stats; raises MissionError on pack collapse, empty pack, vortex ring or step limit."""
    d = derived(ac)
    phases = phase_table(m, dist)
    soc, t, alt, p, dt_s = m["start_soc"], 0.0, 0.0, 0, m["dt_s"]
    q = phases[0][3]
    st = {"takeoff_margin": 0.0, "min_margin": float("inf"), "min_margin_fail": float("inf"),
          "max_current": 0.0, "final_soc": soc, "steps": 0}
    while p < len(phases):
        name, carry, ground, _, rate, vg, vc, wind = phases[p]
        st["steps"] += 1
        if st["steps"] > MAX_STEPS:
            raise MissionError("step_limit")
        final = q <= rate * dt_s
        dt = q / rate if final else dt_s
        mass = d["base_kg"] + payload * carry
        voc = d["ns"] * ocv(soc)
        failed = fail_t is not None and t >= fail_t
        h_abs = m["site_alt_m"] + alt
        rho = air_density(h_abs, m["temp_c"], m["site_alt_m"])
        thrust, margin = 0.0, 0.0
        p_elec = d["p_avionics_w"]
        if not ground:
            n_act = d["n_rotors"] - d["lost_on_motor_out"] if failed else d["n_rotors"]
            weight = mass * G
            v_air = vg + wind
            drag = (0.5 * rho * d["cda_m2"]) * (v_air * v_air)
            thrust = sqrt(weight * weight + drag * drag)
            t_r = thrust / n_act
            vh = sqrt(t_r / ((2.0 * rho) * d["area"]))
            vi = induced_velocity(vh, v_air, vc)
            if vc + vi <= 0.0:
                raise MissionError("vortex_ring")
            mu = v_air / d["v_tip_mps"]
            p_ind = n_act * ((t_r * (vc + vi)) / d["fm"])
            p_prof = n_act * (d["k_prof"] * rho * d["area"] * (d["v_tip_mps"] * d["v_tip_mps"] * d["v_tip_mps"])
                              * (1.0 + 4.65 * (mu * mu)))
            p_par = drag * v_air
            p_elec = (p_ind + p_prof + p_par) / d["eta"] + d["p_avionics_w"]
        disc = voc * voc - 4.0 * d["r_pack"] * p_elec
        if disc < 0.0:
            raise MissionError("pack_collapse")
        cur = (voc - sqrt(disc)) / (2.0 * d["r_pack"])
        vterm = voc - cur * d["r_pack"]
        if not ground:
            kv = vterm / d["v_ref"]
            tmax = d["tmax_ref_n"] * (kv * kv) * (rho / RHO0)
            margin = (n_act * tmax) / weight
            if st["steps"] == 1:
                st["takeoff_margin"] = margin
            if margin < st["min_margin"]:
                st["min_margin"] = margin
            if failed and margin < st["min_margin_fail"]:
                st["min_margin_fail"] = margin
        if cur > st["max_current"]:
            st["max_current"] = cur
        soc = soc - (cur * dt) / (3600.0 * d["cap_ah"])
        if soc <= 0.0:
            raise MissionError("pack_empty")
        t = t + dt
        alt = alt + vc * dt
        if final:
            p += 1
            if p < len(phases):
                q = phases[p][3]
        else:
            q = q - rate * dt
        if trace is not None:
            trace.append(",".join([repr(t), name, repr(alt), repr(vg), repr(mass), repr(thrust),
                                   repr(p_elec), repr(cur), repr(vterm), repr(soc), repr(margin)]))
    st["final_soc"] = soc
    st["duration_s"] = t
    return st


def evaluate(ac, m, payload, dist):
    """Run the rules. Nominal flight: takeoff T/W, continuous current, reserve SoC.
    Motor-out flight (S.FAIL_T_S): margin after failure. Returns [(rule, ok, reason)]."""
    r, d = S.RULES, derived(ac)
    out = []
    try:
        a = fly(ac, m, payload, dist, None)
    except MissionError as e:
        return [("mission", False, "nominal flight: " + str(e))]
    out.append(("tw_takeoff", a["takeoff_margin"] >= r["tw_takeoff"],
                "takeoff T/W " + repr(a["takeoff_margin"]) + " < " + repr(r["tw_takeoff"])))
    out.append(("current_cont", a["max_current"] <= d["i_limit"],
                "peak current " + repr(a["max_current"]) + " A > " + repr(d["i_limit"])))
    out.append(("reserve_soc", a["final_soc"] >= r["reserve_soc"],
                "landing SoC " + repr(a["final_soc"]) + " < " + repr(r["reserve_soc"])))
    try:
        f = fly(ac, m, payload, dist, S.FAIL_T_S)
    except MissionError as e:
        out.append(("margin_fail", False, "motor-out flight: " + str(e)))
        return out
    out.append(("margin_fail", f["min_margin_fail"] >= r["margin_fail"],
                "margin after failure " + repr(f["min_margin_fail"]) + " < " + repr(r["margin_fail"])))
    return out


def passes(ac, m, payload, dist):
    return all(ok for _, ok, _ in evaluate(ac, m, payload, dist))


def max_payload(ac, m, dist):
    """Largest payload (to S.PAYLOAD_RES_KG) passing every rule; None if even 0 kg fails."""
    lo, hi = 0.0, S.PAYLOAD_CAP_KG
    if not passes(ac, m, lo, dist):
        return None
    if passes(ac, m, hi, dist):
        return hi
    while hi - lo > S.PAYLOAD_RES_KG:
        mid = 0.5 * (lo + hi)
        if passes(ac, m, mid, dist):
            lo = mid
        else:
            hi = mid
    return lo


def max_range(ac, m, payload):
    """Largest one-way distance (to S.RANGE_RES_M) passing every rule; None if 0 m fails."""
    lo, hi = 0.0, S.RANGE_CAP_M
    if not passes(ac, m, payload, lo):
        return None
    if passes(ac, m, payload, hi):
        return hi
    while hi - lo > S.RANGE_RES_M:
        mid = 0.5 * (lo + hi)
        if passes(ac, m, payload, mid):
            lo = mid
        else:
            hi = mid
    return lo


def trace_csv():
    tr = S.TRACE
    rows = []
    fly(S.AIRCRAFT[tr["aircraft"]], S.MISSION, tr["payload_kg"], tr["distance_m"], tr["fail_t_s"], rows)
    return CSV_HEAD + "\n" + "\n".join(rows) + "\n"


def fmt(x):
    return "none" if x is None else repr(x)


def summary_csv():
    lines = ["search,aircraft,payload_kg,distance_m,result"]
    for name in S.AIRCRAFT_ORDER:
        ac = S.AIRCRAFT[name]
        for dist in S.SUMMARY_DISTANCES_M:
            lines.append(",".join(["max_payload", name, "", repr(dist), fmt(max_payload(ac, S.MISSION, dist))]))
        for pl in S.SUMMARY_PAYLOADS_KG:
            lines.append(",".join(["max_range", name, repr(pl), "", fmt(max_range(ac, S.MISSION, pl))]))
    return "\n".join(lines) + "\n"


def rules_text():
    tr = S.TRACE
    res = evaluate(S.AIRCRAFT[tr["aircraft"]], S.MISSION, tr["payload_kg"], tr["distance_m"])
    return "".join(n + " " + ("pass" if ok else "FAIL " + why) + "\n" for n, ok, why in res)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "summary"
    fns = {"trace": trace_csv, "summary": summary_csv, "rules": rules_text}
    sys.stdout.write(fns[cmd]())
