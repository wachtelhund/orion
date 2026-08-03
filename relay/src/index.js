// Orion lobby relay: one Durable Object per lobby code, relaying WebSocket
// text frames (the game's RON lockstep messages) between exactly two peers.
// The relay never parses game traffic — it forwards bytes and enforces the
// two-seat lobby shape. Free-tier friendly: tiny, stateless between games.

export class Lobby {
  constructor(state, env) {
    this.host = null;
    this.join = null;
  }

  async fetch(request) {
    const url = new URL(request.url);
    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("expected websocket", { status: 400 });
    }
    const role = url.searchParams.get("role");
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
    } else if (role === "join") {
      if (!this.host) return fail("no such lobby");
      if (this.join) return fail("lobby full");
      this.join = server;
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
    if (url.pathname === "/") {
      return new Response("orion relay up\n", { status: 200 });
    }
    return new Response("not found", { status: 404 });
  },
};
