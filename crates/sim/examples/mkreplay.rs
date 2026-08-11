//! Record a bot game to a replay file: `mkreplay out.ron [seed]`.

use orion_sim::ai::{Bot, Difficulty};
use orion_sim::map::meridian;
use orion_sim::replay::Replay;
use orion_sim::{GameData, State};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "replay.ron".into());
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(42);

    let mut s = State::new_with_races(GameData::load_default(), meridian(), seed, &[0, 1]);
    let mut bots = [
        Bot::with_style(0, Difficulty::Hard, seed ^ 7),
        Bot::with_style(1, Difficulty::Hard, seed ^ 99),
    ];
    while s.winner.is_none() && s.tick < 24 * 60 * 20 {
        let mut cmds = Vec::new();
        for b in &mut bots {
            cmds.extend(b.think(&s));
        }
        s.step(&cmds);
    }
    let r = Replay::from_state(&s, "meridian", None, vec!["ALPHA BOT".into(), "BETA BOT".into()]);
    std::fs::write(&out, r.to_ron()).expect("write replay");
    println!(
        "wrote {out}: winner={:?} ticks={} commands={} ({} KB)",
        s.winner,
        s.tick,
        r.commands.len(),
        r.to_ron().len() / 1024
    );
}
