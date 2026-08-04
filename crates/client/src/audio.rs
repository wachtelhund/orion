//! Procedural audio: every sound and the music loop are synthesized at
//! startup — no asset files, matching the procedural sprite atlas. Playback
//! via rodio. Audio failing to initialize (no output device) is never fatal.

use std::sync::Arc;

use rodio::buffer::SamplesBuffer;
use rodio::source::Source;
use rodio::{OutputStream, OutputStreamHandle, Sink};

const RATE: u32 = 44_100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    Click,
    Error,
    Shot,
    Cannon,
    Explosion,
    BigExplosion,
    UnitReady,
    BuildDone,
    ResearchDone,
    Storm,
    Alarm,
    Ping,
    // Selection acknowledgments: "yes commander?" — radio blips for the
    // Vanguard Combine, organic chirps for the Kyth. Two variations each
    // so spam-clicking doesn't grate.
    SelVc1,
    SelVc2,
    SelVcWorker,
    SelKyth1,
    SelKyth2,
    /// Ferron Compact: metallic FM clangs.
    SelFer1,
    SelFer2,
    SelBuilding,
    // Command acknowledgments: "moving out" / "engaging" / "on it".
    AckMove1,
    AckMove2,
    AckAttack,
    AckGather,
    AckBuild,
    /// Weapon-flavor fire sounds: acid thwip, electric crack, melee
    /// whoosh, rail twang.
    Spit,
    Zap,
    Slash,
    Rail,
    /// Match-start countdown tick and the GO chord.
    CountTick,
    CountGo,
}

pub const ALL_SFX: [Sfx; 31] = [
    Sfx::Click,
    Sfx::Error,
    Sfx::Shot,
    Sfx::Cannon,
    Sfx::Explosion,
    Sfx::BigExplosion,
    Sfx::UnitReady,
    Sfx::BuildDone,
    Sfx::ResearchDone,
    Sfx::Storm,
    Sfx::Alarm,
    Sfx::Ping,
    Sfx::SelVc1,
    Sfx::SelVc2,
    Sfx::SelVcWorker,
    Sfx::SelKyth1,
    Sfx::SelKyth2,
    Sfx::SelFer1,
    Sfx::SelFer2,
    Sfx::SelBuilding,
    Sfx::AckMove1,
    Sfx::AckMove2,
    Sfx::AckAttack,
    Sfx::AckGather,
    Sfx::AckBuild,
    Sfx::Spit,
    Sfx::Zap,
    Sfx::Slash,
    Sfx::Rail,
    Sfx::CountTick,
    Sfx::CountGo,
];

pub struct Audio {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    music: Sink,
    buffers: Vec<Arc<Vec<f32>>>,
    pub sfx_volume: f32,
}

impl Audio {
    pub fn new(music_volume: f32, sfx_volume: f32) -> Option<Audio> {
        let (stream, handle) = OutputStream::try_default().ok()?;
        let music = Sink::try_new(&handle).ok()?;
        music.set_volume(music_volume);
        music.append(SamplesBuffer::new(1, RATE, synth_music()).repeat_infinite());

        let buffers = ALL_SFX.iter().map(|s| Arc::new(synth_sfx(*s))).collect();
        Some(Audio { _stream: stream, handle, music, buffers, sfx_volume })
    }

    pub fn set_music_volume(&self, v: f32) {
        self.music.set_volume(v);
    }

    pub fn play(&self, s: Sfx) {
        self.play_vol(s, 1.0);
    }

    pub fn play_vol(&self, s: Sfx, scale: f32) {
        let idx = ALL_SFX.iter().position(|x| *x == s).unwrap();
        let buf = self.buffers[idx].as_ref().clone();
        let vol = self.sfx_volume * scale;
        if vol <= 0.01 {
            return;
        }
        let _ = self
            .handle
            .play_raw(SamplesBuffer::new(1, RATE, buf).amplify(vol));
    }
}

// ------------------------------------------------------------- synthesis ----

fn secs(s: f32) -> usize {
    (s * RATE as f32) as usize
}

/// Deterministic noise (same soundscape every run).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) - 1.0
    }
}

fn env(t: f32, attack: f32, total: f32) -> f32 {
    if t < attack {
        t / attack
    } else {
        let d = (t - attack) / (total - attack).max(1e-4);
        (1.0 - d).max(0.0).powi(2)
    }
}

