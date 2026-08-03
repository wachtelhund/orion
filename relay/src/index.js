// Orion lobby relay: one Durable Object per lobby code relaying WebSocket
// text frames between exactly two peers, plus a Directory DO listing open
// public lobbies. The relay never parses game traffic.

export class Directory {
  constructor(state) {
    this.state = state;
  }

  async fetch(request) {
    const url = new URL(request.url);
    const lobbies = (await this.state.storage.get("lobbies")) || {};
    const now = Date.now();
    // Prune stale entries (host process died without cleanup).
    for (const [code, l] of Object.entries(lobbies)) {
      if (now - l.created > 15 * 60 * 1000) delete lobbies[code];
    }
    if (url.pathname === "/add") {
      const l = await request.json();
      lobbies[l.code] = { name: l.name, race: l.race, created: now };
      await this.state.storage.put("lobbies", lobbies);
      return new Response("ok");
    }
    if (url.pathname === "/remove") {
      const { code } = await request.json();
      delete lobbies[code];
      await this.state.storage.put("lobbies", lobbies);
      return new Response("ok");
    }
    await this.state.storage.put("lobbies", lobbies);
    const list = Object.entries(lobbies).map(([code, l]) => ({
      code,
      name: l.name,
      race: l.race,
      age_s: Math.floor((now - l.created) / 1000),
    }));
    return new Response(JSON.stringify(list), {
      headers: { "content-type": "application/json" },
    });
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
        this.listed = true;
        const name = (url.searchParams.get("name") || "COMMANDER").slice(0, 16);
        const race = parseInt(url.searchParams.get("race") || "0", 10) || 0;
        try {
          await this.directory().fetch("https://dir/add", {
            method: "POST",
            body: JSON.stringify({ code: this.code, name, race }),
          });
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

    const other = () => (server === this.host ? this.join : this.host);
    server.addEventListener("message", (ev) => {
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

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const m = url.pathname.match(/^\/ws\/([A-Z0-9]{4,8})$/);
    if (m) {
      const id = env.LOBBY.idFromName(m[1]);
      return env.LOBBY.get(id).fetch(request);
    }
    if (url.pathname === "/lobbies") {
      const id = env.DIRECTORY.idFromName("directory");
      return env.DIRECTORY.get(id).fetch(request);
    }
    if (url.pathname === "/") {
      return new Response("orion relay up\n", { status: 200 });
    }
    return new Response("not found", { status: 404 });
  },
};
