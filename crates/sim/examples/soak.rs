//! The AI-testing workhorse: play N full bot matches headless, check every
//! invariant, and print an aggregate report + per-game CSV.
//!
//!   cargo run --release -p orion-sim --example soak            # 32 games
//!   cargo run --release -p orion-sim --example soak -- 100     # more
//!   cargo run --release -p orion-sim --example soak -- 32 out.csv
//!
//! Exit code is non-zero on any invariant violation, crash-free by
//! construction (a panic = reproducible bug: the config prints first).

use std::time::Instant;

use orion_sim::qa::{run_game, soak_configs, GameReport};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(32);
    let csv_path = args.get(2).cloned();

    let start = Instant::now();
    let cfgs = soak_configs(n, 0x0510, 5);
    let mut reports: Vec<GameReport> = Vec::new();
    for (k, cfg) in cfgs.into_iter().enumerate() {
        // Print the config BEFORE running: a panic mid-game is reproducible.
        eprintln!(
            "[{}/{n}] seed={:#x} races={:?} shadow={}",
            k + 1,
            cfg.seed,
            cfg.races,
            cfg.shadow
        );
        reports.push(run_game(cfg));
    }
    let wall = start.elapsed();

    // ---- aggregate ----
    let race_name = |r: u8| if r == 0 { "VC" } else { "Kyth" };
    println!("\n=== ORION SOAK: {} games in {:.1}s ===", reports.len(), wall.as_secs_f32());
    for matchup in [[0u8, 0u8], [1, 1], [0, 1], [1, 0]] {
        let games: Vec<&GameReport> =
            reports.iter().filter(|r| r.cfg.races == matchup).collect();
        if games.is_empty() {
            continue;
        }
        let w0 = games.iter().filter(|g| g.winner == Some(0)).count();
        let w1 = games.iter().filter(|g| g.winner == Some(1)).count();
        let stalls = games.iter().filter(|g| g.winner.is_none()).count();
        let avg_secs: u32 = games
            .iter()
            .filter(|g| g.winner.is_some())
            .map(|g| g.ticks / orion_sim::TICKS_PER_SEC)
            .sum::<u32>()
            / (games.len() - stalls).max(1) as u32;
        println!(
            "{:>4} vs {:<5} {:>2} games  {w0:>2} - {w1:<2}  stalls {stalls}  avg {}:{:02}",
            race_name(matchup[0]),
            race_name(matchup[1]),
            games.len(),
            avg_secs / 60,
            avg_secs % 60
        );
    }

    let shadows = reports.iter().filter(|r| r.cfg.shadow).count();
    let shadow_fails = reports
        .iter()
        .filter(|r| r.violations.iter().any(|v| v.contains("SHADOW DESYNC")))
        .count();
    println!("determinism shadows: {}/{} clean", shadows - shadow_fails, shadows);

    let no_combat = reports
        .iter()
        .filter(|r| r.first_blood_tick.is_none() && r.winner.is_none())
        .count();
    if no_combat > 0 {
        println!("games with zero combat: {no_combat}  <-- smell");
    }

    let mut total_violations = 0;
    for r in &reports {
        for v in &r.violations {
            total_violations += 1;
            if total_violations <= 20 {
                println!(
                    "VIOLATION seed={:#x} races={:?}: {v}",
                    r.cfg.seed, r.cfg.races
                );
            }
        }
    }
    println!(
        "violations: {total_violations}   stalls: {}",
        reports.iter().filter(|r| r.winner.is_none()).count()
    );

    if let Some(path) = csv_path {
        let mut out = String::from(GameReport::csv_header());
        out.push('\n');
        for r in &reports {
            out.push_str(&r.csv_row());
            out.push('\n');
        }
        std::fs::write(&path, out).expect("write csv");
        println!("per-game csv -> {path}");
    }

    if total_violations > 0 {
        std::process::exit(1);
    }
}
