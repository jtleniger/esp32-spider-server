//! The live mode numbers, transcribed from `lib/ledfx/modes.h`.
//!
//! 4, 5 and 7 were retired and their numbers must not be reused; they are never
//! offered and the firmware rejects them.

pub struct Mode {
    pub n: u16,
    pub name: &'static str,
    pub desc: &'static str,
}

pub const MODES: &[Mode] = &[
    Mode {
        n: 0,
        name: "smolder",
        desc: "sleeping/waking ember eyes that agitate green on motion",
    },
    Mode {
        n: 1,
        name: "single channel",
        desc: "lights only channel c at full brightness (wiring checks)",
    },
    Mode {
        n: 2,
        name: "pulse",
        desc: "all channels breathe in lockstep",
    },
    Mode {
        n: 3,
        name: "stalker",
        desc: "one dim random-colour eye scans; motion ramps to a red pop",
    },
    Mode {
        n: 6,
        name: "toxic",
        desc: "dim green base with an orange/red spark; motion makes it frantic",
    },
];

/// Display name for any number the rig might report, including retired holes.
pub fn name(n: u16) -> String {
    MODES
        .iter()
        .find(|m| m.n == n)
        .map(|m| m.name.to_string())
        .unwrap_or_else(|| format!("unknown ({n})"))
}

/// True for the numbers this UI offers and `POST /mode` accepts.
pub fn is_valid(n: u16) -> bool {
    MODES.iter().any(|m| m.n == n)
}

/// Channel count of the single-channel mode (`?c=0..23`).
pub const CHANNELS: u16 = 24;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_modes_are_named_and_holes_are_not_offered() {
        assert_eq!(name(0), "smolder");
        assert_eq!(name(6), "toxic");
        assert_eq!(name(4), "unknown (4)");
        assert_eq!(name(7), "unknown (7)");
        assert!(is_valid(1));
        assert!(!is_valid(4));
        assert!(!is_valid(8));
    }
}