fn sine(t: f32, hz: f32) -> f32 {
    (t * hz * std::f32::consts::TAU).sin()
}

fn square(t: f32, hz: f32) -> f32 {
    if sine(t, hz) >= 0.0 { 1.0 } else { -1.0 }
}

fn saw(t: f32, hz: f32) -> f32 {
    2.0 * (t * hz).fract() - 1.0
}

fn synth_sfx(s: Sfx) -> Vec<f32> {
    match s {
        Sfx::Click => render(0.045, |t, total, _| {
            (sine(t, 1900.0) * 0.5 + sine(t, 950.0) * 0.3) * 0.22 * env(t, 0.002, total)
        }),
        Sfx::Error => render(0.28, |t, total, _| {
            let hz = if t < 0.14 { 220.0 } else { 155.0 };
            (square(t, hz) * 0.55 + square(t, hz * 1.01) * 0.25) * 0.26 * env(t, 0.005, total)
        }),
        // Shot: dry CRACK — noise transient, descending zap, tight tail.
        Sfx::Shot => layered(0.11, |t, total, r, lp| {
            let crack = r * (1.0 - t * 40.0).max(0.0) * 1.2;
            let zap = sine(t, 1500.0 - t * 9000.0) * 0.4 * (1.0 - t * 12.0).max(0.0);
            let tail = lp.run(r * 0.5, 0.12) * (1.0 - t * 9.0).max(0.0);
            (crack + zap + tail) * 0.4 * env(t, 0.001, total)
        }),
        // Cannon: metal clang into a deep boom with a closing-filter tail.
        Sfx::Cannon => layered(0.62, |t, total, r, lp| {
            let clang = (sine(t, 420.0) + sine(t, 420.0 * 2.76) * 0.5 + sine(t, 420.0 * 5.4) * 0.25)
                * (1.0 - t * 7.0).max(0.0)
                * 0.35;
            let boom = sine(t, 62.0 - t * 34.0) * 0.85 * (1.0 - t * 1.6).max(0.0);
            let cutoff = (0.35 * (1.0 - t * 1.8)).max(0.02);
            let tail = lp.run(r, cutoff) * 0.7 * (1.0 - t * 1.5).max(0.0);
            (clang + boom + tail) * 0.6 * env(t, 0.002, total)
        }),
        // Explosion: sub thump + noise washed through a closing lowpass.
        Sfx::Explosion => layered(0.65, |t, total, r, lp| {
            let sub = sine(t, 78.0 - t * 55.0) * 0.7 * (1.0 - t * 1.9).max(0.0);
            let cutoff = (0.4 * (1.0 - t * 1.4)).max(0.025);
            let wash = lp.run(r, cutoff) * 1.0 * (1.0 - t * 1.4).max(0.0);
            let crackle = if r.abs() > 0.93 { r * (1.0 - t).max(0.0) * 0.5 } else { 0.0 };
            (sub + wash + crackle) * 0.55 * env(t, 0.004, total)
        }),
        Sfx::BigExplosion => layered(1.35, |t, total, r, lp| {
            let sub = sine(t, 52.0 - t * 20.0) * 0.85 * (1.0 - t * 0.8).max(0.0);
            let rumble = 1.0 + 0.35 * sine(t, 9.0);
            let cutoff = (0.35 * (1.0 - t * 0.7)).max(0.015);
            let wash = lp.run(r, cutoff) * rumble * (1.0 - t * 0.75).max(0.0);
            let debris = if t > 0.35 && r.abs() > 0.96 { r * (1.3 - t).max(0.0) * 0.6 } else { 0.0 };
            (sub + wash + debris) * 0.6 * env(t, 0.008, total)
        }),
        Sfx::UnitReady => chime(&[(660.0, 0.09), (880.0, 0.16)], 0.2),
        Sfx::BuildDone => chime(&[(523.0, 0.09), (659.0, 0.09), (784.0, 0.18)], 0.2),
        Sfx::ResearchDone => {
            chime(&[(523.0, 0.09), (659.0, 0.09), (784.0, 0.09), (1046.0, 0.22)], 0.18)
        }
        // Storm: arc bursts over a rising charged hum.
        Sfx::Storm => layered(0.75, |t, total, r, lp| {
            let gate = if (t * 31.0).fract() < 0.4 { 1.0 } else { 0.25 };
            let arc = if r.abs() > 0.62 { r * gate } else { r * 0.1 };
            let hum = sine(t, 130.0 + t * 160.0) * 0.18;
            let sizzle = lp.run(arc, 0.6) * 0.8;
            (sizzle + hum) * 0.42 * env(t, 0.01, total)
        }),
        Sfx::Alarm => render(0.5, |t, total, _| {
            let hz = if (t * 8.0) as i32 % 2 == 0 { 660.0 } else { 495.0 };
            (sine(t, hz) * 0.5 + square(t, hz) * 0.12) * 0.26 * env(t, 0.01, total)
        }),
        // VC selection: radio squelch opens, two-tone blip, squelch closes.
        Sfx::SelVc1 => radio(&[(740.0, 0.05), (988.0, 0.08)]),
        Sfx::SelVc2 => radio(&[(660.0, 0.045), (880.0, 0.045), (988.0, 0.06)]),
        Sfx::SelVcWorker => radio(&[(523.0, 0.06), (659.0, 0.08)]),
        // Kyth selection: formant warbles — a living throat, not a beep.
        Sfx::SelKyth1 => formant(0.2, 340.0, 90.0, 11.0),
        Sfx::SelKyth2 => formant(0.24, 260.0, -70.0, 7.5),
        // Ferron selection: struck-metal FM ring.
        Sfx::SelFer1 => metal(0.3, 392.0),
        Sfx::SelFer2 => metal(0.26, 294.0),
        Sfx::SelBuilding => layered(0.16, |t, total, _r, lp| {
            let thunk = sine(t, 170.0) * 0.5 * (1.0 - t * 9.0).max(0.0);
            let servo = lp.run(saw(t, 900.0 - t * 2400.0), 0.25) * 0.18 * (1.0 - t * 5.0).max(0.0);
            let hum = sine(t, 356.0) * 0.14;
            (thunk + servo + hum) * 0.45 * env(t, 0.004, total)
        }),
        Sfx::AckMove1 => radio(&[(587.0, 0.05), (784.0, 0.07)]),
        Sfx::AckMove2 => radio(&[(659.0, 0.04), (740.0, 0.04), (880.0, 0.06)]),
        // Attack ack: clipped aggressive stab.
        Sfx::AckAttack => layered(0.18, |t, total, r, _lp| {
            let stab = if t < 0.05 { r * 0.5 } else { 0.0 };
            let hz = if t < 0.07 { 392.0 } else { 587.0 };
            let tone = (square(t, hz) * 0.45 + saw(t, hz * 0.5) * 0.3).clamp(-0.6, 0.6);
            (stab + tone) * 0.3 * env(t, 0.003, total)
        }),
        Sfx::AckGather => radio(&[(784.0, 0.04), (659.0, 0.06)]),
        Sfx::AckBuild => render(0.13, |t, total, _| {
            let k = (t * 24.0) as i32;
            let hz = 500.0 + k as f32 * 160.0;
            square(t, hz)
                * 0.2
                * env(t, 0.003, total)
                * if (t * 24.0).fract() < 0.6 { 1.0 } else { 0.2 }
        }),
        // Acid spit: wet formant thwip with a splat tail.
        Sfx::Spit => layered(0.16, |t, total, r, lp| {
            let body = sine(t, 300.0 - t * 900.0) * 0.5 * (1.0 - t * 5.0).max(0.0);
            let wet = lp.run(r, 0.3) * 0.4 * (1.0 - t * 4.0).max(0.0);
            (body + wet) * 0.4 * env(t, 0.004, total)
        }),
        // Electric crack.
        Sfx::Zap => layered(0.12, |t, total, r, lp| {
            let crack = r * (1.0 - t * 22.0).max(0.0);
            let sizzle = lp.run(if r.abs() > 0.5 { r } else { 0.0 }, 0.7) * 0.7 * (1.0 - t * 8.0).max(0.0);
            let tone = sine(t, 2400.0 - t * 6000.0) * 0.2 * (1.0 - t * 10.0).max(0.0);
            (crack + sizzle + tone) * 0.34 * env(t, 0.001, total)
        }),
        // Melee whoosh: filtered noise sweeping shut.
        Sfx::Slash => layered(0.14, |t, total, r, lp| {
            let cut = 0.5 - t * 3.0;
            lp.run(r, cut.max(0.05)) * 0.55 * env(t, 0.015, total)
        }),
        // Rail twang: deep FM boom with a metallic overtone.
        Sfx::Rail => layered(0.4, |t, total, r, lp| {
            let boom = sine(t, 95.0 - t * 60.0 + sine(t, 30.0) * 20.0) * 0.6 * (1.0 - t * 2.2).max(0.0);
            let metal = sine(t, 640.0) * 0.25 * (1.0 - t * 5.0).max(0.0);
            let tail = lp.run(r, 0.12) * 0.3 * (1.0 - t * 2.5).max(0.0);
            (boom + metal + tail) * 0.5 * env(t, 0.002, total)
        }),
        Sfx::CountTick => render(0.12, |t, total, _| {
            (sine(t, 880.0) * 0.6 + sine(t, 1760.0) * 0.2) * 0.4 * env(t, 0.004, total)
        }),
        Sfx::CountGo => chime(&[(523.0, 0.1), (659.0, 0.1), (784.0, 0.1), (1046.0, 0.24)], 0.28),
        Sfx::Ping => render(0.08, |t, total, _| sine(t, 1200.0) * 0.18 * env(t, 0.002, total)),
    }
}

