//! The mini-campaign: five scripted missions over the tutorial's
//! objective machinery. Each mission is a normal SP state mutated at
//! start (outposts, grants, heroes), local bots as command sources for
//! the other seats, timed spawn waves, and a short objective list drawn
//! by the HUD. Replays stay off — scripted events aren't commands, so a
//! mission isn't a valid command stream.

use orion_sim::ai::Difficulty;
use orion_sim::{EntityKind, Order, State};

pub struct MissionDef {
    pub title: &'static str,
    /// One-line pitch shown on the campaign page.
    pub blurb: &'static str,
    pub map: &'static str,
    /// Race per player, player 0 is the human.
    pub races: &'static [u8],
    /// Team per player (identity for 1v1 missions).
    pub teams: &'static [u8],
    /// Bot-driven seats: (player, difficulty). Seats absent here are
    /// static — buildings stand, nobody thinks.
    pub bots: &'static [(u8, Difficulty)],
    /// Survive-mission length in seconds; None = destroy the enemy.
    pub survive_s: Option<u32>,
    pub objectives: &'static [(&'static str, &'static str)],
}

pub const MISSIONS: [MissionDef; 5] = [
    MissionDef {
        title: "BROKEN DAWN",
        blurb: "REBUILD THE MINING LINE AND BURN OUT THE KYTH WARREN",
        map: "meridian",
        races: &[0, 1],
        teams: &[0, 1],
        bots: &[], // a static outpost plus scripted raids
        survive_s: None,
        objectives: &[
            ("RESTORE THE MINING LINE", "GET 10 FABRICATORS ON MINERALS"),
            ("MUSTER A STRIKE FORCE", "FIELD 8 TROOPERS - EXPECT RAIDS"),
            ("ERADICATE THE WARREN", "RAZE EVERY KYTH STRUCTURE MID-MAP"),
        ],
    },
    MissionDef {
        title: "HOLD THE LINE",
        blurb: "THE FERRON COMPACT COMES FOR THE CAUSEWAY - OUTLAST THEM",
        map: "causeway",
        races: &[0, 2],
        teams: &[0, 1],
        bots: &[(1, Difficulty::Hard)],
        survive_s: Some(480),
        objectives: &[(
            "SURVIVE THE ONSLAUGHT",
            "KEEP YOUR HEADQUARTERS STANDING FOR {TIME}",
        )],
    },
    MissionDef {
        title: "THE ASSEMBLY",
        blurb: "FIGHT AS THE KYTH - THE SWARM ANSWERS TO YOU NOW",
        map: "caverns",
        races: &[1, 0],
        teams: &[0, 1],
        bots: &[(1, Difficulty::Normal)],
        survive_s: None,
        objectives: &[(
            "ANNIHILATE THE VANGUARD",
            "SWARM CHEAP AND FAST - SKITTERS OVERWHELM, SPITTERS SIEGE",
        )],
    },
    MissionDef {
        title: "IRON COMPACT",
        blurb: "FIGHT AS THE FERRON - HOLD THE HIGH GROUND OF THORNWOOD",
        map: "thornwood",
        races: &[2, 1],
        teams: &[0, 1],
        bots: &[(1, Difficulty::Hard)],
        survive_s: None,
        objectives: &[(
            "BREAK THE SWARM",
            "ARCLIGHTS HOLD LINES - TAKE HIGH GROUND, MIND THE TREES",
        )],
    },
    MissionDef {
        title: "MERIDIAN'S END",
        blurb: "THE FINALE: TWO ARMIES, ONE ALLY, MARSHAL KADE AT THE FRONT",
        map: "crossfire",
        races: &[0, 0, 1, 2],
        teams: &[0, 0, 1, 1],
        bots: &[
            (1, Difficulty::Normal),
            (2, Difficulty::Normal),
            (3, Difficulty::Normal),
        ],
        survive_s: None,
        objectives: &[(
            "END IT",
            "MARSHAL KADE LEADS YOUR ARMY - DESTROY BOTH ENEMY BASES",
        )],
    },
];

/// A scripted enemy raid: units spawned at a map-fraction position and
/// thrown at another, `at_s` seconds in.
pub struct Wave {
    pub at_s: u32,
    pub owner: u8,
    pub units: &'static [(&'static str, u32)],
    /// Spawn point and attack target as map fractions (0..1).
    pub from: (f32, f32),
    pub to: (f32, f32),
}

/// Mission 1's raids; other missions use live bots instead.
pub const M1_WAVES: [Wave; 2] = [
    Wave {
        at_s: 170,
        owner: 1,
        units: &[("skitter", 5)],
        from: (0.5, 0.5),
        to: (0.2, 0.2),
    },
    Wave {
        at_s: 330,
        owner: 1,
        units: &[("skitter", 6), ("spitter", 3)],
        from: (0.5, 0.5),
        to: (0.2, 0.2),
    },
];

pub fn waves(mission: usize) -> &'static [Wave] {
    match mission {
        0 => &M1_WAVES,
        _ => &[],
    }
}

/// Live progress through one mission.
pub struct CampaignRun {
    pub mission: usize,
    pub step: usize,
    /// Seconds left of the "objective complete" flash.
    pub flash: f32,
    pub waves_fired: usize,
    /// Set once the mission resolves so the banner only fires once.
    pub resolved: bool,
}

impl CampaignRun {
    pub fn new(mission: usize) -> CampaignRun {
        CampaignRun { mission, step: 0, flash: 0.0, waves_fired: 0, resolved: false }
    }

    pub fn def(&self) -> &'static MissionDef {
        &MISSIONS[self.mission]
    }

    pub fn finished_objectives(&self) -> bool {
        self.step >= self.def().objectives.len()
    }

    /// Is the current objective satisfied? Pure state predicates — the
    /// campaign has no camera/selection steps.
    pub fn current_done(&self, s: &State, human: u8) -> bool {
        let count = |f: &dyn Fn(&orion_sim::Entity) -> bool| {
            s.entities
                .iter()
                .filter(|e| e.alive && e.owner == human && f(e))
                .count()
        };
        match (self.mission, self.step) {
            (0, 0) => {
                count(&|e| {
                    e.kind == EntityKind::Unit
                        && s.data.units[e.def as usize].harvester
                        && matches!(e.order, Order::Gather { .. })
                }) >= 10
            }
            (0, 1) => {
                count(&|e| {
                    e.kind == EntityKind::Unit
                        && s.data.units[e.def as usize].tag == "trooper"
                }) >= 8
            }
            // Final objectives everywhere resolve through victory itself.
            _ => false,
        }
    }
}
