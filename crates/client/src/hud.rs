//! The in-game console (bottom bar), top status readout, and end banner.
//! Every metric scales with App::ui() — display DPI x the player's HUD-size
//! setting.

use orion_sim::data::DefId;
use orion_sim::map::{TileKind, TilePos};
use orion_sim::EntityKind;

use crate::app::{hp_color, App, EffKind, Mode, GAS_COLOR, MINERAL_COLOR, TEAM_COLORS, WHITE};
use crate::gfx::Inst;

/// A clickable command-card button.
pub(crate) enum CardAction {
    RunAttack,
    RunStop,
    RunHold,
    OpenBuild,
    Train(usize),
    Place(DefId),
    Research(u8),
    SiegeBtn,
    StormBtn,
    CancelMode,
    CancelConstructionBtn,
}

/// What to draw inside a card button.
pub(crate) enum CardIcon {
    Building(usize),
    Unit(usize),
    Letter,
}

pub(crate) struct CardButton {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub key: String,
    pub hint: String,
    pub icon: CardIcon,
    pub action: CardAction,
    /// Tooltip lines (text, color); first line is the title.
    pub tip: Vec<(String, [f32; 4])>,
}

/// Compact research label for card buttons ("W+1", "A+2").
fn short_research(tag: &str) -> String {
    let (kind, lvl) = tag.split_at(tag.len() - 1);
    let k = if kind.starts_with("weapons") { "W" } else { "A" };
    format!("{k}+{lvl}")
}

const TIP_TITLE: [f32; 4] = [0.95, 0.95, 0.9, 1.0];
const TIP_COST: [f32; 4] = [0.65, 0.85, 1.0, 1.0];
const TIP_STAT: [f32; 4] = [0.75, 0.75, 0.72, 1.0];
const TIP_DESC: [f32; 4] = [0.6, 0.62, 0.66, 1.0];
const TIP_WARN: [f32; 4] = [1.0, 0.6, 0.3, 1.0];

impl App {
    pub(crate) fn console_h(&self) -> f32 {
        118.0 * self.ui()
    }

    // -------------------------------------------------------- tooltips ----

    fn cost_line(m: u32, g: u32, supply: u32, ticks: u32) -> String {
        let mut s = format!("{m} MINERALS");
        if g > 0 {
            s += &format!("  {g} PLASMA");
        }
        if supply > 0 {
            s += &format!("  {supply} SUPPLY");
        }
        s += &format!("  {}S", ticks / orion_sim::TICKS_PER_SEC);
        s
    }

    pub(crate) fn tip_unit(&self, def: DefId) -> Vec<(String, [f32; 4])> {
        let d = &self.state.data.units[def as usize];
        let mut t = vec![
            (d.name.to_uppercase(), TIP_TITLE),
            (Self::cost_line(d.cost_minerals, d.cost_gas, d.supply, d.build_ticks), TIP_COST),
        ];
        let mut stats = format!("HP {}", d.hp);
        if let Some(w) = &d.weapon {
            stats += &format!("  DMG {}  RNG {:.1}", w.damage, w.range.to_f32());
            if w.air {
                stats += "  ANTI-AIR";
            }
        }
        if d.energy_max > 0 {
            stats += &format!("  ENERGY {}", d.energy_max);
        }
        t.push((stats, TIP_STAT));
        if let Some(w) = &d.weapon_siege {
            t.push((
                format!("SIEGED: DMG {}  RNG {:.1}  SPLASH", w.damage, w.range.to_f32()),
                TIP_STAT,
            ));
        }
        if !d.desc.is_empty() {
            t.push((d.desc.to_uppercase(), TIP_DESC));
        }
        if let Some(req) = d.requires {
            if !self.state.requirement_met(self.human, Some(req)) {
                t.push((
                    format!("REQUIRES {}", self.state.data.buildings[req as usize].name)
                        .to_uppercase(),
                    TIP_WARN,
                ));
            }
        }
        t
    }

    pub(crate) fn tip_building(&self, def: DefId) -> Vec<(String, [f32; 4])> {
        let d = &self.state.data.buildings[def as usize];
        let mut t = vec![
            (d.name.to_uppercase(), TIP_TITLE),
            (Self::cost_line(d.cost_minerals, d.cost_gas, 0, d.build_ticks), TIP_COST),
        ];
        let mut stats = format!("HP {}", d.hp);
        if d.supply_provided > 0 {
            stats += &format!("  +{} SUPPLY", d.supply_provided);
        }
        t.push((stats, TIP_STAT));
        if !d.desc.is_empty() {
            t.push((d.desc.to_uppercase(), TIP_DESC));
        }
        if let Some(req) = d.requires {
            if !self.state.requirement_met(self.human, Some(req)) {
                t.push((
                    format!("REQUIRES {}", self.state.data.buildings[req as usize].name)
                        .to_uppercase(),
                    TIP_WARN,
                ));
            }
        }
        t
    }

    fn tip_research(&self, r: u8) -> Vec<(String, [f32; 4])> {
        let d = &self.state.data.research[r as usize];
        let mut t = vec![
            (d.name.clone(), TIP_TITLE),
            (Self::cost_line(d.cost_minerals, d.cost_gas, 0, d.ticks), TIP_COST),
        ];
        if !d.desc.is_empty() {
            t.push((d.desc.to_uppercase(), TIP_DESC));
        }
        if self.state.players[self.human as usize].research_done[r as usize] {
            t.push(("ALREADY RESEARCHED".into(), TIP_WARN));
        } else if let Some(pre) = d.requires {
            if !self.state.players[self.human as usize].research_done[pre as usize] {
                t.push((
                    format!("REQUIRES {}", self.state.data.research[pre as usize].name),
                    TIP_WARN,
                ));
            }
        }
        t
    }

    fn tip_action(title: &str, desc: &str) -> Vec<(String, [f32; 4])> {
        vec![(title.into(), TIP_TITLE), (desc.into(), TIP_DESC)]
    }

