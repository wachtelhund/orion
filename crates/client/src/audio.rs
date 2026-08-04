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
    SelBuilding,
    // Command acknowledgments: "moving out" / "engaging" / "on it".
    AckMove1,
    AckMove2,
    AckAttack,
    AckGather,
    AckBuild,
}

pub const ALL_SFX: [Sfx; 23] = [
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
    Sfx::SelBuilding,
    Sfx::AckMove1,
    Sfx::AckMove2,
    Sfx::AckAttack,
    Sfx::AckGather,
    Sfx::AckBuild,
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
    let mut rng = Rng(0x0510CAFE);
    match s {
        Sfx::Click => render(0.05, |t, total, _| {
            square(t, 1400.0) * 0.25 * env(t, 0.002, total)
        }, &mut rng),
        Sfx::Error => render(0.28, |t, total, _| {
            let hz = if t < 0.14 { 220.0 } else { 155.0 };
            (square(t, hz) * 0.7 + square(t, hz * 1.01) * 0.3) * 0.28 * env(t, 0.005, total)
        }, &mut rng),
        Sfx::Shot => render(0.09, |t, total, r| {
            let n = r * 0.8 + sine(t, 900.0 - t * 5000.0) * 0.3;
            n * 0.35 * env(t, 0.002, total)
        }, &mut rng),
        Sfx::Cannon => render(0.45, |t, total, r| {
            let boom = sine(t, 65.0 - t * 40.0) * 0.8 + r * 0.35 * (1.0 - t * 1.5).max(0.0);
            boom * 0.55 * env(t, 0.004, total)
        }, &mut rng),
        Sfx::Explosion => render(0.5, |t, total, r| {
            (r * 0.7 + sine(t, 90.0 - t * 80.0) * 0.4) * 0.45 * env(t, 0.01, total)
        }, &mut rng),
        Sfx::BigExplosion => render(1.0, |t, total, r| {
            (r * 0.8 + sine(t, 55.0 - t * 30.0) * 0.6) * 0.55 * env(t, 0.02, total)
        }, &mut rng),
        Sfx::UnitReady => melody(&[(660.0, 0.09), (880.0, 0.14)], 0.22),
        Sfx::BuildDone => melody(&[(523.0, 0.09), (659.0, 0.09), (784.0, 0.16)], 0.22),
        Sfx::ResearchDone => melody(
            &[(523.0, 0.09), (659.0, 0.09), (784.0, 0.09), (1046.0, 0.2)],
            0.2,
        ),
        Sfx::Storm => render(0.7, |t, total, r| {
            let crackle = if r.abs() > 0.75 { r } else { r * 0.15 };
            (crackle * 0.6 + sine(t, 180.0 + r * 60.0) * 0.15) * 0.4 * env(t, 0.01, total)
        }, &mut rng),
        Sfx::Alarm => render(0.5, |t, total, _| {
            let hz = if (t * 8.0) as i32 % 2 == 0 { 660.0 } else { 495.0 };
            square(t, hz) * 0.2 * env(t, 0.01, total)
        }, &mut rng),
        // VC selection: crisp two-tone radio blips with a click of static.
        Sfx::SelVc1 => melody(&[(740.0, 0.05), (988.0, 0.07)], 0.16),
        Sfx::SelVc2 => melody(&[(660.0, 0.05), (880.0, 0.05), (988.0, 0.05)], 0.15),
        Sfx::SelVcWorker => melody(&[(523.0, 0.06), (659.0, 0.08)], 0.13),
        // Kyth selection: warbling organic chirps.
        Sfx::SelKyth1 => render(0.16, |t, total, _| {
            let hz = 620.0 + (t * 34.0).sin() * 160.0 + t * 500.0;
            (sine(t, hz) * 0.7 + sine(t, hz * 1.99) * 0.3) * 0.3 * env(t, 0.01, total)
        }, &mut rng),
        Sfx::SelKyth2 => render(0.18, |t, total, _| {
            let hz = 840.0 - t * 900.0 + (t * 55.0).sin() * 90.0;
            sine(t, hz) * 0.32 * env(t, 0.012, total)
        }, &mut rng),
        // Building select: low mechanical thunk + hum.
        Sfx::SelBuilding => render(0.14, |t, total, _| {
            (sine(t, 180.0) * 0.5 + square(t, 90.0) * 0.12 + sine(t, 361.0) * 0.2)
                * 0.4
                * env(t, 0.004, total)
        }, &mut rng),
        // Command acks.
        Sfx::AckMove1 => melody(&[(587.0, 0.05), (784.0, 0.06)], 0.14),
        Sfx::AckMove2 => melody(&[(659.0, 0.04), (740.0, 0.04), (880.0, 0.05)], 0.14),
        Sfx::AckAttack => render(0.17, |t, total, _| {
            let hz = if t < 0.06 { 392.0 } else { 587.0 };
            (square(t, hz) * 0.5 + saw(t, hz * 0.5) * 0.3) * 0.26 * env(t, 0.004, total)
        }, &mut rng),
        Sfx::AckGather => melody(&[(784.0, 0.04), (659.0, 0.06)], 0.13),
        Sfx::AckBuild => render(0.13, |t, total, _| {
            // Quick ratchet: three ticks rising.
            let k = (t * 24.0) as i32;
            let hz = 500.0 + k as f32 * 160.0;
            square(t, hz) * 0.22 * env(t, 0.003, total) * if t * 24.0 % 1.0 < 0.6 { 1.0 } else { 0.2 }
        }, &mut rng),
        Sfx::Ping => render(0.08, |t, total, _| {
            sine(t, 1200.0) * 0.18 * env(t, 0.002, total)
        }, &mut rng),
    }
}