/// One-pole lowpass; coefficient per call so cutoffs can close over time.
struct Lp(f32);
impl Lp {
    fn run(&mut self, x: f32, a: f32) -> f32 {
        self.0 += (x - self.0) * a;
        self.0
    }
}

fn render(dur: f32, f: impl Fn(f32, f32, f32) -> f32) -> Vec<f32> {
    let mut rng = Rng(0x0510CAFE);
    (0..secs(dur))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let r = rng.next();
            f(t, dur, r).clamp(-1.0, 1.0)
        })
        .collect()
}

/// Render with a private lowpass — for anything with a filtered noise body.
fn layered(dur: f32, f: impl Fn(f32, f32, f32, &mut Lp) -> f32) -> Vec<f32> {
    let mut rng = Rng(0x0510CAFE);
    let mut lp = Lp(0.0);
    (0..secs(dur))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let r = rng.next();
            f(t, dur, r, &mut lp).clamp(-1.0, 1.0)
        })
        .collect()
}

/// Completion chime: notes with a soft chorused triangle-ish voice.
fn chime(notes: &[(f32, f32)], vol: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &(hz, dur) in notes {
        for i in 0..secs(dur) {
            let t = i as f32 / RATE as f32;
            let v = (sine(t, hz) * 0.6
                + sine(t, hz * 1.003) * 0.25
                + sine(t, hz * 2.0) * 0.18
                + sine(t, hz * 3.0) * 0.06)
                * vol
                * env(t, 0.012, dur);
            out.push(v);
        }
    }
    out
}

