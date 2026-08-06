//! Guided first game: a sequence of objectives over a normal single-player
//! state — the sim is untouched, no bot thinks, and a small enemy outpost
//! waits to be destroyed as the finale. Each objective is a predicate over
//! the app; the HUD draws the current one with a hint.

use orion_sim::{EntityKind, Order, State};

pub struct Tutorial {
    pub step: usize,
    /// Camera position when the tutorial began — objective 1 is "move it".
    pub cam0: (f32, f32),
    /// Seconds left of the "objective complete" flash.
    pub flash: f32,
}

pub struct Objective {
    pub title: &'static str,
    pub hint: &'static str,
}

pub const OBJECTIVES: [Objective; 7] = [
    Objective {
        title: "LOOK AROUND",
        hint: "PAN THE CAMERA: ARROW KEYS, SCREEN EDGE, OR MIDDLE-DRAG",
    },
    Objective {
        title: "SELECT YOUR FABRICATORS",
        hint: "DRAG A BOX AROUND THE WORKERS BY YOUR HQ",
    },
    Objective {
        title: "PUT THEM TO WORK",
        hint: "RIGHT-CLICK THE BLUE MINERAL CRYSTALS TO MINE",
    },
    Objective {
        title: "BUILD A SUPPLY PYLON",
        // {BUILD} is filled from the live build grid + keybinds.
        hint: "SELECT A WORKER, PRESS {BUILD}, CLICK OPEN GROUND",
    },
    Objective {
        title: "BUILD A MUSTER HALL",
        hint: "{BUILD} WITH A WORKER - IT TRAINS YOUR ARMY",
    },
    Objective {
        title: "TRAIN 4 TROOPERS",
        hint: "SELECT THE MUSTER HALL AND PRESS Q - QUEUE THEM UP",
    },
    Objective {
        title: "DESTROY THE OUTPOST",
        hint: "SELECT YOUR TROOPERS, PRESS A, CLICK THE ENEMY CAMP MID-MAP",
    },
];

impl Tutorial {
    pub fn new(cam0: (f32, f32)) -> Tutorial {
        Tutorial { step: 0, cam0, flash: 0.0 }
    }

    /// Is the current objective satisfied by this state?
    pub fn current_done(&self, s: &State, human: u8) -> bool {
        let my_units = |f: &dyn Fn(&orion_sim::Entity) -> bool| {
            s.entities
                .iter()
                .filter(|e| e.alive && e.owner == human && f(e))
                .count()
        };
        match self.step {
            // Steps 0 (camera) and 1 (selection) are app-side state; the
            // app checks them directly.
            0 | 1 => false,
            2 => {
                my_units(&|e| {
                    e.kind == EntityKind::Unit
                        && s.data.units[e.def as usize].harvester
                        && matches!(e.order, Order::Gather { .. })
                }) >= 3
            }
            3 => {
                my_units(&|e| {
                    e.kind == EntityKind::Building
                        && s.data.buildings[e.def as usize].supply_provided > 0
                        && !s.data.buildings[e.def as usize].headquarters
                }) >= 1
            }
            4 => {
                my_units(&|e| {
                    e.kind == EntityKind::Building
                        && e.construction.is_none()
                        && !s.data.buildings[e.def as usize].trains.is_empty()
                        && !s.data.buildings[e.def as usize].headquarters
                }) >= 1
            }
            5 => {
                my_units(&|e| {
                    e.kind == EntityKind::Unit
                        && !s.data.units[e.def as usize].harvester
                }) >= 4
            }
            6 => {
                // The outpost is the only enemy presence.
                s.entities
                    .iter()
                    .filter(|e| e.alive && e.owner != human && e.kind == EntityKind::Building)
                    .count()
                    == 0
            }
            _ => false,
        }
    }

    pub fn finished(&self) -> bool {
        self.step >= OBJECTIVES.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orion_sim::fixed::FxVec2;
    use orion_sim::{Command, GameData, State, TilePos};

    fn tut_state() -> (Tutorial, State) {
        let mut s = State::new(GameData::load_default(), orion_sim::map::meridian(), 7);
        // Clear the enemy start like start_tutorial does.
        for i in 0..s.entities.len() {
            if s.entities[i].alive && s.entities[i].owner == 1 {
                s.kill(i as u32);
            }
        }
        (Tutorial::new((0.0, 0.0)), s)
    }

    fn def_u(s: &State, tag: &str) -> u16 {
        s.data.units.iter().position(|u| u.tag == tag).unwrap() as u16
    }
    fn def_b(s: &State, tag: &str) -> u16 {
        s.data.buildings.iter().position(|b| b.tag == tag).unwrap() as u16
    }

    #[test]
    fn objectives_advance_in_order() {
        let (mut tut, mut s) = tut_state();
        tut.step = 2;
        assert!(!tut.current_done(&s, 0), "no one mining yet");
        // Order three workers to the nearest mineral.
        let workers: Vec<_> = s
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive && e.owner == 0 && s.data.units[e.def as usize].harvester)
            .map(|(i, _)| s.id_of(i as u32))
            .collect();
        // Nearest resource to the start is a mineral patch (not a geyser
        // or a destructible).
        let start = s.map.starts[0].center();
        let mineral = s
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.alive && e.kind == EntityKind::Resource)
            .min_by_key(|(_, e)| orion_sim::fixed::dist_sq_raw(e.pos, start))
            .map(|(i, _)| s.id_of(i as u32))
            .unwrap();
        // Box-the-workers-and-right-click: patches have limited mining
        // slots, so ordering everyone still seats at least three.
        s.step(&[(0, Command::Gather { units: workers, resource: mineral, queued: false })]);
        s.step(&[]);
        assert!(tut.current_done(&s, 0), "three-plus workers gathering");

        tut.step = 3;
        assert!(!tut.current_done(&s, 0));
        let depot = def_b(&s, "depot");
        s.spawn_building(0, depot, TilePos::new(20, 20), false);
        assert!(tut.current_done(&s, 0), "supply building counts");

        tut.step = 4;
        assert!(!tut.current_done(&s, 0), "no production building yet");
        let barracks = def_b(&s, "barracks");
        s.spawn_building(0, barracks, TilePos::new(24, 20), false);
        assert!(tut.current_done(&s, 0), "barracks counts");

        tut.step = 5;
        assert!(!tut.current_done(&s, 0));
        let trooper = def_u(&s, "trooper");
        for k in 0..4 {
            s.spawn_unit(0, trooper, FxVec2::from_int(30 + k, 30));
        }
        assert!(tut.current_done(&s, 0), "four troopers");

        tut.step = 6;
        // Raise the outpost first (start_tutorial does this at setup).
        let warren = def_b(&s, "warren");
        let outpost = s.spawn_building(1, warren, TilePos::new(40, 40), false);
        assert!(!tut.current_done(&s, 0), "outpost still stands");
        s.kill(outpost.idx);
        assert!(tut.current_done(&s, 0), "outpost destroyed");
    }
}
