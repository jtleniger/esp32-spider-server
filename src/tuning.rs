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
    /// What the knob does, verbatim from the firmware's knob reference
    /// (`esp32-spider` `API.md`, same table as `lib/ledfx/led_layout.h`).
    pub desc: &'static str,
}

/// All 58 knobs, in `API.md` order. Ranges are inclusive; `max` is the firmware's own
/// ceiling (`kMaxPwm` = 4095, `kTuningMaxMs` = 3600000, `kTuningMaxStepMs` = 60000).
pub const KNOBS: &[Knob] = &[
    // --- pulse_* — mode 2 (Pulse) ---
    Knob { name: "pulse_half_slow_ms", group: "pulse", min: 1, max: 3600000, default: 1000, desc: "half cycle while radar idle (full cycle is 2×)" },
    Knob { name: "pulse_half_fast_ms", group: "pulse", min: 1, max: 3600000, default: 250, desc: "half cycle while radar sees motion" },
    Knob { name: "pulse_levels", group: "pulse", min: 1, max: 4095, default: 64, desc: "brightness rungs per half cycle" },
    // --- smolder_* — mode 0 (Smolder) embers ---
    Knob { name: "smolder_dwell_ms", group: "smolder embers", min: 0, max: 3600000, default: 7000, desc: "hold on one colour, bright" },
    Knob { name: "smolder_fade_ms", group: "smolder embers", min: 1, max: 3600000, default: 7000, desc: "eased crossfade to the other colour" },
    Knob { name: "smolder_levels", group: "smolder embers", min: 1, max: 4095, default: 64, desc: "rungs the crossfade quantises to" },
    Knob { name: "smolder_flicker_pwm", group: "smolder embers", min: 0, max: 4095, default: 1400, desc: "ember noise at full brightness (scales with level)" },
    Knob { name: "smolder_flicker_step_ms", group: "smolder embers", min: 1, max: 60000, default: 47, desc: "flicker refresh interval" },
    // --- sleep/wake machine — mode 0 ---
    Knob { name: "smolder_sleep_after_ms", group: "sleep / wake", min: 1, max: 3600000, default: 30000, desc: "earliest sleep onset while quiet" },
    Knob { name: "smolder_sleep_deadline_ms", group: "sleep / wake", min: 1, max: 3600000, default: 240000, desc: "forced onset by here" },
    Knob { name: "smolder_sleep_check_ms", group: "sleep / wake", min: 1, max: 3600000, default: 5000, desc: "hazard roll interval for nodding off" },
    Knob { name: "smolder_sleep_min_ms", group: "sleep / wake", min: 1, max: 3600000, default: 30000, desc: "shortest nap" },
    Knob { name: "smolder_sleep_max_ms", group: "sleep / wake", min: 1, max: 3600000, default: 180000, desc: "longest nap" },
    Knob { name: "sleep_fall_min_ms", group: "sleep / wake", min: 1, max: 3600000, default: 5000, desc: "earliest eye-closing gesture after sleep onset" },
    Knob { name: "sleep_fall_max_ms", group: "sleep / wake", min: 1, max: 3600000, default: 20000, desc: "latest eye-closing gesture after sleep onset" },
    Knob { name: "sleep_open_min_gap_ms", group: "sleep / wake", min: 1, max: 3600000, default: 1000, desc: "shortest gap between asleep half-opens" },
    Knob { name: "sleep_open_max_gap_ms", group: "sleep / wake", min: 1, max: 3600000, default: 5000, desc: "longest gap between asleep half-opens" },
    Knob { name: "sleep_open_group_permille", group: "sleep / wake", min: 0, max: 1000, default: 350, desc: "chance an eye joins a half-open event" },
    Knob { name: "sleep_gesture_ms", group: "sleep / wake", min: 1, max: 3600000, default: 900, desc: "duration of one eyelid gesture" },
    Knob { name: "sleep_dip_end_ms", group: "sleep / wake", min: 1, max: 3600000, default: 250, desc: "when the gesture reaches its dim point" },
    Knob { name: "sleep_flare_end_ms", group: "sleep / wake", min: 1, max: 3600000, default: 550, desc: "when the gesture reaches its flare" },
    Knob { name: "sleep_dip_permille", group: "sleep / wake", min: 0, max: 999, default: 400, desc: "dim level during the gesture" },
    Knob { name: "sleep_flare_permille", group: "sleep / wake", min: 0, max: 999, default: 750, desc: "flare level during the gesture" },
    Knob { name: "sleep_wake_spread_ms", group: "sleep / wake", min: 1, max: 3600000, default: 120, desc: "stagger between consecutive eyes waking" },
    // --- agitation_* / green_* — mode 0 green agitation ---
    Knob { name: "agitation_hold_ms", group: "agitation (green)", min: 1, max: 3600000, default: 1500, desc: "radar-quiet time before the sweep leaves" },
    Knob { name: "agitation_exit_ms", group: "agitation (green)", min: 1, max: 3600000, default: 1000, desc: "crossfade back into the embers" },
    Knob { name: "agitation_flicker_pwm", group: "agitation (green)", min: 0, max: 4094, default: 1200, desc: "shared flicker amplitude" },
    Knob { name: "agitation_flicker_step_ms", group: "agitation (green)", min: 1, max: 60000, default: 43, desc: "shared flicker refresh interval" },
    Knob { name: "agitation_sweep_min_ms", group: "agitation (green)", min: 1, max: 3600000, default: 250, desc: "fastest sweep across the face" },
    Knob { name: "agitation_sweep_max_ms", group: "agitation (green)", min: 1, max: 3600000, default: 1000, desc: "slowest sweep across the face" },
    Knob { name: "agitation_sweep_pause_min_ms", group: "agitation (green)", min: 1, max: 3600000, default: 500, desc: "shortest rest at the end eye" },
    Knob { name: "agitation_sweep_pause_max_ms", group: "agitation (green)", min: 1, max: 3600000, default: 2000, desc: "longest rest at the end eye" },
    Knob { name: "agitation_sweep_tail_permille", group: "agitation (green)", min: 1, max: 7000, default: 3500, desc: "trailing glow length (3.5 eyes at 3500)" },
    Knob { name: "green_spread_ms", group: "agitation (green)", min: 0, max: 3600000, default: 120, desc: "stagger between consecutive eyes" },
    Knob { name: "green_attack_ms", group: "agitation (green)", min: 1, max: 60000, default: 40, desc: "snap-on ramp for a green eye" },
    Knob { name: "green_fade_ms", group: "agitation (green)", min: 1, max: 3600000, default: 800, desc: "fade back out once motion stops" },
    Knob { name: "green_flicker_pwm", group: "agitation (green)", min: 0, max: 4095, default: 1200, desc: "ember noise at full green brightness" },
    // --- stalker_* — mode 3 (Stalker) ---
    Knob { name: "stalker_step_ms", group: "stalker", min: 1, max: 3600000, default: 900, desc: "dwell per eye while scanning" },
    Knob { name: "stalker_dim_pwm", group: "stalker", min: 0, max: 4095, default: 700, desc: "peak of the dim scanning glow" },
    Knob { name: "stalker_green_permille", group: "stalker", min: 0, max: 1000, default: 620, desc: "green read-brightness trim" },
    Knob { name: "stalker_flicker_pwm", group: "stalker", min: 0, max: 4095, default: 600, desc: "shimmer on the scan (at full)" },
    Knob { name: "stalker_flicker_step_ms", group: "stalker", min: 1, max: 60000, default: 53, desc: "shimmer refresh interval" },
    Knob { name: "stalker_pop_ms", group: "stalker", min: 1, max: 3600000, default: 220, desc: "ramp up to the red pop on motion" },
    Knob { name: "stalker_release_ms", group: "stalker", min: 1, max: 3600000, default: 900, desc: "decay back to scanning" },
    Knob { name: "stalker_pop_pwm", group: "stalker", min: 0, max: 4095, default: 4095, desc: "settled pop level" },
    Knob { name: "stalker_pop_flicker_pwm", group: "stalker", min: 0, max: 4095, default: 1000, desc: "stutter on the pop (at full)" },
    // --- toxic_* — mode 6 (Toxic) ---
    Knob { name: "toxic_green_base_pwm", group: "toxic", min: 0, max: 4095, default: 700, desc: "steady green venom glow" },
    Knob { name: "toxic_green_flicker_pwm", group: "toxic", min: 0, max: 4095, default: 1000, desc: "bubble amplitude (at full)" },
    Knob { name: "toxic_green_step_ms", group: "toxic", min: 1, max: 60000, default: 61, desc: "bubble refresh interval" },
    Knob { name: "toxic_spark_idle_pwm", group: "toxic", min: 0, max: 4095, default: 1600, desc: "spark head level, idle" },
    Knob { name: "toxic_spark_head_pwm", group: "toxic", min: 0, max: 4095, default: 4095, desc: "spark head level, frantic" },
    Knob { name: "toxic_spark_flicker_pwm", group: "toxic", min: 0, max: 4095, default: 700, desc: "idle spark flicker (at full)" },
    Knob { name: "toxic_spark_frantic_pwm", group: "toxic", min: 0, max: 4095, default: 1400, desc: "spark flicker when frantic" },
    Knob { name: "toxic_spark_idle_step_ms", group: "toxic", min: 1, max: 3600000, default: 420, desc: "ms per eye, idle" },
    Knob { name: "toxic_spark_fast_step_ms", group: "toxic", min: 1, max: 3600000, default: 130, desc: "ms per eye, frantic" },
    Knob { name: "toxic_spark_tail_permille", group: "toxic", min: 1, max: 1000, default: 900, desc: "trailing glow behind the spark" },
    Knob { name: "toxic_motion_ramp_ms", group: "toxic", min: 1, max: 3600000, default: 600, desc: "ramp to/from frantic" },
    Knob { name: "toxic_flicker_step_ms", group: "toxic", min: 1, max: 60000, default: 43, desc: "spark flicker refresh interval" },
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

    /// The `Meaning` column of the firmware's knob reference (`esp32-spider` `API.md`,
    /// which ships here verbatim) is the text the tuning form shows for each knob.
    #[test]
    fn descriptions_match_the_api_document() {
        let mut doc: Vec<(&str, u32, u32, u32, &str)> = Vec::new();
        for line in include_str!("../API.md").lines() {
            let Some(rest) = line.strip_prefix("| `") else {
                continue;
            };
            let cols: Vec<&str> = rest.trim_end_matches('|').split('|').map(str::trim).collect();
            if cols.len() != 5 {
                continue;
            }
            let (Ok(default), Ok(lo), Ok(hi)) = (
                cols[1].parse::<u32>(),
                cols[2].parse::<u32>(),
                cols[3].parse::<u32>(),
            ) else {
                continue; // not a knob row (e.g. the constraint table)
            };
            doc.push((cols[0].trim_end_matches('`'), default, lo, hi, cols[4]));
        }
        assert_eq!(doc.len(), KNOBS.len(), "knob reference row count");
        for k in KNOBS {
            let row = doc
                .iter()
                .find(|(name, ..)| *name == k.name)
                .unwrap_or_else(|| panic!("{}: missing from API.md", k.name));
            assert_eq!((k.default, k.min, k.max), (row.1, row.2, row.3), "{}: numbers", k.name);
            assert_eq!(k.desc, row.4, "{}: description", k.name);
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
                ("pulse", 3),
                ("smolder embers", 5),
                ("sleep / wake", 16),
                ("agitation (green)", 13),
                ("stalker", 9),
                ("toxic", 12),
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