/// VC radio ack: static squelch opens, tones speak, squelch closes.
fn radio(notes: &[(f32, f32)]) -> Vec<f32> {
    let mut rng = Rng(0x0510CAFE);
    let mut out = Vec::new();
    // Squelch open: 18ms of bright static.
    for i in 0..secs(0.018) {
        let t = i as f32 / RATE as f32;
        out.push(rng.next() * 0.22 * (1.0 - t * 40.0).max(0.2));
    }
    for &(hz, dur) in notes {
        for i in 0..secs(dur) {
            let t = i as f32 / RATE as f32;
            let tone = (square(t, hz) * 0.32 + sine(t, hz) * 0.3) * env(t, 0.004, dur);
            let hiss = rng.next() * 0.03;
            out.push(tone + hiss);
        }
    }
    for i in 0..secs(0.02) {
        let t = i as f32 / RATE as f32;
        out.push(rng.next() * 0.16 * (1.0 - t * 45.0).max(0.0));
    }
    out
}

/// Kyth formant warble: a gliding voiced tone with throat resonances.
fn formant(dur: f32, f0: f32, glide: f32, vib: f32) -> Vec<f32> {
    (0..secs(dur))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let base = f0 + glide * t * 6.0 + (t * vib * std::f32::consts::TAU).sin() * 24.0;
            let voiced = sine(t, base) * 0.5
                + sine(t, base * 2.4) * 0.3 * (1.0 + (t * 17.0).sin() * 0.5)
                + sine(t, base * 3.9) * 0.12;
            (voiced * 0.34 * env(t, 0.015, dur)).clamp(-1.0, 1.0)
        })
        .collect()
}