    /// The tooltip for whatever bottom-HUD element the cursor is over.
    fn hovered_tooltip(&self) -> Option<Vec<(String, [f32; 4])>> {
        let (mx, my) = self.mouse;
        // Command card buttons.
        for b in self.card_buttons() {
            if mx >= b.x && mx <= b.x + b.w && my >= b.y && my <= b.y + b.h {
                if b.tip.is_empty() {
                    return None;
                }
                return Some(b.tip);
            }
        }
        // Production queue slots.
        if let Some(bid) = self.single_own_building() {
            let b = &self.state.entities[bid.idx as usize];
            for (x, y, w, h, slot) in self.queue_slot_rects() {
                if mx >= x && mx <= x + w && my >= y && my <= y + h {
                    let unit = b.queue[slot];
                    let mut t = self.tip_unit(unit);
                    t.push(("CLICK TO CANCEL - FULL REFUND".into(), TIP_WARN));
                    return Some(t);
                }
            }
        }
        // Multi-select strip tiles.
        for (x, y, w, h, id) in self.strip_tiles() {
            if mx >= x && mx <= x + w && my >= y && my <= y + h {
                let e = self.state.get(id)?;
                let mut t = match e.kind {
                    EntityKind::Unit => self.tip_unit(e.def),
                    EntityKind::Building => self.tip_building(e.def),
                    EntityKind::Resource => return None,
                };
                t.push(("CLICK ICONS / TAB TO CYCLE SUBGROUPS".into(), TIP_DESC));
                return Some(t);
            }
        }
        None
    }

    /// Layout of the multi-select strip — shared by draw and tooltips.
    pub(crate) fn strip_tiles(&self) -> Vec<(f32, f32, f32, f32, orion_sim::EntityId)> {
        if self.selection.len() <= 1 {
            return Vec::new();
        }
        let ui = self.ui();
        let (mx, _, size) = self.minimap_rect();
        let x0 = mx + size + 24.0 * ui;
        let tx = x0 + 84.0 * ui + 16.0 * ui;
        let sx = tx + 240.0 * ui;
        let sy = self.cam.screen_h - self.console_h() + 14.0 * ui;
        let mut out = Vec::new();
        let mut k = 0;
        for id in self.selection.iter().take(18) {
            let Some(u) = self.state.get(*id) else { continue };
            if u.owner != self.human || u.kind == EntityKind::Resource {
                continue;
            }
            let col = k % 9;
            let row = k / 9;
            out.push((
                sx + col as f32 * 34.0 * ui - 2.0,
                sy + row as f32 * 46.0 * ui - 2.0,
                32.0 * ui,
                42.0 * ui,
                *id,
            ));
            k += 1;
        }
        out
    }

    fn draw_tooltip(&self, out: &mut Vec<Inst>) {
        let Some(lines) = self.hovered_tooltip() else { return };
        let ui = self.ui();
        let ts_title = self.ts(1.6);
        let ts_body = self.ts(1.1);
        let pad = 8.0 * ui;
        let line_h = 13.0 * ui;
        let mut w: f32 = 120.0 * ui;
        for (k, (line, _)) in lines.iter().enumerate() {
            let s = if k == 0 { ts_title } else { ts_body };
            w = w.max(self.gfx.text_width(s, line) + pad * 2.0);
        }
        let h = pad * 2.0 + ts_title * 8.0 + (lines.len() as f32 - 1.0) * line_h;
        let x = (self.mouse.0 - w * 0.5)
            .clamp(8.0, (self.cam.screen_w - w - 8.0).max(8.0));
        let y = self.cam.screen_h - self.console_h() - h - 10.0 * ui;
        self.gfx.quad(out, x - 2.0, y - 2.0, w + 4.0, h + 4.0, [0.35, 0.38, 0.45, 0.95]);
        self.gfx.quad(out, x, y, w, h, [0.05, 0.05, 0.08, 0.96]);
        let mut ly = y + pad;
        for (k, (line, color)) in lines.iter().enumerate() {
            let s = if k == 0 { ts_title } else { ts_body };
            self.gfx.text(out, x + pad, ly, s, *color, line);
            ly += if k == 0 { ts_title * 8.0 } else { line_h };
        }
    }

    // ------------------------------------------------------------ draw ----

    pub(crate) fn draw_hud(&self, out: &mut Vec<Inst>) {
        if !self.in_game {
            return;
        }
        self.draw_console(out);
        self.draw_top_status(out);
        self.draw_banner(out);
    }

    fn draw_console(&self, out: &mut Vec<Inst>) {
        let ui = self.ui();
        let w = self.cam.screen_w;
        let h = self.cam.screen_h;
        let ch = self.console_h();
        let cy = h - ch;
        let steel = [0.13, 0.14, 0.17, 1.0];
        let steel_dark = [0.09, 0.095, 0.12, 1.0];
        let edge_lit = [0.30, 0.33, 0.40, 1.0];

        self.gfx.quad(out, 0.0, cy, w, ch, steel);
        self.gfx.quad(out, 0.0, cy, w, 2.0 * ui, edge_lit);
        self.gfx.quad(out, 0.0, cy + 2.0 * ui, w, 1.0 * ui, [0.05, 0.05, 0.07, 1.0]);
        let mut rx = 16.0 * ui;
        while rx < w {
            self.gfx.quad(out, rx, cy + 6.0 * ui, 2.0 * ui, 2.0 * ui, edge_lit);
            self.gfx.quad(out, rx, h - 8.0 * ui, 2.0 * ui, 2.0 * ui, steel_dark);
            rx += 48.0 * ui;
        }

        self.draw_minimap(out);
        self.draw_info_panel(out);
        self.draw_command_card(out);
        self.draw_mode_hint(out);
        self.draw_tooltip(out);
        self.draw_error_flash(out);
    }

