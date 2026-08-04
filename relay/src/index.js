// Orion lobby relay: one Durable Object per lobby code relaying WebSocket
// text frames between exactly two peers, plus a Directory DO listing open
// public lobbies. The relay never parses game traffic.
//
// The relay is public and unauthenticated by design — the 5-letter lobby code
// is the only credential. Nothing below is a security boundary; it exists to
// make a flood cost the attacker something and to bound what a flood can do
// to storage, bandwidth, and the singleton Directory DO. Every threshold sits
// well clear of legitimate traffic: one WebSocket per player for the length
// of a game, a /lobbies poll every ~3s, and lockstep frames at 24 Hz.

const LOBBY_TTL_MS = 15 * 60 * 1000; // stale host cleanup
const LIST_CACHE_MS = 2000; // memoize /lobbies between client polls
const MAX_LISTED = 200; // cap on public directory size

// Per-connection relay caps. Lockstep sends ~24-48 frames/s per peer, all of
// them short RON lines, so these leave >5x headroom while stopping the relay
// from being used as a free data pipe.
const MAX_MSG_BYTES = 16 * 1024;
const MSG_RATE_PER_S = 120;
const MSG_BURST = 300;
const MAX_CONN_BYTES = 128 * 1024 * 1024;

export class Directory {
  constructor(state) {
    this.state = state;
    this.listJson = null; // memoized list response
    this.listAt = 0;
  }

  // Read storage and drop entries whose host died without cleaning up.
  // Returns [lobbies, prunedAnything] so callers decide whether to write —
  // the list path used to write on every single poll.
  async load(now) {
    const lobbies = (await this.state.storage.get("lobbies")) || {};
    let pruned = false;
    for (const [code, l] of Object.entries(lobbies)) {
      if (now - l.created > LOBBY_TTL_MS) {
        delete lobbies[code];
        pruned = true;
      }
    }
    return [lobbies, pruned];
  }

  async save(lobbies) {
    await this.state.storage.put("lobbies", lobbies);
    this.listJson = null; // adds/removes must show up on the next poll
  }

  json(body) {
    return new Response(body, {
      headers: { "content-type": "application/json" },
    });
  }

  async fetch(request) {
    const url = new URL(request.url);
    const now = Date.now();

    if (url.pathname === "/add") {
      const l = await request.json();
      const [lobbies] = await this.load(now);
      // Bound the directory: a flood of hosts can't grow storage without
      // limit or bloat the list response. Re-listing an existing code is
      // always allowed so a legitimate host can't be locked out by the cap.
      if (!(l.code in lobbies) && Object.keys(lobbies).length >= MAX_LISTED) {
        return new Response("directory full", { status: 429 });
      }
      lobbies[l.code] = { name: l.name, race: l.race, created: now };
      await this.save(lobbies);
      return new Response("ok");
    }

    if (url.pathname === "/remove") {
      const { code } = await request.json();
      const [lobbies, pruned] = await this.load(now);
      if (code in lobbies) {
        delete lobbies[code];
        await this.save(lobbies);
      } else if (pruned) {
        await this.save(lobbies);
      }
      return new Response("ok");
    }

    // List path. Served from memory between polls: clients poll every ~3s, so
    // without this every poll cost a storage read *and* a write. age_s can be
    // up to LIST_CACHE_MS stale, which the UI doesn't care about.
    if (this.listJson !== null && now - this.listAt < LIST_CACHE_MS) {
      return this.json(this.listJson);
    }
    const [lobbies, pruned] = await this.load(now);
    if (pruned) await this.save(lobbies);
    const list = Object.entries(lobbies).map(([code, l]) => ({
      code,
      name: l.name,
      race: l.race,
      age_s: Math.floor((now - l.created) / 1000),
    }));
    this.listJson = JSON.stringify(list);
    this.listAt = now;
    return this.json(this.listJson);
  }
}

export class Lobby {
  constructor(state, env) {
    this.env = env;
    this.host = null;
    this.join = null;
    this.code = null;
    this.listed = false;
  }