fn render(dur: f32, f: impl Fn(f32, f32, f32) -> f32, rng: &mut Rng) -> Vec<f32> {
    (0..secs(dur))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let r = rng.next();
            f(t, dur, r).clamp(-1.0, 1.0)
        })
        .collect()
}

fn melody(notes: &[(f32, f32)], vol: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &(hz, dur) in notes {
        for i in 0..secs(dur) {
            let t = i as f32 / RATE as f32;
            let v = (sine(t, hz) * 0.7 + sine(t, hz * 2.0) * 0.2) * vol * env(t, 0.01, dur);
            out.push(v);
        }
    }
    out
}

/// ~51s ambient loop: slow minor-key pads with a deep bass pulse and an
/// occasional sparkle. Sci-fi desert vibes, quiet enough to sit under combat.
fn synth_music() -> Vec<f32> {
    // A minor-ish progression: Am, F, C, G — as root frequencies.
    const CHORDS: [[f32; 3]; 4] = [
        [220.0, 261.63, 329.63], // A C E
        [174.61, 220.0, 261.63], // F A C
        [130.81, 164.81, 196.0], // C E G
        [196.0, 246.94, 293.66], // G B D
    ];
    let bar = 12.8f32; // seconds per chord
    let total = bar * CHORDS.len() as f32;
    let n = secs(total);
    let mut out = vec![0.0f32; n];
    let mut lp = 0.0f32; // one-pole lowpass state
    let mut rng = Rng(0xBADA55);

    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let bar_i = ((t / bar) as usize) % CHORDS.len();
        let bt = t % bar;
        let chord = &CHORDS[bar_i];
        // Crossfade at bar edges.
        let fade = (bt / 1.5).min(1.0).min(((bar - bt) / 1.5).min(1.0));

        let mut v = 0.0;
        for (k, &hz) in chord.iter().enumerate() {
            let detune = 1.0 + 0.0015 * (k as f32 - 1.0);
            let slow = 0.5 + 0.5 * sine(t, 0.07 + 0.013 * k as f32);
            v += (saw(t, hz * 0.5 * detune) * 0.16 + sine(t, hz * 0.5) * 0.22) * slow;
        }
        // Bass pulse on the root, twice a bar.
        let pulse_t = bt % (bar / 2.0);
        v += sine(t, chord[0] * 0.25) * 0.3 * (1.0 - pulse_t / 3.0).max(0.0);
        // Sparkle: rare soft high blip.
        let r = rng.next();
        if r > 0.99993 {
            v += 0.0; // keep deterministic length; sparkles via LFO below
        }
        v += sine(t, chord[2] * 4.0) * 0.02 * (0.5 + 0.5 * sine(t, 0.031)).powi(4);

        // Lowpass to soften everything.
        lp += (v - lp) * 0.08;
        out[i] = (lp * fade * 0.5).clamp(-1.0, 1.0);
    }
    out
}