    /// Flashing mid-screen warning: "NOT ENOUGH MINERALS" etc.
    fn draw_error_flash(&self, out: &mut Vec<Inst>) {
        let Some((msg, t0)) = &self.error_flash else { return };
        let age = t0.elapsed().as_secs_f32();
        if age > 1.8 {
            return;
        }
        // Flash by blinking, fade out at the end.
        let blink = ((age * 7.0) as i32 % 2) == 0;
        let fade = (1.0 - (age - 1.2).max(0.0) / 0.6).clamp(0.0, 1.0);
        if !blink && age < 1.2 {
            return;
        }
        let ts = self.ts(2.4);
        let w = self.cam.screen_w;
        let tw = self.gfx.text_width(ts, msg);
        let x = (w - tw) * 0.5;
        let y = self.cam.screen_h * 0.30;
        self.gfx.quad(
            out,
            x - 14.0,
            y - 8.0,
            tw + 28.0,
            ts * 8.0 + 14.0,
            [0.05, 0.02, 0.02, 0.75 * fade],
        );
        self.gfx.text(out, x, y, ts, [1.0, 0.45, 0.3, fade], msg);
    }

    pub(crate) fn minimap_rect(&self) -> (f32, f32, f32) {
        let ui = self.ui();
        let size = self.console_h() - 20.0 * ui;
        (10.0 * ui, self.cam.screen_h - self.console_h() + 12.0 * ui, size)
    }

    pub(crate) fn minimap_pick(&self, sx: f32, sy: f32) -> Option<(f32, f32)> {
        if !self.in_game {
            return None;
        }
        let (mx, my, size) = self.minimap_rect();
        if sx < mx || sx > mx + size || sy < my || sy > my + size {
            return None;
        }
        let map = &self.state.map;
        Some((
            (sx - mx) / size * map.width as f32,
            (sy - my) / size * map.height as f32,
        ))
    }