  directory() {
    return this.env.DIRECTORY.get(this.env.DIRECTORY.idFromName("directory"));
  }

  async unlist() {
    if (this.listed && this.code) {
      this.listed = false;
      try {
        await this.directory().fetch("https://dir/remove", {
          method: "POST",
          body: JSON.stringify({ code: this.code }),
        });
      } catch (_) {}
    }
  }

  async fetch(request) {
    const url = new URL(request.url);
    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("expected websocket", { status: 400 });
    }
    const role = url.searchParams.get("role");
    this.code = url.pathname.split("/").pop();
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    server.accept();

    const fail = (reason) => {
      try {
        server.send(JSON.stringify({ relay_error: reason }));
        server.close(1008, reason);
      } catch (_) {}
      return new Response(null, { status: 101, webSocket: client });
    };

    if (role === "host") {
      if (this.host) return fail("code already in use");
      this.host = server;
      // Public lobbies appear in the directory until someone joins.
      if (url.searchParams.get("private") !== "1") {
        const name = (url.searchParams.get("name") || "COMMANDER").slice(0, 16);
        const race = parseInt(url.searchParams.get("race") || "0", 10) || 0;
        try {
          const res = await this.directory().fetch("https://dir/add", {
            method: "POST",
            body: JSON.stringify({ code: this.code, name, race }),
          });
          // Only claim a directory seat we actually got, so unlist() can't
          // remove someone else's entry later.
          this.listed = res.ok;
        } catch (_) {}
      }
    } else if (role === "join") {
      if (!this.host) return fail("no such lobby");
      if (this.join) return fail("lobby full");
      this.join = server;
      await this.unlist(); // seat filled: stop advertising
    } else {
      return fail("bad role");
    }

    // Token bucket + byte budget, per connection. Returns a reason string on
    // violation, else null.
    const meter = { tokens: MSG_BURST, last: Date.now(), bytes: 0 };
    const violation = (size) => {
      if (size > MAX_MSG_BYTES) return "frame too large";
      const now = Date.now();
      // Clamp elapsed at 0: a clock that steps backwards must not be able to
      // drain the bucket and kill a legitimate game.
      const elapsed = Math.max(0, now - meter.last);
      meter.tokens = Math.min(
        MSG_BURST,
        meter.tokens + (elapsed / 1000) * MSG_RATE_PER_S,
      );
      meter.last = now;
      meter.bytes += size;
      if (meter.bytes > MAX_CONN_BYTES) return "byte budget exhausted";
      if (meter.tokens < 1) return "message rate exceeded";
      meter.tokens -= 1;
      return null;
    };

    const other = () => (server === this.host ? this.join : this.host);
    server.addEventListener("message", (ev) => {
      const size =
        typeof ev.data === "string" ? ev.data.length : ev.data.byteLength;
      const bad = violation(size);
      if (bad) {
        // Closing fires the close listener below, which tears the pair down.
        try {
          server.send(JSON.stringify({ relay_error: bad }));
          server.close(1008, bad);
        } catch (_) {}
        return;
      }
      const o = other();
      if (o) {
        try {
          o.send(ev.data);
        } catch (_) {}
      }
    });
    const teardown = () => {
      this.unlist();
      const o = other();
      if (o) {
        try {
          o.close(1000, "peer left");
        } catch (_) {}
      }
      this.host = null;
      this.join = null;
    };
    server.addEventListener("close", teardown);
    server.addEventListener("error", teardown);

    return new Response(null, { status: 101, webSocket: client });
  }
}

// ---------------------------------------------------------------------------
// Automatic matchmaking: a singleton DO holding the ranked queue + ratings.
//
// Clients hold a WebSocket to /queue while searching. A pass every few
// seconds pairs players whose MMR gap fits inside BOTH players' tolerance
// (which widens the longer they wait) AND whose combined relay latency fits
// their latency window (also widening). Matched players get a fresh lobby
// code and reconnect through the ordinary Lobby relay — the matchmaker is
// only an introducer. Ratings are Elo (K=32, start 1200), updated when both
// players report the same winner; a single report resolves after a timeout
// so a rage-quit can't freeze ratings. Humans only by construction: every
// queue entry is a live client socket.

