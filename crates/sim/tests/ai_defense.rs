//! The bot must DEFEND. Regression for the reported hole: enemy air units
//! parked over the bot's mineral line were ignored entirely — no
//! interception, no worker reaction, nothing.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::meridian;
use orion_sim::{FxVec2, GameData, State};

/// Enemy flyers hovering the bot's mineral line get engaged by anti-air
/// within 30 seconds.
#[test]
fn bot_intercepts_air_harass() {
    let mut s =
        State::new_with_races(GameData::load_default(), meridian(), 5, &[0, 0]);
    let mut bot = Bot::with_style(0, Difficulty::Hard, 3);

    // Let the bot build an economy and some army in peace.
    for _ in 0..24 * 200 {
        let cmds = bot.think(&s);
        s.step(&cmds);
    }

    // Park three enemy skywings over the mineral line, SC-cheese style.
    let skywing = s.data.unit_tag("skywing");
    let hover = FxVec2::new(
        orion_sim::Fx::from_ratio(13, 2),
        orion_sim::Fx::from_ratio(21, 2),
    );
    let mut raiders = Vec::new();
    for k in 0..3 {
        let off = FxVec2::new(
            orion_sim::Fx::from_int(k % 2),
            orion_sim::Fx::from_int(k),
        );
        raiders.push(s.spawn_unit(1, skywing, FxVec2::new(hover.x + off.x, hover.y + off.y)));
    }
    let hp0: i32 = raiders
        .iter()
        .map(|id| s.get(*id).map(|e| e.hp).unwrap_or(0))
        .sum();
    assert!(hp0 > 0);

    // 30 seconds of bot thinking. The old bot did NOTHING here forever.
    for _ in 0..24 * 30 {
        let cmds = bot.think(&s);
        s.step(&cmds);
    }
    let hp1: i32 = raiders
        .iter()
        .map(|id| s.get(*id).map(|e| e.hp).unwrap_or(0))
        .sum();
    assert!(
        hp1 < hp0,
        "harassing flyers untouched after 30s (hp {hp0} -> {hp1}): the bot is not defending"
    );
}
