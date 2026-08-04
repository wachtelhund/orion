//! Replay files on disk: ~/.orion-replays/<date>-<map>.ron. Saving is
//! automatic at game end; the REPLAYS menu lists whatever is here.

use std::path::PathBuf;

use orion_sim::replay::Replay;

pub fn dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".orion-replays")
}

/// (year, month, day) from days since 1970-01-01 (Howard Hinnant's
/// civil_from_days — exact for any reasonable date).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (y, mo, d) = civil_from_days(secs.div_euclid(86400));
    let tod = secs.rem_euclid(86400);
    format!(
        "{y:04}{mo:02}{d:02}-{:02}{:02}{:02}",
        tod / 3600,
        (tod / 60) % 60,
        tod % 60
    )
}

pub fn save(replay: &Replay) -> std::io::Result<PathBuf> {
    let d = dir();
    std::fs::create_dir_all(&d)?;
    let path = d.join(format!("{}-{}.ron", timestamp(), replay.map));
    std::fs::write(&path, replay.to_ron())?;
    Ok(path)
}

/// Replays on disk, newest first: (display label, path). Reads each file's
/// header — replay files are kilobytes, listing stays instant.
pub fn list(race_names: &[String]) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .collect();
    files.sort();
    files.reverse();
    files
        .into_iter()
        .take(8)
        .filter_map(|p| {
            let src = std::fs::read_to_string(&p).ok()?;
            let r = Replay::from_ron(&src).ok()?;
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            let date = stem.get(..13).unwrap_or(&stem).to_string();
            let vs = r
                .races
                .iter()
                .map(|&rc| {
                    race_names
                        .get(rc as usize)
                        .map(|n| n.split_whitespace().next().unwrap_or("?").to_uppercase())
                        .unwrap_or_else(|| "?".into())
                })
                .collect::<Vec<_>>()
                .join(" VS ");
            let mins = r.duration_ticks / (24 * 60);
            let secs = (r.duration_ticks / 24) % 60;
            Some((format!("{date}  {vs}  {mins}:{secs:02}"), p))
        })
        .collect()
}
