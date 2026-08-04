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
    if (url.pathname === "/") {
      return new Response("orion relay up\n", { status: 200 });
    }
    return new Response("not found", { status: 404 });
  },
};
