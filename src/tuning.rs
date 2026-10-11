//! The knob table and the cross-field constraint logic, transcribed from `API.md`
//! (itself the same content as `lib/ledfx/tuning.h` and `lib/ledfx/led_layout.h`).
//!
//! The rig validates a request against the *result*: one out-of-range value or one
//! violated constraint rejects the whole batch. This module lets the server reject
//! such a batch locally — no request, no partial state — and sequence a batch of more
//! than 16 edits so every intermediate request the firmware sees is itself consistent.

use std::collections::BTreeMap;

pub struct Knob {
    pub name: &'static str,
    pub group: &'static str,
    pub min: u32,
    pub max: u32,
    pub default: u32,
}

/// All 58 knobs, in `API.md` order. Ranges are inclusive; `max` is the firmware's own
/// ceiling (`kMaxPwm` = 4095, `kTuningMaxMs` = 3600000, `kTuningMaxStepMs` = 60000).
pub const KNOBS: &[Knob] = &[
    // --- pulse_* — mode 2 (Pulse) ---
    Knob { name: "pulse_half_slow_ms", group: "Pulse", min: 1, max: 3600000, default: 1000 },
    Knob { name: "pulse_half_fast_ms", group: "Pulse", min: 1, max: 3600000, default: 250 },
    Knob { name: "pulse_levels", group: "Pulse", min: 1, max: 4095, default: 64 },
    // --- smolder_* — mode 0 (Smolder) embers ---
    Knob { name: "smolder_dwell_ms", group: "Smolder embers", min: 0, max: 3600000, default: 7000 },
    Knob { name: "smolder_fade_ms", group: "Smolder embers", min: 1, max: 3600000, default: 7000 },
    Knob { name: "smolder_levels", group: "Smolder embers", min: 1, max: 4095, default: 64 },
    Knob { name: "smolder_flicker_pwm", group: "Smolder embers", min: 0, max: 4095, default: 1400 },
    Knob { name: "smolder_flicker_step_ms", group: "Smolder embers", min: 1, max: 60000, default: 47 },
    // --- sleep/wake machine — mode 0 ---
    Knob { name: "smolder_sleep_after_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 30000 },
    Knob { name: "smolder_sleep_deadline_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 240000 },
    Knob { name: "smolder_sleep_check_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 5000 },
    Knob { name: "smolder_sleep_min_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 30000 },
    Knob { name: "smolder_sleep_max_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 180000 },
    Knob { name: "sleep_fall_min_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 5000 },
    Knob { name: "sleep_fall_max_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 20000 },
    Knob { name: "sleep_open_min_gap_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 1000 },
    Knob { name: "sleep_open_max_gap_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 5000 },
    Knob { name: "sleep_open_group_permille", group: "Sleep / wake", min: 0, max: 1000, default: 350 },
    Knob { name: "sleep_gesture_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 900 },
    Knob { name: "sleep_dip_end_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 250 },
    Knob { name: "sleep_flare_end_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 550 },
    Knob { name: "sleep_dip_permille", group: "Sleep / wake", min: 0, max: 999, default: 400 },
    Knob { name: "sleep_flare_permille", group: "Sleep / wake", min: 0, max: 999, default: 750 },
    Knob { name: "sleep_wake_spread_ms", group: "Sleep / wake", min: 1, max: 3600000, default: 120 },
    // --- agitation_* / green_* — mode 0 green agitation ---
    Knob { name: "agitation_hold_ms", group: "Agitation (green)", min: 1, max: 3600000, default: 1500 },
    Knob { name: "agitation_exit_ms", group: "Agitation (green)", min: 1, max: 3600000, default: 1000 },
    Knob { name: "agitation_flicker_pwm", group: "Agitation (green)", min: 0, max: 4094, default: 1200 },
    Knob { name: "agitation_flicker_step_ms", group: "Agitation (green)", min: 1, max: 60000, default: 43 },
    Knob { name: "agitation_sweep_min_ms", group: "Agitation (green)", min: 1, max: 3600000, default: 250 },
    Knob { name: "agitation_sweep_max_ms", group: "Agitation (green)", min: 1, max: 3600000, default: 1000 },
    Knob { name: "agitation_sweep_pause_min_ms", group: "Agitation (green)", min: 1, max: 3600000, default: 500 },
    Knob { name: "agitation_sweep_pause_max_ms", group: "Agitation (green)", min: 1, max: 3600000, default: 2000 },
    Knob { name: "agitation_sweep_tail_permille", group: "Agitation (green)", min: 1, max: 7000, default: 3500 },
    Knob { name: "green_spread_ms", group: "Agitation (green)", min: 0, max: 3600000, default: 120 },
    Knob { name: "green_attack_ms", group: "Agitation (green)", min: 1, max: 60000, default: 40 },
    Knob { name: "green_fade_ms", group: "Agitation (green)", min: 1, max: 3600000, default: 800 },
    Knob { name: "green_flicker_pwm", group: "Agitation (green)", min: 0, max: 4095, default: 1200 },
    // --- stalker_* — mode 3 (Stalker) ---
    Knob { name: "stalker_step_ms", group: "Stalker", min: 1, max: 3600000, default: 900 },
    Knob { name: "stalker_dim_pwm", group: "Stalker", min: 0, max: 4095, default: 700 },
    Knob { name: "stalker_green_permille", group: "Stalker", min: 0, max: 1000, default: 620 },
    Knob { name: "stalker_flicker_pwm", group: "Stalker", min: 0, max: 4095, default: 600 },
    Knob { name: "stalker_flicker_step_ms", group: "Stalker", min: 1, max: 60000, default: 53 },
    Knob { name: "stalker_pop_ms", group: "Stalker", min: 1, max: 3600000, default: 220 },
    Knob { name: "stalker_release_ms", group: "Stalker", min: 1, max: 3600000, default: 900 },
    Knob { name: "stalker_pop_pwm", group: "Stalker", min: 0, max: 4095, default: 4095 },
    Knob { name: "stalker_pop_flicker_pwm", group: "Stalker", min: 0, max: 4095, default: 1000 },
    // --- toxic_* — mode 6 (Toxic) ---
    Knob { name: "toxic_green_base_pwm", group: "Toxic", min: 0, max: 4095, default: 700 },
    Knob { name: "toxic_green_flicker_pwm", group: "Toxic", min: 0, max: 4095, default: 1000 },
    Knob { name: "toxic_green_step_ms", group: "Toxic", min: 1, max: 60000, default: 61 },
    Knob { name: "toxic_spark_idle_pwm", group: "Toxic", min: 0, max: 4095, default: 1600 },
    Knob { name: "toxic_spark_head_pwm", group: "Toxic", min: 0, max: 4095, default: 4095 },
    Knob { name: "toxic_spark_flicker_pwm", group: "Toxic", min: 0, max: 4095, default: 700 },
    Knob { name: "toxic_spark_frantic_pwm", group: "Toxic", min: 0, max: 4095, default: 1400 },
    Knob { name: "toxic_spark_idle_step_ms", group: "Toxic", min: 1, max: 3600000, default: 420 },
    Knob { name: "toxic_spark_fast_step_ms", group: "Toxic", min: 1, max: 3600000, default: 130 },
    Knob { name: "toxic_spark_tail_permille", group: "Toxic", min: 1, max: 1000, default: 900 },
    Knob { name: "toxic_motion_ramp_ms", group: "Toxic", min: 1, max: 3600000, default: 600 },
    Knob { name: "toxic_flicker_step_ms", group: "Toxic", min: 1, max: 60000, default: 43 },
];

pub fn knob(name: &str) -> Option<&'static Knob> {
    KNOBS.iter().find(|k| k.name == name)
}

/// The knob's unit, for the form's hint text.
pub fn unit_of(name: &str) -> &'static str {
    if name.ends_with("_ms") {
        "ms"
    } else if name.ends_with("_pwm") {
        "PWM"
    } else if name.ends_with("_permille") {
        "‰"
    } else if name.ends_with("_levels") {
        "levels"
    } else {
        ""
    }
}

/// The knobs grouped the way `API.md` presents them, in table order.
pub fn groups() -> Vec<(&'static str, Vec<&'static Knob>)> {
    let mut out: Vec<(&'static str, Vec<&'static Knob>)> = Vec::new();
    for k in KNOBS {
        match out.iter_mut().find(|(name, _)| *name == k.group) {
            Some((_, knobs)) => knobs.push(k),
            None => out.push((k.group, vec![k])),
        }
    }
    out
}

/// Every knob at its built-in default, as the firmware would report it after
/// `POST /config/reset`.
pub fn defaults() -> BTreeMap<String, u32> {
    KNOBS
        .iter()
        .map(|k| (k.name.to_string(), k.default))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    pub value: u32,
}

/// First change whose value is outside its knob's range (or names no knob at all).
pub fn check_ranges(changes: &[Change]) -> Result<(), String> {
    for change in changes {
        match knob(&change.name) {
            Some(k) if change.value >= k.min && change.value <= k.max => {}
            _ => return Err(change.name.clone()),
        }
    }
    Ok(())
}

/// A "must hold" pair from `API.md`: `state[lo] < state[hi]`, with the field the
/// firmware names in its `400` when it does not.
pub struct Constraint {
    pub lo: &'static str,
    pub hi: &'static str,
    pub reported: &'static str,
}

/// The ten cross-field constraints, normalised to `lo < hi` (`toxic_spark_*` is
/// inverted on the wire: idle > fast, i.e. fast < idle).
pub const CONSTRAINTS: &[Constraint] = &[
    Constraint { lo: "smolder_sleep_after_ms", hi: "smolder_sleep_deadline_ms", reported: "smolder_sleep_after_ms" },
    Constraint { lo: "smolder_sleep_min_ms", hi: "smolder_sleep_max_ms", reported: "smolder_sleep_min_ms" },
    Constraint { lo: "sleep_fall_min_ms", hi: "sleep_fall_max_ms", reported: "sleep_fall_min_ms" },
    Constraint { lo: "sleep_open_min_gap_ms", hi: "sleep_open_max_gap_ms", reported: "sleep_open_min_gap_ms" },
    Constraint { lo: "sleep_dip_end_ms", hi: "sleep_flare_end_ms", reported: "sleep_dip_end_ms" },
    Constraint { lo: "sleep_flare_end_ms", hi: "sleep_gesture_ms", reported: "sleep_flare_end_ms" },
    Constraint { lo: "sleep_dip_permille", hi: "sleep_flare_permille", reported: "sleep_dip_permille" },
    Constraint { lo: "agitation_sweep_min_ms", hi: "agitation_sweep_max_ms", reported: "agitation_sweep_min_ms" },
    Constraint { lo: "agitation_sweep_pause_min_ms", hi: "agitation_sweep_pause_max_ms", reported: "agitation_sweep_pause_min_ms" },
    // Inverted on the wire (idle > fast); normalised here as fast < idle.
    Constraint { lo: "toxic_spark_fast_step_ms", hi: "toxic_spark_idle_step_ms", reported: "toxic_spark_idle_step_ms" },
];

/// First violated constraint in the resulting state, named as the firmware names it.
/// A constraint whose fields are not both present is skipped.
pub fn check_constraints(state: &BTreeMap<String, u32>) -> Result<(), String> {
    for c in CONSTRAINTS {
        if let (Some(lo), Some(hi)) = (state.get(c.lo), state.get(c.hi)) {
            if lo >= hi {
                return Err(c.reported.to_string());
            }
        }
    }
    Ok(())
}

/// Order `changes` so that every intermediate state satisfies `check_constraints`,
/// which is what the firmware requires across a batch split into 16-edit requests.
///
/// Greedy widen-first: repeatedly apply a pending change that keeps the simulated
/// state consistent. For any constrained pair with both ends changing, at least one
/// end is safe to apply on its own (either the new `lo` is below the old `hi`, or the
/// new `hi` is above the new `lo`), so a candidate exists whenever the target state is
/// valid. If none is found the caller must send nothing.
pub fn order_changes(
    current: &BTreeMap<String, u32>,
    changes: &[Change],
) -> anyhow::Result<Vec<Change>> {
    let mut sim = current.clone();
    let mut pending: Vec<Change> = changes.to_vec();
    let mut ordered: Vec<Change> = Vec::with_capacity(pending.len());

    while !pending.is_empty() {
        let mut chosen = None;
        for (i, change) in pending.iter().enumerate() {
            let previous = sim.insert(change.name.clone(), change.value);
            let consistent = check_constraints(&sim).is_ok();
            match previous {
                Some(value) => {
                    sim.insert(change.name.clone(), value);
                }
                None => {
                    sim.remove(&change.name);
                }
            }
            if consistent {
                chosen = Some(i);
                break;
            }
        }
        match chosen {
            Some(i) => {
                let change = pending.remove(i);
                sim.insert(change.name.clone(), change.value);
                ordered.push(change);
            }
            None => anyhow::bail!("no consistent ordering for the requested changes"),
        }
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(pairs: &[(&str, u32)]) -> BTreeMap<String, u32> {
        let mut state = defaults();
        for (name, value) in pairs {
            state.insert(name.to_string(), *value);
        }
        state
    }

    fn change(name: &str, value: u32) -> Change {
        Change {
            name: name.to_string(),
            value,
        }
    }

    #[test]
    fn table_matches_the_api_document() {
        assert_eq!(KNOBS.len(), 58);
        assert_eq!(CONSTRAINTS.len(), 10);
        for k in KNOBS {
            assert!(k.max >= k.min, "{}: max < min", k.name);
            assert!(
                k.default >= k.min && k.default <= k.max,
                "{}: default {} outside {}..={}",
                k.name,
                k.default,
                k.min,
                k.max
            );
        }
        let mut names: Vec<&str> = KNOBS.iter().map(|k| k.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KNOBS.len(), "duplicate knob name");

        let counts: Vec<(&str, usize)> = groups().iter().map(|(g, k)| (*g, k.len())).collect();
        assert_eq!(
            counts,
            vec![
                ("Pulse", 3),
                ("Smolder embers", 5),
                ("Sleep / wake", 16),
                ("Agitation (green)", 13),
                ("Stalker", 9),
                ("Toxic", 12),
            ]
        );

        for c in CONSTRAINTS {
            assert!(knob(c.lo).is_some(), "constraint field {} unknown", c.lo);
            assert!(knob(c.hi).is_some(), "constraint field {} unknown", c.hi);
            assert!(knob(c.reported).is_some(), "reported field {} unknown", c.reported);
        }
        // The defaults the firmware ships with must satisfy every constraint.
        assert!(check_constraints(&defaults()).is_ok());
    }

    #[test]
    fn units_follow_the_name_suffix() {
        assert_eq!(unit_of("smolder_dwell_ms"), "ms");
        assert_eq!(unit_of("stalker_pop_pwm"), "PWM");
        assert_eq!(unit_of("sleep_dip_permille"), "‰");
        assert_eq!(unit_of("pulse_levels"), "levels");
        assert_eq!(unit_of("nonsense"), "");
    }

    #[test]
    fn range_checks_catch_both_ends() {
        assert!(check_ranges(&[change("smolder_levels", 64)]).is_ok());
        assert_eq!(
            check_ranges(&[change("smolder_levels", 99999)]),
            Err("smolder_levels".to_string())
        );
        assert_eq!(
            check_ranges(&[change("pulse_half_slow_ms", 0)]),
            Err("pulse_half_slow_ms".to_string())
        );
        assert_eq!(
            check_ranges(&[change("not_a_knob", 1)]),
            Err("not_a_knob".to_string())
        );
    }

    #[test]
    fn constraint_check_names_the_reported_field() {
        let state = state_with(&[("sleep_dip_permille", 900), ("sleep_flare_permille", 500)]);
        assert_eq!(
            check_constraints(&state),
            Err("sleep_dip_permille".to_string())
        );
        let state = state_with(&[
            ("toxic_spark_idle_step_ms", 130),
            ("toxic_spark_fast_step_ms", 420),
        ]);
        assert_eq!(
            check_constraints(&state),
            Err("toxic_spark_idle_step_ms".to_string())
        );
        // Equal values are rejected too (the constraint is strict).
        let state = state_with(&[("sleep_gesture_ms", 550)]);
        assert_eq!(check_constraints(&state), Err("sleep_flare_end_ms".to_string()));
    }

    #[test]
    fn api_doc_example_orders_the_widening_field_first() {
        // API.md's worked example: dip 400 -> 800 while flare 750 -> 900.
        let current = defaults();
        let changes = vec![
            change("sleep_dip_permille", 800),
            change("sleep_flare_permille", 900),
        ];
        let ordered = order_changes(&current, &changes).unwrap();
        assert_eq!(ordered[0].name, "sleep_flare_permille");
        assert_eq!(ordered[1].name, "sleep_dip_permille");
    }

    #[test]
    fn every_constraint_pair_can_be_raised_in_a_consistent_order() {
        for c in CONSTRAINTS {
            let current = defaults();
            let lo_value = current[c.lo] + 1;
            let hi_value = current[c.hi] + 1;
            let changes = vec![change(c.lo, lo_value), change(c.hi, hi_value)];
            assert!(check_constraints(&state_with(&[(c.lo, lo_value), (c.hi, hi_value)])).is_ok());

            let ordered = order_changes(&current, &changes).unwrap();
            assert_eq!(ordered.len(), 2);

            // Every prefix the firmware would see must already be consistent.
            let mut sim = current.clone();
            for step in &ordered {
                sim.insert(step.name.clone(), step.value);
                assert!(
                    check_constraints(&sim).is_ok(),
                    "constraint {} violated after applying {}",
                    c.reported,
                    step.name
                );
            }
            assert_eq!(sim[c.lo], lo_value);
            assert_eq!(sim[c.hi], hi_value);
        }
    }

    #[test]
    fn chained_constraints_order_without_deadlock() {
        // sleep_dip_end_ms < sleep_flare_end_ms < sleep_gesture_ms, all raised.
        let current = defaults();
        let changes = vec![
            change("sleep_dip_end_ms", 800),
            change("sleep_flare_end_ms", 850),
            change("sleep_gesture_ms", 880),
        ];
        let ordered = order_changes(&current, &changes).unwrap();
        let mut sim = current.clone();
        for step in &ordered {
            sim.insert(step.name.clone(), step.value);
            assert!(check_constraints(&sim).is_ok());
        }
        assert_eq!(ordered.len(), 3);
        assert_eq!(sim["sleep_gesture_ms"], 880);
    }

    #[test]
    fn impossible_target_reports_an_error_instead_of_ordering() {
        let current = defaults();
        // 900 is in range for dip_permille, but cannot coexist with flare at 750.
        let changes = vec![change("sleep_dip_permille", 900)];
        assert!(order_changes(&current, &changes).is_err());
    }
}