const MM_PASS_MS = 3000;
const MM_START_MMR = 1200;
const MM_K = 32;
const MM_TOL_BASE = 100; // +- MMR at 0s wait
const MM_TOL_STEP = 60; // widens per 10s waited
const MM_LAT_BASE = 250; // combined ms allowed at 0s wait
const MM_LAT_STEP = 80;
const MM_RESULT_TIMEOUT_MS = 120 * 1000;
const MM_QUEUE_CAP = 500;

export class Matchmaker {
  constructor(state) {
    this.state = state;
    this.queue = new Map(); // id -> entry
  }

  async rating(id) {
    return (
      (await this.state.storage.get(`mmr:${id}`)) || {
        mmr: MM_START_MMR,
        games: 0,
      }
    );
  }

  json(obj, status = 200) {
    return new Response(JSON.stringify(obj), {
      status,
      headers: { "content-type": "application/json" },
    });
  }

  async armAlarm() {
    const cur = await this.state.storage.getAlarm();
    if (cur === null) {
      await this.state.storage.setAlarm(Date.now() + MM_PASS_MS);
    }
  }

  async fetch(request) {
    const url = new URL(request.url);

    if (url.pathname === "/queue") {
      if (request.headers.get("Upgrade") !== "websocket") {
        return new Response("expected websocket", { status: 400 });
      }
      const id = (url.searchParams.get("id") || "").slice(0, 32);
      const name = (url.searchParams.get("name") || "COMMANDER").slice(0, 16);
      const race = parseInt(url.searchParams.get("race") || "0", 10) || 0;
      const rtt = Math.min(
        2000,
        parseInt(url.searchParams.get("rtt") || "100", 10) || 100,
      );
      const pair = new WebSocketPair();
      const [client, server] = Object.values(pair);
      server.accept();
      if (!id || this.queue.size >= MM_QUEUE_CAP) {
        try {
          server.send(JSON.stringify({ type: "error", reason: "queue full" }));
          server.close(1008, "queue full");
        } catch (_) {}
        return new Response(null, { status: 101, webSocket: client });
      }
      // Re-queueing the same id replaces the old socket (client restarted).
      const prev = this.queue.get(id);
      if (prev) {
        try {
          prev.ws.close(1000, "requeued");
        } catch (_) {}
      }
      const { mmr, games } = await this.rating(id);
      // Remember the display name for the leaderboard.
      const rec = await this.rating(id);
      rec.name = name;
      await this.state.storage.put(`mmr:${id}`, rec);
      const entry = {
        id,
        ws: server,
        name,
        race,
        rtt,
        continent: (request.cf && request.cf.continent) || "??",
        mmr,
        games,
        since: Date.now(),
      };
      this.queue.set(id, entry);
      const drop = () => {
        if (this.queue.get(id) === entry) this.queue.delete(id);
      };
      server.addEventListener("close", drop);
      server.addEventListener("error", drop);
      try {
        server.send(JSON.stringify({ type: "queued", mmr, games }));
      } catch (_) {}
      await this.armAlarm();
      return new Response(null, { status: 101, webSocket: client });
    }

    if (url.pathname === "/result" && request.method === "POST") {
      const { code, id, winner_slot } = await request.json();
      if (typeof code !== "string" || typeof id !== "string") {
        return this.json({ error: "bad report" }, 400);
      }
      const key = `match:${code}`;
      const match = await this.state.storage.get(key);
      if (!match) return this.json({ error: "unknown match" }, 404);
      const slot = match.ids.indexOf(id);
      if (slot === -1) return this.json({ error: "not your match" }, 403);
      if (winner_slot !== 0 && winner_slot !== 1) {
        return this.json({ error: "bad winner" }, 400);
      }
      match.reports[slot] = winner_slot;
      const [ra, rb] = match.reports;
      if (ra !== null && rb !== null) {
        if (ra === rb) {
          await this.resolve(code, match, ra);
        } else {
          await this.state.storage.delete(key); // liars: discard
        }
      } else {
        await this.state.storage.put(key, match);
        await this.armAlarm(); // timeout resolution needs a tick
      }
      return this.json({ ok: true });
    }

    if (url.pathname === "/rating") {
      const id = (url.searchParams.get("id") || "").slice(0, 32);
      return this.json(await this.rating(id));
    }

    if (url.pathname === "/leaderboard") {
      const all = await this.state.storage.list({ prefix: "mmr:" });
      const rows = [];
      for (const [key, r] of all) {
        if ((r.games || 0) < 1) continue;
        rows.push({
          id: key.slice(4, 12), // prefix only: identify yourself, dox nobody
          name: r.name || "UNKNOWN",
          mmr: Math.round(r.mmr),
          games: r.games,
        });
      }
      rows.sort((a, b) => b.mmr - a.mmr);
      return this.json(rows.slice(0, 25));
    }

    return new Response("not found", { status: 404 });
  }

