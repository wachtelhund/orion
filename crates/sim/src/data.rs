//! Game content, loaded from RON. Everything balance-related lives in
//! `assets/units.ron`, not in code — this is what makes adding race #2
//! a content problem instead of an engineering problem.
//!
//! Human-friendly units in the file (seconds, tiles/sec) are converted to
//! sim units (ticks, per-tick fixed steps) once at load.

use serde::Deserialize;

use crate::fixed::Fx;
use crate::TICKS_PER_SEC;

pub type DefId = u16;

#[derive(Debug, Deserialize)]
pub struct WeaponRaw {
    pub damage: i32,
    pub range: f64,
    pub cooldown_s: f64,
    /// Can shoot flyers.
    #[serde(default)]
    pub air: bool,
    /// Splash radius in tiles (0 = single target). Splash hits friendlies.
    #[serde(default)]
    pub splash: f64,
    /// Cannot fire closer than this (siege weapons).
    #[serde(default)]
    pub min_range: f64,
}

#[derive(Debug, Deserialize)]
pub struct UnitRaw {
    pub id: String,
    pub name: String,
    pub hp: i32,
    pub speed: f64,
    pub radius: f64,
    pub sight: u32,
    pub cost_minerals: u32,
    #[serde(default)]
    pub cost_gas: u32,
    pub supply: u32,
    pub build_time_s: f64,
    pub weapon: Option<WeaponRaw>,
    /// Alternate stationary-mode weapon (siege mode).
    #[serde(default)]
    pub weapon_siege: Option<WeaponRaw>,
    #[serde(default)]
    pub harvester: bool,
    #[serde(default)]
    pub builder: bool,
    /// Flies: ignores terrain, only air-capable weapons can hit it.
    #[serde(default)]
    pub fly: bool,
    /// Spellcaster energy pool (0 = not a caster).
    #[serde(default)]
    pub energy: u32,
    /// Building tag that must exist (completed) before training this unit.
    #[serde(default)]
    pub requires: Option<String>,
    /// Tooltip flavor line.
    #[serde(default)]
    pub desc: String,
    /// Which race this belongs to (index into race_names).
    #[serde(default)]
    pub race: u8,
    /// Deployable shield projector: (radius tiles, damage reduction %).
    /// Deploying uses the siege toggle; the aura is active while deployed.
    #[serde(default)]
    pub shield_aura: Option<(f64, u32)>,
    /// Can burrow: hidden and untargetable underground, but cannot act.
    #[serde(default)]
    pub burrow: bool,
}

#[derive(Debug, Deserialize)]
pub struct BuildingRaw {
    pub id: String,
    pub name: String,
    pub hp: i32,
    pub footprint: (u32, u32),
    pub sight: u32,
    pub cost_minerals: u32,
    #[serde(default)]
    pub cost_gas: u32,
    pub supply_provided: u32,
    pub build_time_s: f64,
    #[serde(default)]
    pub trains: Vec<String>,
    #[serde(default)]
    pub deposit: bool,
    #[serde(default)]
    pub headquarters: bool,
    /// Built on top of a plasma geyser; workers harvest gas from it.
    #[serde(default)]
    pub gas_extractor: bool,
    /// Building tag that must exist (completed) before placing this.
    #[serde(default)]
    pub requires: Option<String>,
    /// Research ids this building can run.
    #[serde(default)]
    pub researches: Vec<String>,
    /// Tooltip flavor line.
    #[serde(default)]
    pub desc: String,
    /// Which race this belongs to (index into race_names).
    #[serde(default)]
    pub race: u8,
}

#[derive(Debug, Deserialize)]
pub struct ResearchRaw {
    pub id: String,
    pub name: String,
    pub cost_minerals: u32,
    pub cost_gas: u32,
    pub time_s: f64,
    #[serde(default)]
    pub requires: Option<String>,
    #[serde(default)]
    pub desc: String,
}

#[derive(Debug, Deserialize)]
pub struct RaceRaw {
    pub name: String,
    /// Playable race names; defs reference these by index via `race`.
    #[serde(default)]
    pub race_names: Vec<String>,
    pub units: Vec<UnitRaw>,
    pub buildings: Vec<BuildingRaw>,
    #[serde(default)]
    pub research: Vec<ResearchRaw>,
}

