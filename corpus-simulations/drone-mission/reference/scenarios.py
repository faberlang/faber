"""Invented aircraft, mission and rule data for the drone-mission reference.

Flat constants only (floats, ints, strings, lists of tuples), so the Faber port
can embed the same values verbatim. Nothing here is taken from any real design.
SI units throughout.
"""

# One cell open-circuit-voltage curve (LiPo-like): (state of charge, volts), ascending.
OCV_TABLE = [
    (0.00, 3.30), (0.05, 3.55), (0.10, 3.65), (0.20, 3.72),
    (0.50, 3.85), (0.80, 4.00), (0.95, 4.12), (1.00, 4.20),
]
V_NOMINAL_CELL = 3.7  # reference cell voltage for max-thrust scaling

AIRCRAFT = {
    "quad": {
        "n_rotors": 4, "lost_on_motor_out": 2,   # one motor out also idles its opposite
        "rotor_radius_m": 0.35, "fm": 0.70,      # figure of merit
        "k_prof": 1.2e-4, "v_tip_mps": 110.0,    # profile power = k_prof * rho * A * vtip^3 * (1 + 4.65 mu^2)
        "tmax_ref_n": 80.0,                      # per-rotor max thrust at nominal pack voltage, rho = 1.225
        "cda_m2": 0.05, "eta": 0.85, "p_avionics_w": 45.0,
        "airframe_kg": 6.2, "pack_kg": 1.0,
        "ns": 12, "np": 1, "cell_ah": 3.2, "r_cell_ohm": 0.005, "c_cont": 15.0,
    },
    "octo": {
        "n_rotors": 8, "lost_on_motor_out": 2,
        "rotor_radius_m": 0.28, "fm": 0.68,
        "k_prof": 1.3e-4, "v_tip_mps": 120.0,
        "tmax_ref_n": 42.0,
        "cda_m2": 0.08, "eta": 0.85, "p_avionics_w": 60.0,
        "airframe_kg": 8.5, "pack_kg": 1.9,
        "ns": 12, "np": 2, "cell_ah": 3.2, "r_cell_ohm": 0.005, "c_cont": 15.0,
    },
}
AIRCRAFT_ORDER = ["quad", "octo"]

MISSION = {   # the delivery mission
    "site_alt_m": 300.0, "temp_c": 20.0,
    "cruise_alt_m": 30.0, "land_alt_m": 3.0,
    "climb_rate_mps": 3.0, "descent_rate_mps": 2.0, "land_rate_mps": 0.5,
    "hover_s": 10.0, "drop_s": 10.0,
    "speed_out_mps": 12.0, "speed_ret_mps": 15.0, "headwind_mps": 4.0,
    "start_soc": 1.0, "dt_s": 1.0,
}

RULES = {"tw_takeoff": 2.0, "reserve_soc": 0.20, "margin_fail": 1.3}  # + per-aircraft c_cont
FAIL_T_S = 25.0            # motor-out time for the failure run (early cruise, heaviest)

PAYLOAD_CAP_KG = 16.0      # search ceilings; 16/0.5 and 6400/50 are powers of two, so bisection mids are exact
PAYLOAD_RES_KG = 0.5
RANGE_CAP_M = 6400.0
RANGE_RES_M = 50.0

# Fixed outputs
TRACE = {"aircraft": "quad", "payload_kg": 2.0, "distance_m": 2000.0, "fail_t_s": 25.0}
SUMMARY_DISTANCES_M = [1000.0, 2000.0, 3000.0]
SUMMARY_PAYLOADS_KG = [1.0, 2.0, 3.0]