  // Elo update + cleanup for a decided match.
  async resolve(code, match, winnerSlot) {
    const [a, b] = match.ids;
    const ra = await this.rating(a);
    const rb = await this.rating(b);
    const expA = 1 / (1 + Math.pow(10, (rb.mmr - ra.mmr) / 400));
    const scoreA = winnerSlot === 0 ? 1 : 0;
    const delta = Math.round(MM_K * (scoreA - expA));
    ra.mmr += delta;
    rb.mmr -= delta;
    ra.games += 1;
    rb.games += 1;
    await this.state.storage.put(`mmr:${a}`, ra);
    await this.state.storage.put(`mmr:${b}`, rb);
    await this.state.storage.delete(`match:${code}`);
  }

  async alarm() {
    const now = Date.now();

    // Resolve single-report matches past the timeout; expire ancient ones.
    const matches = await this.state.storage.list({ prefix: "match:" });
    for (const [key, match] of matches) {
      const [ra, rb] = match.reports;
      const one = ra !== null ? ra : rb;
      if (one !== null && now - match.created > MM_RESULT_TIMEOUT_MS) {
        await this.resolve(key.slice("match:".length), match, one);
      } else if (now - match.created > 4 * 60 * 60 * 1000) {
        await this.state.storage.delete(key); // nobody ever reported
      }
    }

    // Matching pass: longest-waiting first, best-scoring partner that fits
    // both players' (widening) MMR and latency windows.
    const entries = [...this.queue.values()].sort((x, y) => x.since - y.since);
    const tol = (e) =>
      MM_TOL_BASE + MM_TOL_STEP * Math.floor((now - e.since) / 10000);
    const latCap = (e) =>
      MM_LAT_BASE + MM_LAT_STEP * Math.floor((now - e.since) / 10000);
    const taken = new Set();
    for (const a of entries) {
      if (taken.has(a.id)) continue;
      let best = null;
      for (const b of entries) {
        if (b.id === a.id || taken.has(b.id)) continue;
        const gap = Math.abs(a.mmr - b.mmr);
        const lat = a.rtt + b.rtt + (a.continent !== b.continent ? 150 : 0);
        if (gap > Math.min(tol(a), tol(b))) continue;
        if (lat > Math.min(latCap(a), latCap(b))) continue;
        const score = gap + lat;
        if (!best || score < best.score) best = { b, score };
      }
      if (best) {
        taken.add(a.id);
        taken.add(best.b.id);
        await this.pair(a, best.b);
      } else {
        // Keep searchers informed of their widening window.
        try {
          a.ws.send(
            JSON.stringify({
              type: "searching",
              tol: tol(a),
              waited_s: Math.floor((now - a.since) / 1000),
            }),
          );
        } catch (_) {}
      }
    }

    if (this.queue.size > 0) {
      await this.state.storage.setAlarm(Date.now() + MM_PASS_MS);
    } else {
      const matches = await this.state.storage.list({ prefix: "match:" });
      for (const [, m] of matches) {
        if (m.reports[0] !== null || m.reports[1] !== null) {
          await this.state.storage.setAlarm(Date.now() + MM_PASS_MS);
          break;
        }
      }
    }
  }