// ---- Compiled (sim-unit) forms ----

#[derive(Debug, Clone)]
pub struct Weapon {
    pub damage: i32,
    pub range: Fx,
    pub cooldown_ticks: u16,
    pub air: bool,
    pub splash: Fx,
    pub min_range: Fx,
}

#[derive(Debug, Clone)]
pub struct UnitDef {
    pub tag: String,
    pub name: String,
    pub hp: i32,
    /// Movement per tick.
    pub step: Fx,
    pub radius: Fx,
    pub sight: u32,
    pub cost_minerals: u32,
    pub cost_gas: u32,
    pub supply: u32,
    pub build_ticks: u32,
    pub weapon: Option<Weapon>,
    pub weapon_siege: Option<Weapon>,
    pub harvester: bool,
    pub builder: bool,
    pub fly: bool,
    pub energy_max: u32,
    /// Index into buildings.
    pub requires: Option<DefId>,
    pub desc: String,
    pub race: u8,
    pub shield_aura: Option<(Fx, i32)>,
    pub burrow: bool,
}

#[derive(Debug, Clone)]
pub struct BuildingDef {
    pub tag: String,
    pub name: String,
    pub hp: i32,
    pub footprint: (i32, i32),
    pub sight: u32,
    pub cost_minerals: u32,
    pub cost_gas: u32,
    pub supply_provided: u32,
    pub build_ticks: u32,
    /// Indices into `GameData::units`.
    pub trains: Vec<DefId>,
    pub deposit: bool,
    pub headquarters: bool,
    pub gas_extractor: bool,
    pub requires: Option<DefId>,
    /// Indices into `GameData::research`.
    pub researches: Vec<u8>,
    pub desc: String,
    pub race: u8,
}

#[derive(Debug, Clone)]
pub struct ResearchDef {
    pub tag: String,
    pub name: String,
    pub cost_minerals: u32,
    pub cost_gas: u32,
    pub ticks: u32,
    pub requires: Option<u8>,
    /// "weapons" or "armor" — what completing this bumps.
    pub upgrades_weapons: bool,
    pub desc: String,
}

#[derive(Debug)]
pub struct GameData {
    pub race_name: String,
    pub race_names: Vec<String>,
    pub units: Vec<UnitDef>,
    pub buildings: Vec<BuildingDef>,
    pub research: Vec<ResearchDef>,
}

impl GameData {
    pub fn load_default() -> GameData {
        Self::load(include_str!("../assets/units.ron"))
    }