    fn draw_minimap(&self, out: &mut Vec<Inst>) {
        let (mx, my, size) = self.minimap_rect();
        let map = &self.state.map;
        let scale = size / map.width as f32;
        self.gfx.quad(out, mx - 2.0, my - 2.0, size + 4.0, size + 4.0, [0.32, 0.34, 0.4, 1.0]);
        self.gfx.quad(out, mx, my, size, size, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..map.height {
            for x in 0..map.width {
                let t = TilePos::new(x, y);
                let c = match map.kind_at(x, y) {
                    TileKind::Blocked => [0.15, 0.14, 0.15],
                    TileKind::Ramp => [0.42, 0.37, 0.28],
                    TileKind::Ground => {
                        if map.elev_at(x, y) > 0 {
                            [0.45, 0.41, 0.33]
                        } else {
                            [0.33, 0.30, 0.25]
                        }
                    }
                };
                let dim = self.fog_tint(t);
                self.gfx.quad(
                    out,
                    mx + x as f32 * scale,
                    my + y as f32 * scale,
                    scale + 0.5,
                    scale + 0.5,
                    [c[0] * dim, c[1] * dim, c[2] * dim, 1.0],
                );
            }
        }
        for i in 0..self.state.entities.len() {
            let e = &self.state.entities[i];
            if !e.alive {
                continue;
            }
            let t = TilePos::of(e.pos);
            let (visible, color) = match e.kind {
                EntityKind::Resource => (true, MINERAL_COLOR),
                _ => {
                    let vis = e.owner == self.human || self.visible(t);
                    (vis, TEAM_COLORS[e.owner as usize % 2])
                }
            };
            if !visible {
                continue;
            }
            let s = if e.kind == EntityKind::Building { 3.0 } else { 2.0 };
            self.gfx.quad(
                out,
                mx + e.pos.x.to_f32() * scale - s * 0.5,
                my + e.pos.y.to_f32() * scale - s * 0.5,
                s,
                s,
                [color[0], color[1], color[2], 1.0],
            );
        }
        // Under-attack alerts: flashing red rings.
        for e in &self.effects {
            if e.kind != EffKind::MapPing {
                continue;
            }
            let f = e.age / e.ttl;
            let flash = ((e.age * 8.0) as i32 % 2) == 0;
            if flash {
                let s = 8.0 + f * 6.0;
                self.gfx.sprite(
                    out,
                    self.gfx.book.ring,
                    mx + e.ax * scale,
                    my + e.ay * scale,
                    s,
                    s,
                    [1.0, 0.25, 0.2, 0.9],
                );
            }
        }
        // Camera viewport.
        let corners = [
            self.cam.screen_to_world(0.0, 0.0),
            self.cam.screen_to_world(self.cam.screen_w, 0.0),
            self.cam
                .screen_to_world(self.cam.screen_w, self.cam.screen_h - self.console_h()),
            self.cam.screen_to_world(0.0, self.cam.screen_h - self.console_h()),
        ];
        let c = [1.0, 1.0, 1.0, 0.7];
        for k in 0..4 {
            let (ax, ay) = corners[k];
            let (bx, by) = corners[(k + 1) % 4];
            let (sx0, sy0) = (
                (mx + ax * scale).clamp(mx, mx + size),
                (my + ay * scale).clamp(my, my + size),
            );
            let (sx1, sy1) = (
                (mx + bx * scale).clamp(mx, mx + size),
                (my + by * scale).clamp(my, my + size),
            );
            self.gfx.beam(out, sx0, sy0, sx1, sy1, 1.0, c);
        }
    }

    /// Production queue slot rects for the selected building (hit-testing +
    /// drawing share this).
    pub(crate) fn queue_slot_rects(&self) -> Vec<(f32, f32, f32, f32, usize)> {
        let Some(bid) = self.single_own_building() else { return Vec::new() };
        let b = &self.state.entities[bid.idx as usize];
        if b.queue.is_empty() {
            return Vec::new();
        }
        let ui = self.ui();
        let (mx, _, size) = self.minimap_rect();
        let x0 = mx + size + 24.0 * ui + 84.0 * ui + 16.0 * ui;
        let y = self.cam.screen_h - self.console_h() + 66.0 * ui;
        (0..b.queue.len())
            .map(|k| (x0 + k as f32 * 36.0 * ui, y, 32.0 * ui, 38.0 * ui, k))
            .collect()
    }

    fn draw_info_panel(&self, out: &mut Vec<Inst>) {
        let ui = self.ui();
        let h = self.cam.screen_h;
        let cy = h - self.console_h();
        let (mx, _, size) = self.minimap_rect();
        let x0 = mx + size + 24.0 * ui;
        let white = [0.92, 0.92, 0.88, 1.0];
        let dim = [0.62, 0.62, 0.6, 1.0];
        let book = &self.gfx.book;

        let Some(first) = self.active_entity() else {
            self.gfx.text(out, x0, cy + 16.0 * ui, self.ts(2.0), dim, "NO SELECTION");
            self.gfx.text(
                out,
                x0,
                cy + 40.0 * ui,
                self.ts(1.0),
                dim,
                "DRAG: SELECT   RIGHT CLICK: ORDER   CTRL+CLICK: SELECT TYPE   TAB: SUBGROUP",
            );
            return;
        };
        let Some(e) = self.state.get(first) else { return };

        // Portrait.
        let pw = 84.0 * ui;
        self.gfx.quad(out, x0, cy + 12.0 * ui, pw, pw + 8.0 * ui, [0.05, 0.05, 0.08, 1.0]);
        self.gfx.quad(out, x0, cy + 12.0 * ui, pw, 2.0 * ui, [0.3, 0.33, 0.4, 1.0]);
        let team = (e.owner as usize).min(1);
        match e.kind {
            EntityKind::Unit => {
                let r = book.unit(self.unit_type[e.def as usize], team, 2, 0);
                let s = (pw - 24.0 * ui) / r.h as f32;
                self.gfx.sprite(out, r, x0 + pw * 0.5, cy + 12.0 * ui + pw * 0.55, r.w as f32 * s, r.h as f32 * s, WHITE);
            }
            EntityKind::Building => {
                let r = book.building(self.building_type[e.def as usize], team);
                let s = (pw - 10.0 * ui) / r.w as f32;
                self.gfx.sprite(out, r, x0 + pw * 0.5, cy + 12.0 * ui + pw * 0.55, r.w as f32 * s, r.h as f32 * s, WHITE);
            }
            EntityKind::Resource => {
                let r = if e.def == orion_sim::state::RES_GEYSER {
                    book.geyser
                } else {
                    book.minerals[0]
                };
                let s = (pw - 14.0 * ui) / r.w as f32;
                self.gfx.sprite(out, r, x0 + pw * 0.5, cy + 12.0 * ui + pw * 0.5, r.w as f32 * s, r.h as f32 * s, WHITE);
            }
        }

        let tx = x0 + pw + 16.0 * ui;
        let (name, maxhp) = match e.kind {
            EntityKind::Unit => {
                let d = &self.state.data.units[e.def as usize];
                (d.name.clone(), d.hp)
            }
            EntityKind::Building => {
                let d = &self.state.data.buildings[e.def as usize];
                (d.name.clone(), d.hp)
            }
            EntityKind::Resource => {
                let n = if e.def == orion_sim::state::RES_GEYSER {
                    "PLASMA GEYSER"
                } else {
                    "MINERAL FIELD"
                };
                (n.to_string(), 0)
            }
        };
        self.gfx.text(out, tx, cy + 16.0 * ui, self.ts(2.0), white, &name);
        if e.kind == EntityKind::Resource {
            self.gfx.text(
                out,
                tx,
                cy + 38.0 * ui,
                self.ts(1.5),
                [MINERAL_COLOR[0], MINERAL_COLOR[1], MINERAL_COLOR[2], 1.0],
                &format!("{} LEFT", e.amount),
            );
        } else {
            let frac = e.hp as f32 / maxhp.max(1) as f32;
            self.gfx.text(out, tx, cy + 38.0 * ui, self.ts(1.5), dim, &format!("HP {}/{}", e.hp, maxhp));
            self.gfx.quad(out, tx, cy + 54.0 * ui, 120.0 * ui, 5.0 * ui, [0.05, 0.05, 0.05, 1.0]);
            self.gfx.quad(out, tx, cy + 55.0 * ui, 120.0 * ui * frac.clamp(0.0, 1.0), 3.0 * ui, hp_color(frac));
        }
        // Extractor: show remaining gas.
        if e.kind == EntityKind::Building
            && self.state.data.buildings[e.def as usize].gas_extractor
            && e.construction.is_none()
        {
            self.gfx.text(
                out,
                tx,
                cy + 66.0 * ui,
                self.ts(1.5),
                [GAS_COLOR[0], GAS_COLOR[1], GAS_COLOR[2], 1.0],
                &format!("PLASMA {}", e.amount),
            );
        }
        if e.construction.is_some() {
            self.gfx.text(out, tx, cy + 66.0 * ui, self.ts(1.5), [0.95, 0.8, 0.3, 1.0], "CONSTRUCTING");
        }
        // Caster energy.
        if e.kind == EntityKind::Unit {
            let d = &self.state.data.units[e.def as usize];
            if d.energy_max > 0 {
                self.gfx.text(
                    out,
                    tx,
                    cy + 66.0 * ui,
                    self.ts(1.5),
                    [0.7, 0.55, 1.0, 1.0],
                    &format!("ENERGY {}/{}", e.energy, d.energy_max),
                );
            }
        }
        // Active research progress.
        if let Some((r, p)) = e.research {
            let rdef = &self.state.data.research[r as usize];
            let frac = p as f32 / rdef.ticks.max(1) as f32;
            self.gfx.text(out, tx, cy + 66.0 * ui, self.ts(1.5), [0.5, 0.9, 1.0, 1.0], &format!("RESEARCHING {}", rdef.name));
            self.gfx.quad(out, tx, cy + 84.0 * ui, 120.0 * ui, 5.0 * ui, [0.05, 0.05, 0.05, 1.0]);
            self.gfx.quad(out, tx, cy + 85.0 * ui, 120.0 * ui * frac, 3.0 * ui, [0.5, 0.9, 1.0, 1.0]);
        }
        if self.selection_types().len() > 1 {
            self.gfx.text(
                out,
                tx,
                cy + 80.0 * ui,
                self.ts(1.0),
                dim,
                "TAB: NEXT SUBGROUP",
            );
        }

        // Production queue: clickable slots with the front item's progress.
        if e.kind == EntityKind::Building && !e.queue.is_empty() {
            self.gfx.text(out, tx, cy + 50.0 * ui + 0.0, self.ts(1.0), dim, "");
            for (bx, by, bw, bh, k) in self.queue_slot_rects() {
                let unit = e.queue[k];
                let r = book.unit(self.unit_type[unit as usize], team, 2, 0);
                self.gfx.quad(out, bx, by, bw, bh, [0.06, 0.06, 0.09, 1.0]);
                self.gfx.quad(out, bx, by, bw, 1.5 * ui, [0.3, 0.33, 0.4, 1.0]);
                self.gfx.sprite(out, r, bx + bw * 0.5, by + bh * 0.45, r.w as f32 * 0.9 * ui, r.h as f32 * 0.9 * ui, WHITE);
                if k == 0 {
                    let total = self.state.data.units[unit as usize].build_ticks;
                    let frac = e.progress as f32 / total.max(1) as f32;
                    self.gfx.quad(out, bx, by + bh - 3.0 * ui, bw * frac, 3.0 * ui, [0.3, 0.8, 1.0, 1.0]);
                }
            }
            let qr = self.queue_slot_rects();
            if let Some((bx, by, ..)) = qr.first().copied() {
                self.gfx.text(out, bx, by - 12.0 * ui, self.ts(1.0), dim, "QUEUE (CLICK TO CANCEL)");
            }
        }

        // Multi-select strip: units AND buildings; the Tab-active subgroup
        // gets a highlighted border. Layout shared with tooltips.
        let active = self.active_type();
        for (bx2, by2, tw2, th2, id) in self.strip_tiles() {
            let Some(u) = self.state.get(id) else { continue };
            let (bx, by) = (bx2 + 2.0, by2 + 2.0);
            let is_active = active == Some((u.kind, u.def));
            let border = if is_active {
                [0.4, 1.0, 0.4, 1.0]
            } else {
                [0.06, 0.06, 0.09, 1.0]
            };
            self.gfx.quad(out, bx - 2.0, by - 2.0, tw2, th2, border);
            self.gfx.quad(out, bx - 1.0, by - 1.0, tw2 - 2.0 * ui, th2 - 2.0 * ui, [0.06, 0.06, 0.09, 1.0]);
            match u.kind {
                EntityKind::Unit => {
                    let r = book.unit(self.unit_type[u.def as usize], (u.owner as usize).min(1), 2, 0);
                    self.gfx.sprite(out, r, bx + 14.0 * ui, by + 18.0 * ui, r.w as f32 * 0.8 * ui, r.h as f32 * 0.8 * ui, WHITE);
                    let frac = u.hp as f32 / self.state.data.units[u.def as usize].hp as f32;
                    self.gfx.quad(out, bx, by + 37.0 * ui, 28.0 * ui * frac, 2.0 * ui, hp_color(frac));
                }
                EntityKind::Building => {
                    let r = book.building(self.building_type[u.def as usize], (u.owner as usize).min(1));
                    let s = (28.0 * ui) / r.w as f32;
                    self.gfx.sprite(out, r, bx + 14.0 * ui, by + 18.0 * ui, r.w as f32 * s, r.h as f32 * s, WHITE);
                    let frac = u.hp as f32 / self.state.data.buildings[u.def as usize].hp as f32;
                    self.gfx.quad(out, bx, by + 37.0 * ui, 28.0 * ui * frac, 2.0 * ui, hp_color(frac));
                }
                EntityKind::Resource => continue,
            }
        }
    }

    /// Command card layout: SC2-style 4-column grid with sprite icons.
    /// Shared by draw and click handling.
    pub(crate) fn card_buttons(&self) -> Vec<CardButton> {
        let ui = self.ui();
        let w = self.cam.screen_w;
        let cy = self.cam.screen_h - self.console_h();
        let bw = 52.0 * ui;
        let bh = 46.0 * ui;
        let gap = 4.0 * ui;
        let bx0 = w - 4.0 * (bw + gap) - 10.0 * ui;
        let mut list: Vec<(String, String, CardIcon, CardAction, Vec<(String, [f32; 4])>)> =
            Vec::new();
        let key_of = |a: crate::config::Action| crate::config::key_label(self.settings.key_for(a));
        match self.mode {
            Mode::BuildMenu => {
                let place_keys = [
                    crate::config::Action::Place1,
                    crate::config::Action::Place2,
                    crate::config::Action::Place3,
                    crate::config::Action::Place4,
                    crate::config::Action::Place5,
                    crate::config::Action::Place6,
                    crate::config::Action::Place7,
                ];
                for (slot, def) in self.build_menu_defs().into_iter().enumerate().take(7) {
                    let act = place_keys[slot];
                    let b = &self.state.data.buildings[def as usize];
                    let met = self.state.requirement_met(self.human, b.requires);
                    let cost = if !met {
                        let req = &self.state.data.buildings[b.requires.unwrap() as usize];
                        format!("NEEDS {}", req.name)
                    } else if b.cost_gas > 0 {
                        format!("{} {}G", b.cost_minerals, b.cost_gas)
                    } else {
                        format!("{}", b.cost_minerals)
                    };
                    list.push((
                        key_of(act),
                        cost,
                        CardIcon::Building(self.building_type[def as usize]),
                        if met { CardAction::Place(def) } else { CardAction::CancelMode },
                        self.tip_building(def),
                    ));
                }
                list.push((
                    "ESC".into(),
                    "CANCEL".into(),
                    CardIcon::Letter,
                    CardAction::CancelMode,
                    Self::tip_action("CANCEL", "CLOSE THE BUILD MENU."),
                ));
            }
            Mode::Placing(_) | Mode::AttackMove | Mode::CastTarget => {
                list.push((
                    "ESC".into(),
                    "CANCEL".into(),
                    CardIcon::Letter,
                    CardAction::CancelMode,
                    Self::tip_action("CANCEL", "EXIT THIS TARGETING MODE."),
                ));
            }
            Mode::Normal => {
                if let Some(bid) = self.single_own_building() {
                    let b = &self.state.entities[bid.idx as usize];
                    if b.construction.is_some() {
                        // Under construction: only cancel is available.
                        let key = crate::config::key_label(
                            self.settings.key_for(crate::config::Action::CancelConstruction),
                        );
                        let d = &self.state.data.buildings[b.def as usize];
                        let refund = format!("+{}", d.cost_minerals * 3 / 4);
                        return vec![CardButton {
                            x: bx0 + 3.0 * (bw + gap),
                            y: cy + 12.0 * ui,
                            w: bw,
                            h: bh,
                            key,
                            hint: format!("CANCEL {refund}"),
                            icon: CardIcon::Letter,
                            action: CardAction::CancelConstructionBtn,
                            tip: Self::tip_action(
                                "CANCEL CONSTRUCTION",
                                "ABORT THIS BUILDING - 75% OF ITS COST IS REFUNDED.",
                            ),
                        }];
                    }
                    let bdef = &self.state.data.buildings[b.def as usize];
                    let trains = &bdef.trains;
                    let train_keys = [
                        crate::config::Action::Train0,
                        crate::config::Action::Train1,
                        crate::config::Action::Train2,
                    ];
                    for (k, &u) in trains.iter().take(3).enumerate() {
                        let d = &self.state.data.units[u as usize];
                        let met = self.state.requirement_met(self.human, d.requires);
                        let cost = if !met {
                            let req =
                                &self.state.data.buildings[d.requires.unwrap() as usize];
                            format!("NEEDS {}", req.name)
                        } else if d.cost_gas > 0 {
                            format!("{} {}G", d.cost_minerals, d.cost_gas)
                        } else {
                            format!("{}", d.cost_minerals)
                        };
                        list.push((
                            key_of(train_keys[k]),
                            cost,
                            CardIcon::Unit(self.unit_type[u as usize]),
                            if met { CardAction::Train(k) } else { CardAction::CancelMode },
                            self.tip_unit(u),
                        ));
                    }
                    // Research options (Archive).
                    for (k, &r) in bdef.researches.iter().take(4).enumerate() {
                        let rdef = &self.state.data.research[r as usize];
                        let done = self.state.players[self.human as usize].research_done
                            [r as usize];
                        let prereq_ok = rdef.requires.map_or(true, |p| {
                            self.state.players[self.human as usize].research_done[p as usize]
                        });
                        let researching = b.research.map_or(false, |(id, _)| id == r);
                        // Keep the on-button label SHORT — the tooltip has
                        // the full name, cost and effect.
                        let hint = if done {
                            "DONE".to_string()
                        } else if researching {
                            "...".to_string()
                        } else if !prereq_ok {
                            "LOCKED".to_string()
                        } else {
                            format!("{}+{}G", rdef.cost_minerals, rdef.cost_gas)
                        };
                        let keys = [
                            crate::config::Action::Train0,
                            crate::config::Action::Train1,
                            crate::config::Action::Train2,
                            crate::config::Action::Place4,
                        ];
                        list.push((
                            key_of(keys[k]),
                            format!("{} {}", short_research(&rdef.tag), hint),
                            CardIcon::Letter,
                            if done || researching || !prereq_ok {
                                CardAction::CancelMode
                            } else {
                                CardAction::Research(r)
                            },
                            self.tip_research(r),
                        ));
                    }
                } else if self.own_selected_units().next().is_some() {
                    list.push((
                        key_of(crate::config::Action::Attack),
                        "ATTACK".into(),
                        CardIcon::Letter,
                        CardAction::RunAttack,
                        Self::tip_action("ATTACK MOVE", "MOVE AND ENGAGE ANY ENEMY MET ON THE WAY."),
                    ));
                    list.push((
                        key_of(crate::config::Action::Stop),
                        "STOP".into(),
                        CardIcon::Letter,
                        CardAction::RunStop,
                        Self::tip_action("STOP", "HALT AND CLEAR ALL QUEUED ORDERS."),
                    ));
                    list.push((
                        key_of(crate::config::Action::Hold),
                        "HOLD".into(),
                        CardIcon::Letter,
                        CardAction::RunHold,
                        Self::tip_action("HOLD POSITION", "STAND GROUND - FIRE, NEVER CHASE."),
                    ));
                    if self.selected_builder().is_some() {
                        list.push((
                            key_of(crate::config::Action::BuildMenu),
                            "BUILD".into(),
                            CardIcon::Letter,
                            CardAction::OpenBuild,
                            Self::tip_action("BUILD", "OPEN THE CONSTRUCTION MENU."),
                        ));
                    }
                    if self.any_selected_siege() {
                        list.push((
                            key_of(crate::config::Action::SiegeToggle),
                            "SIEGE".into(),
                            CardIcon::Letter,
                            CardAction::SiegeBtn,
                            Self::tip_action(
                                "SIEGE MODE",
                                "DEPLOY: IMMOBILE, LONG RANGE, SPLASH DAMAGE, MIN RANGE.",
                            ),
                        ));
                    }
                    if self.any_selected_caster() {
                        list.push((
                            key_of(crate::config::Action::CastStorm),
                            "STORM 75E".into(),
                            CardIcon::Letter,
                            CardAction::StormBtn,
                            Self::tip_action(
                                "PLASMA STORM - 75 ENERGY",
                                "CRACKLING ZONE THAT DAMAGES EVERYTHING INSIDE FOR 3 SECONDS.",
                            ),
                        ));
                    }
                }
            }
        }
        list.into_iter()
            .enumerate()
            .map(|(k, (key, hint, icon, action, tip))| {
                let col = k % 4;
                let row = k / 4;
                CardButton {
                    tip,
                    x: bx0 + col as f32 * (bw + gap),
                    y: cy + 12.0 * ui + row as f32 * (bh + gap),
                    w: bw,
                    h: bh,
                    key,
                    hint,
                    icon,
                    action,
                }
            })
            .collect()
    }

    fn draw_command_card(&self, out: &mut Vec<Inst>) {
        let ui = self.ui();
        let white = [0.92, 0.92, 0.88, 1.0];
        let book = &self.gfx.book;
        for b in self.card_buttons() {
            let hover = self.mouse.0 >= b.x
                && self.mouse.0 <= b.x + b.w
                && self.mouse.1 >= b.y
                && self.mouse.1 <= b.y + b.h;
            let bg = if hover { [0.13, 0.15, 0.20, 1.0] } else { [0.08, 0.09, 0.12, 1.0] };
            self.gfx.quad(out, b.x, b.y, b.w, b.h, bg);
            self.gfx.quad(out, b.x, b.y, b.w, 1.5 * ui, [0.34, 0.37, 0.45, 1.0]);
            self.gfx.quad(out, b.x, b.y + b.h - 1.5 * ui, b.w, 1.5 * ui, [0.04, 0.04, 0.06, 1.0]);
            // Icon fills the button; hotkey letter overlays the top-left
            // corner; cost/label sits along the bottom (SC2 style).
            let icon_cy = b.y + b.h * 0.44;
            match b.icon {
                CardIcon::Building(btype) => {
                    let r = book.building(btype, self.human as usize);
                    let s = (b.h * 0.72) / r.h as f32;
                    self.gfx.sprite(out, r, b.x + b.w * 0.5, icon_cy, r.w as f32 * s, r.h as f32 * s, WHITE);
                }
                CardIcon::Unit(utype) => {
                    let r = book.unit(utype, self.human as usize, 2, 0);
                    let s = (b.h * 0.7) / r.h as f32;
                    self.gfx.sprite(out, r, b.x + b.w * 0.5, icon_cy, r.w as f32 * s, r.h as f32 * s, WHITE);
                }
                CardIcon::Letter => {
                    let ts = self.ts(2.5);
                    let tw = self.gfx.text_width(ts, &b.key);
                    self.gfx.text(out, b.x + b.w * 0.5 - tw * 0.5, b.y + b.h * 0.28, ts, white, &b.key);
                }
            }
            // Hotkey badge.
            if !matches!(b.icon, CardIcon::Letter) {
                self.gfx.quad(out, b.x + 2.0 * ui, b.y + 2.0 * ui, 12.0 * ui, 11.0 * ui, [0.02, 0.02, 0.04, 0.85]);
                self.gfx.text(out, b.x + 4.0 * ui, b.y + 4.0 * ui, self.ts(1.2), [1.0, 0.9, 0.4, 1.0], &b.key);
            }
            // Clip the hint to the button width — tooltips carry the rest.
            let hs = self.ts(0.9);
            let mut hint = b.hint.clone();
            while hint.len() > 2 && self.gfx.text_width(hs, &hint) > b.w - 2.0 * ui {
                hint.pop();
            }
            let hw = self.gfx.text_width(hs, &hint);
            self.gfx.text(
                out,
                b.x + b.w * 0.5 - hw * 0.5,
                b.y + b.h - 10.0 * ui,
                hs,
                [0.72, 0.86, 0.95, 1.0],
                &hint,
            );
        }
    }

    fn draw_mode_hint(&self, out: &mut Vec<Inst>) {
        let w = self.cam.screen_w;
        let cy = self.cam.screen_h - self.console_h();
        let hint = match self.mode {
            Mode::AttackMove => Some("ATTACK MOVE: CLICK TARGET"),
            Mode::Placing(_) => Some("CLICK TO PLACE   SHIFT: CHAIN   ESC: CANCEL"),
            Mode::CastTarget => Some("PLASMA STORM: CLICK TARGET (75 ENERGY)"),
            _ => None,
        };
        if let Some(hint) = hint {
            let ts = self.ts(1.5);
            let tw = self.gfx.text_width(ts, hint);
            self.gfx.quad(out, w * 0.5 - tw * 0.5 - 10.0, cy - 34.0, tw + 20.0, 24.0, [0.04, 0.04, 0.06, 0.85]);
            self.gfx.text(out, w * 0.5 - tw * 0.5, cy - 29.0, ts, [1.0, 0.9, 0.4, 1.0], hint);
        }
    }

    fn draw_top_status(&self, out: &mut Vec<Inst>) {
        let ui = self.ui();
        let w = self.cam.screen_w;
        let white = [0.92, 0.92, 0.88, 1.0];
        let book = &self.gfx.book;
        let ts = self.ts(2.0);
        let icon_y = 12.0 * ui + 5.0 * ui;

        let minerals = self.state.players[self.human as usize].minerals;
        let gas = self.state.players[self.human as usize].gas;
        let (used, prov) = self.state.supply(self.human);

        // Right-aligned: minerals | gas | supply.
        let sup_s = format!("{used}/{prov}");
        let sup_col = if used >= prov { [1.0, 0.45, 0.3, 1.0] } else { white };
        let mut x = w - 16.0 * ui - self.gfx.text_width(ts, &sup_s);
        self.gfx.text(out, x, 10.0 * ui, ts, sup_col, &sup_s);
        let pr = book.building(1, self.human as usize);
        self.gfx.sprite(out, pr, x - 12.0 * ui, icon_y, pr.w as f32 * 0.3 * ui, pr.h as f32 * 0.3 * ui, WHITE);

        let gas_s = format!("{gas}");
        x -= 70.0 * ui + self.gfx.text_width(ts, &gas_s);
        self.gfx.text(out, x, 10.0 * ui, ts, [GAS_COLOR[0], GAS_COLOR[1], GAS_COLOR[2], 1.0], &gas_s);
        let gr = book.geyser;
        self.gfx.sprite(out, gr, x - 14.0 * ui, icon_y, gr.w as f32 * 0.28 * ui, gr.h as f32 * 0.28 * ui, WHITE);

        let min_s = format!("{minerals}");
        x -= 70.0 * ui + self.gfx.text_width(ts, &min_s);
        self.gfx.text(out, x, 10.0 * ui, ts, [0.75, 0.93, 1.0, 1.0], &min_s);
        let mr = book.minerals[0];
        self.gfx.sprite(out, mr, x - 13.0 * ui, icon_y, mr.w as f32 * 0.5 * ui, mr.h as f32 * 0.5 * ui, WHITE);

        // Clock + FPS, top-left, subtle.
        let secs = self.state.tick / 24;
        self.gfx.text(
            out,
            10.0 * ui,
            10.0 * ui,
            self.ts(1.0),
            [0.5, 0.5, 0.5, 0.8],
            &format!("{:02}:{:02}  FPS {:.0}", secs / 60, secs % 60, self.fps),
        );
    }

    fn draw_banner(&self, out: &mut Vec<Inst>) {
        // Multiplayer terminal states outrank the normal banner.
        if let Some(mp) = &self.mp {
            let msg = if mp.desync {
                Some(("DESYNC DETECTED", [1.0, 0.6, 0.2, 1.0]))
            } else if mp.disconnected && self.state.winner.is_none() {
                Some(("OPPONENT DISCONNECTED", [0.4, 1.0, 0.4, 1.0]))
            } else {
                None
            };
            if let Some((msg, color)) = msg {
                let w = self.cam.screen_w;
                let h = self.cam.screen_h;
                self.gfx.quad(out, 0.0, h * 0.5 - 70.0 * self.ui(), w, 140.0 * self.ui(), [0.02, 0.02, 0.03, 0.85]);
                let ts = self.ts(4.0);
                let tw = self.gfx.text_width(ts, msg);
                self.gfx.text(out, (w - tw) * 0.5, h * 0.5 - 30.0 * self.ui(), ts, color, msg);
                let sub = "ESC: MENU";
                let ts2 = self.ts(2.0);
                let sw = self.gfx.text_width(ts2, sub);
                self.gfx.text(out, (w - sw) * 0.5, h * 0.5 + 24.0 * self.ui(), ts2, [0.7, 0.7, 0.7, 1.0], sub);
                return;
            }
        }
        let Some(winner) = self.state.winner else { return };
        let w = self.cam.screen_w;
        let h = self.cam.screen_h;
        let (msg, color) = if winner == self.human {
            ("VICTORY!", [0.4, 1.0, 0.4, 1.0])
        } else {
            ("DEFEAT", [1.0, 0.35, 0.3, 1.0])
        };
        self.gfx.quad(out, 0.0, h * 0.5 - 70.0 * self.ui(), w, 140.0 * self.ui(), [0.02, 0.02, 0.03, 0.85]);
        let ts = self.ts(6.0);
        let tw = self.gfx.text_width(ts, msg);
        self.gfx.text(out, (w - tw) * 0.5, h * 0.5 - 40.0 * self.ui(), ts, color, msg);
        // Match stats.
        let secs = self.state.tick / 24;
        let ts3 = self.ts(1.4);
        let mut y = h * 0.5 + 16.0 * self.ui();
        let time_l = format!("MATCH TIME {:02}:{:02}", secs / 60, secs % 60);
        let tw3 = self.gfx.text_width(ts3, &time_l);
        self.gfx.text(out, (w - tw3) * 0.5, y, ts3, [0.75, 0.75, 0.72, 1.0], &time_l);
        y += 20.0 * self.ui();
        for (p, pl) in self.state.players.iter().enumerate() {
            let name = self
                .state
                .data
                .race_names
                .get(pl.race as usize)
                .cloned()
                .unwrap_or_default()
                .to_uppercase();
            let line = format!(
                "P{}  {}  UNITS {}  LOST {}  MINED {}+{}G",
                p + 1,
                name,
                pl.units_built,
                pl.units_lost + pl.buildings_lost,
                pl.minerals_mined,
                pl.gas_mined,
            );
            let c = TEAM_COLORS[p % 2];
            let lw = self.gfx.text_width(ts3, &line);
            self.gfx.text(out, (w - lw) * 0.5, y, ts3, [c[0], c[1], c[2], 1.0], &line);
            y += 18.0 * self.ui();
        }
        let sub = "R: PLAY AGAIN   ESC: MENU";
        let ts2 = self.ts(2.0);
        let sw = self.gfx.text_width(ts2, sub);
        self.gfx.text(out, (w - sw) * 0.5, y + 8.0 * self.ui(), ts2, [0.7, 0.7, 0.7, 1.0], sub);
    }

    // ----------------------------------------------------------- click ----

    /// A left click landed on the console. Route it.
    pub(crate) fn console_click(&mut self) {
        // Minimap: move camera.
        if let Some((wx, wy)) = self.minimap_pick(self.mouse.0, self.mouse.1) {
            let (ix, iy) = crate::iso::world_to_iso(wx, wy);
            self.cam.cx = ix;
            self.cam.cy = iy;
            return;
        }
        // Production queue cancel.
        for (bx, by, bw, bh, slot) in self.queue_slot_rects() {
            if self.mouse.0 >= bx
                && self.mouse.0 <= bx + bw
                && self.mouse.1 >= by
                && self.mouse.1 <= by + bh
            {
                self.cancel_train(slot);
                return;
            }
        }
        // Command card.
        let mut hit: Option<CardAction> = None;
        for b in self.card_buttons() {
            if self.mouse.0 >= b.x
                && self.mouse.0 <= b.x + b.w
                && self.mouse.1 >= b.y
                && self.mouse.1 <= b.y + b.h
            {
                hit = Some(b.action);
                break;
            }
        }
        if let Some(action) = hit {
            match action {
                CardAction::RunAttack => self.run_action(crate::config::Action::Attack),
                CardAction::RunStop => self.run_action(crate::config::Action::Stop),
                CardAction::RunHold => self.run_action(crate::config::Action::Hold),
                CardAction::OpenBuild => self.run_action(crate::config::Action::BuildMenu),
                CardAction::Train(k) => self.train_hotkey(k),
                CardAction::Place(def) => self.mode = Mode::Placing(def),
                CardAction::Research(r) => {
                    if let Some(bid) = self.primary_selected_building() {
                        self.pending
                            .push((self.human, orion_sim::Command::Research {
                                building: bid,
                                research: r,
                            }));
                    }
                }
                CardAction::SiegeBtn => self.run_action(crate::config::Action::SiegeToggle),
                CardAction::StormBtn => self.run_action(crate::config::Action::CastStorm),
                CardAction::CancelMode => self.mode = Mode::Normal,
                CardAction::CancelConstructionBtn => {
                    self.run_action(crate::config::Action::CancelConstruction)
                }
            }
        }
    }
}