/// Ferron struck metal: inharmonic partials ringing down.
fn metal(dur: f32, hz: f32) -> Vec<f32> {
    (0..secs(dur))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let ring = sine(t, hz) * 0.5
                + sine(t, hz * 2.76) * 0.35 * (1.0 - t * 2.0).max(0.0)
                + sine(t, hz * 5.4) * 0.2 * (1.0 - t * 4.0).max(0.0)
                + sine(t, hz * 8.9) * 0.1 * (1.0 - t * 7.0).max(0.0);
            (ring * 0.32 * env(t, 0.002, dur)).clamp(-1.0, 1.0)
        })
        .collect()
}

/// ~64s dark ambient bed: detuned root drones under wandering wind, a
/// sparse minor motif and a distant metallic knell. Dystopian, quiet,
/// sits under combat without fighting it.
fn synth_music() -> Vec<f32> {
    // Root movement: A1, F1, G1, E1 — 16s each.
    const ROOTS: [f32; 4] = [55.0, 43.65, 49.0, 41.2];
    let bar = 16.0f32;
    let total = bar * ROOTS.len() as f32;
    let n = secs(total);
    let mut out = vec![0.0f32; n];
    let mut lp = Lp(0.0);
    let mut wind_lp = Lp(0.0);
    let mut rng = Rng(0xBADA55);

    // Sparse motif: (bar_offset_seconds, semitone_ratio, length).
    const MOTIF: [(f32, f32, f32); 3] = [(6.0, 4.0, 2.2), (9.5, 4.755, 1.8), (12.0, 3.564, 2.6)];

    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let bar_i = ((t / bar) as usize) % ROOTS.len();
        let bt = t % bar;
        let root = ROOTS[bar_i];
        let fade = (bt / 2.5).min(1.0).min(((bar - bt) / 2.5).min(1.0));

        // Drone: detuned pair + dark fifth, breathing slowly.
        let breathe = 0.6 + 0.4 * sine(t, 0.043);
        let mut v = (saw(t, root) * 0.10 + saw(t, root * 1.006) * 0.10 + sine(t, root * 1.5) * 0.08)
            * breathe;
        // Sub anchor.
        v += sine(t, root * 0.5) * 0.16;
        // Wind: noise through a slowly wandering filter.
        let wind_cut = 0.015 + 0.012 * (1.0 + sine(t, 0.05));
        v += wind_lp.run(rng.next(), wind_cut) * 0.5;
        // Motif notes above the drone.
        for &(off, ratio, len) in MOTIF.iter() {
            let nt = bt - off;
            if nt > 0.0 && nt < len {
                let hz = root * 4.0 * ratio / 4.0;
                v += (sine(nt, hz) * 0.6 + sine(nt, hz * 2.0) * 0.15)
                    * 0.10
                    * env(nt, 0.4, len);
            }
        }
        // Distant knell once a bar.
        let kt = bt - 14.0;
        if kt > 0.0 && kt < 1.6 {
            let khz = root * 6.0;
            v += (sine(kt, khz) * 0.5 + sine(kt, khz * 2.76) * 0.3)
                * 0.05
                * (1.0 - kt / 1.6).powi(2);
        }

        let filtered = lp.run(v, 0.06);
        out[i] = (filtered * fade * 0.55).clamp(-1.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sfx_render_clean() {
        for s in ALL_SFX {
            let buf = synth_sfx(s);
            assert!(!buf.is_empty(), "{s:?} rendered empty");
            assert!(
                buf.iter().all(|v| v.is_finite() && v.abs() <= 1.0),
                "{s:?} has out-of-range samples"
            );
            let peak = buf.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.05, "{s:?} is near-silent (peak {peak})");
        }
    }

    #[test]
    fn music_loop_renders_clean() {
        let m = synth_music();
        assert!(m.len() > RATE as usize * 60, "music loop under a minute");
        assert!(m.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    }
}