    pub fn load(src: &str) -> GameData {
        let raw: RaceRaw = ron::from_str(src).expect("units.ron parse error");
        let ticks = |s: f64| -> u32 { (s * TICKS_PER_SEC as f64).round() as u32 };
        let weapon = |w: &WeaponRaw| Weapon {
            damage: w.damage,
            range: Fx::from_f64_data(w.range),
            cooldown_ticks: ticks(w.cooldown_s) as u16,
            air: w.air,
            splash: Fx::from_f64_data(w.splash),
            min_range: Fx::from_f64_data(w.min_range),
        };

        let building_index = |tag: &str| -> DefId {
            raw.buildings
                .iter()
                .position(|b| b.id == tag)
                .unwrap_or_else(|| panic!("unknown building tag in requires: {tag}"))
                as DefId
        };

        let units: Vec<UnitDef> = raw
            .units
            .iter()
            .map(|u| UnitDef {
                tag: u.id.clone(),
                name: u.name.clone(),
                hp: u.hp,
                step: Fx::from_f64_data(u.speed / TICKS_PER_SEC as f64),
                radius: Fx::from_f64_data(u.radius),
                sight: u.sight,
                cost_minerals: u.cost_minerals,
                cost_gas: u.cost_gas,
                supply: u.supply,
                build_ticks: ticks(u.build_time_s),
                weapon: u.weapon.as_ref().map(&weapon),
                weapon_siege: u.weapon_siege.as_ref().map(&weapon),
                harvester: u.harvester,
                builder: u.builder,
                fly: u.fly,
                energy_max: u.energy,
                requires: u.requires.as_deref().map(building_index),
                desc: u.desc.clone(),
                race: u.race,
                shield_aura: u
                    .shield_aura
                    .map(|(r, pct)| (Fx::from_f64_data(r), pct as i32)),
                burrow: u.burrow,
            })
            .collect();

        let unit_index = |tag: &str| -> DefId {
            units
                .iter()
                .position(|u| u.tag == tag)
                .unwrap_or_else(|| panic!("unknown unit tag in trains list: {tag}"))
                as DefId
        };
        let research_index = |tag: &str| -> u8 {
            raw.research
                .iter()
                .position(|r| r.id == tag)
                .unwrap_or_else(|| panic!("unknown research tag: {tag}")) as u8
        };

        let buildings = raw
            .buildings
            .iter()
            .map(|b| BuildingDef {
                tag: b.id.clone(),
                name: b.name.clone(),
                hp: b.hp,
                footprint: (b.footprint.0 as i32, b.footprint.1 as i32),
                sight: b.sight,
                cost_minerals: b.cost_minerals,
                cost_gas: b.cost_gas,
                supply_provided: b.supply_provided,
                build_ticks: ticks(b.build_time_s),
                trains: b.trains.iter().map(|t| unit_index(t)).collect(),
                deposit: b.deposit,
                headquarters: b.headquarters,
                gas_extractor: b.gas_extractor,
                requires: b.requires.as_deref().map(building_index),
                researches: b.researches.iter().map(|r| research_index(r)).collect(),
                desc: b.desc.clone(),
                race: b.race,
            })
            .collect();

        let research = raw
            .research
            .iter()
            .map(|r| ResearchDef {
                tag: r.id.clone(),
                name: r.name.clone(),
                cost_minerals: r.cost_minerals,
                cost_gas: r.cost_gas,
                ticks: ticks(r.time_s),
                requires: r.requires.as_deref().map(research_index),
                upgrades_weapons: r.id.starts_with("weapons"),
                desc: r.desc.clone(),
            })
            .collect();

        let race_names = if raw.race_names.is_empty() {
            vec![raw.name.clone()]
        } else {
            raw.race_names.clone()
        };
        GameData { race_name: raw.name, race_names, units, buildings, research }
    }

    /// First headquarters building of a race.
    pub fn hq_of_race(&self, race: u8) -> DefId {
        self.buildings
            .iter()
            .position(|b| b.headquarters && b.race == race)
            .expect("race has no headquarters") as DefId
    }

    /// First harvester unit of a race.
    pub fn worker_of_race(&self, race: u8) -> DefId {
        self.units
            .iter()
            .position(|u| u.harvester && u.race == race)
            .expect("race has no worker") as DefId
    }

    pub fn unit_tag(&self, tag: &str) -> DefId {
        self.units.iter().position(|u| u.tag == tag).expect("unit tag") as DefId
    }

    pub fn building_tag(&self, tag: &str) -> DefId {
        self.buildings.iter().position(|b| b.tag == tag).expect("building tag") as DefId
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_data_loads() {
        let d = GameData::load_default();
        assert!(d.units.len() >= 6);
        assert!(d.buildings.len() >= 7);
        assert_eq!(d.research.len(), 4);
        let hq = &d.buildings[d.building_tag("hq") as usize];
        assert!(hq.deposit && hq.headquarters && !hq.trains.is_empty());
        let worker = &d.units[d.unit_tag("fabricator") as usize];
        assert!(worker.harvester && worker.builder);
        // Tech tree sanity.
        let breaker = &d.units[d.unit_tag("breaker") as usize];
        assert!(breaker.weapon_siege.is_some());
        let sky = &d.units[d.unit_tag("skywing") as usize];
        assert!(sky.fly && sky.weapon.as_ref().unwrap().air);
        let caster = &d.units[d.unit_tag("stormcaller") as usize];
        assert!(caster.energy_max > 0 && caster.requires.is_some());
        let aerie = &d.buildings[d.building_tag("aerie") as usize];
        assert_eq!(aerie.requires, Some(d.building_tag("forge")));
        let archive = &d.buildings[d.building_tag("archive") as usize];
        assert_eq!(archive.researches.len(), 4);
    }
}