  async pair(a, b) {
    // Random ranked code (M-prefixed, can't collide with 5-letter lobbies)
    // and a random map from the ranked pool.
    const letters = "ABCDEFGHJKLMNPQRSTUVWXYZ";
    let code = "M";
    for (let i = 0; i < 5; i++) {
      code += letters[Math.floor(Math.random() * letters.length)];
    }
    const maps = ["meridian", "caverns"];
    const map = maps[Math.floor(Math.random() * maps.length)];
    await this.state.storage.put(`match:${code}`, {
      ids: [a.id, b.id],
      reports: [null, null],
      created: Date.now(),
    });
    const tell = (e, role, opp) => {
      try {
        e.ws.send(
          JSON.stringify({
            type: "match",
            code,
            role,
            map,
            opp_name: opp.name,
            opp_mmr: Math.round(opp.mmr),
          }),
        );
        e.ws.close(1000, "matched");
      } catch (_) {}
    };
    tell(a, "host", b);
    tell(b, "join", a);
    this.queue.delete(a.id);
    this.queue.delete(b.id);
  }
}

// Cloudflare's rate-limit binding, applied at the edge so a rejected request
// never costs a Durable Object invocation. Guarded on every axis: if the
// binding is absent or throws, we fail open. Losing the limiter must never
// take the relay down, and the per-DO caps above still apply.
async function rateLimited(binding, key) {
  if (!binding || typeof binding.limit !== "function") return false;
  try {
    const { success } = await binding.limit({ key });
    return !success;
  } catch (_) {
    return false;
  }
}

function tooMany() {
  return new Response("slow down\n", {
    status: 429,
    headers: { "retry-after": "10" },
  });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const ip = request.headers.get("CF-Connecting-IP") || "unknown";

    const m = url.pathname.match(/^\/ws\/([A-Z0-9]{4,8})$/);
    if (m) {
      // Blocks code enumeration: each distinct code would otherwise spin up
      // its own DO instance holding a socket.
      if (await rateLimited(env.WS_LIMITER, ip)) return tooMany();
      const id = env.LOBBY.idFromName(m[1]);
      return env.LOBBY.get(id).fetch(request);
    }
    if (url.pathname === "/lobbies") {
      if (await rateLimited(env.LIST_LIMITER, ip)) return tooMany();
      const id = env.DIRECTORY.idFromName("directory");
      return env.DIRECTORY.get(id).fetch(request);
    }
    // Ranked matchmaking: queue socket + result/rating endpoints, all on
    // the singleton Matchmaker DO.
    if (url.pathname === "/queue") {
      if (await rateLimited(env.WS_LIMITER, ip)) return tooMany();
      const id = env.MATCHMAKER.idFromName("matchmaker");
      return env.MATCHMAKER.get(id).fetch(request);
    }
    if (
      url.pathname === "/result" ||
      url.pathname === "/rating" ||
      url.pathname === "/leaderboard"
    ) {
      if (await rateLimited(env.LIST_LIMITER, ip)) return tooMany();
      const id = env.MATCHMAKER.idFromName("matchmaker");
      return env.MATCHMAKER.get(id).fetch(request);
    }
    // RTT probe for the latency half of matchmaking. Answered at the edge —
    // measuring it must not cost a DO hop.
    if (url.pathname === "/ping") {
      return new Response("pong\n", { status: 200 });
    }
    if (url.pathname === "/") {
      return new Response("orion relay up\n", { status: 200 });
    }
    return new Response("not found", { status: 404 });
  },
};
