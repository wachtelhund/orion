// Stub harness for relay/src/index.js — no deps, runs on plain node.
// Verifies the Directory storage behaviour and the per-connection meter.

// --- minimal Workers runtime stubs -----------------------------------------
globalThis.Response = class {
  constructor(body, init = {}) {
    this.body = body;
    this.status = init.status ?? 200;
    this.ok = this.status >= 200 && this.status < 300;
    this.headers = new Map(Object.entries(init.headers || {}));
    this.webSocket = init.webSocket;
  }
  async text() { return this.body; }
  async json() { return JSON.parse(this.body); }
};

const sockets = [];
class FakeSocket {
  constructor(kind) { this.kind = kind; this.h = {}; this.sent = []; this.closed = null; }
  accept() {}
  addEventListener(t, fn) { (this.h[t] ||= []).push(fn); }
  send(d) { if (this.closed) throw new Error("closed"); this.sent.push(d); }
  close(code, reason) {
    if (this.closed) return;
    this.closed = { code, reason };
    (this.h.close || []).forEach((fn) => fn());
  }
  emit(data) { (this.h.message || []).forEach((fn) => fn({ data })); }
}
globalThis.WebSocketPair = function () {
  const c = new FakeSocket("client"), s = new FakeSocket("server");
  sockets.push(s);
  return { 0: c, 1: s };
};

class FakeStorage {
  constructor() { this.map = new Map(); this.gets = 0; this.puts = 0; }
  async get(k) { this.gets++; return structuredClone(this.map.get(k)); }
  async put(k, v) { this.puts++; this.map.set(k, structuredClone(v)); }
}

const req = (path, body) => ({
  url: `https://dir${path}`,
  headers: { get: () => null },
  json: async () => body,
});
const wsReq = (path, qs) => ({
  url: `https://relay${path}?${qs}`,
  headers: { get: (h) => (h === "Upgrade" ? "websocket" : null) },
});

// --- test plumbing ---------------------------------------------------------
let pass = 0, fail = 0;
const ok = (cond, label) => {
  if (cond) { pass++; console.log(`  ok   ${label}`); }
  else { fail++; console.log(`  FAIL ${label}`); }
};

const { Directory, Lobby } = await import("./src/index.js");

// --- Directory -------------------------------------------------------------
console.log("Directory");
{
  const st = new FakeStorage();
  const d = new Directory({ storage: st });

  const r1 = await d.fetch(req("/lobbies"));
  ok((await r1.text()) === "[]", "empty list returns []");
  ok(st.puts === 0, "list path performs no storage write (was 1 per poll)");

  await d.fetch(req("/add", { code: "ABCDE", name: "HAMPUS", race: 1 }));
  const list = JSON.parse(await (await d.fetch(req("/lobbies"))).text());
  ok(list.length === 1 && list[0].code === "ABCDE", "added lobby is listed");
  ok(list[0].name === "HAMPUS" && list[0].race === 1, "name/race round-trip");

  const getsBefore = st.gets;
  await d.fetch(req("/lobbies"));
  ok(st.gets === getsBefore, "second poll within 2s is served from memo");

  await d.fetch(req("/remove", { code: "ABCDE" }));
  const after = JSON.parse(await (await d.fetch(req("/lobbies"))).text());
  ok(after.length === 0, "remove invalidates the memo immediately");
}

// cap
{
  const st = new FakeStorage();
  const d = new Directory({ storage: st });
  for (let i = 0; i < 200; i++) {
    await d.fetch(req("/add", { code: `C${i}`, name: "X", race: 0 }));
  }
  const over = await d.fetch(req("/add", { code: "NEW1", name: "X", race: 0 }));
  ok(over.status === 429, "201st distinct lobby is refused (directory cap)");
  const again = await d.fetch(req("/add", { code: "C7", name: "X", race: 0 }));
  ok(again.status === 200, "existing host can re-list despite the cap");
}

// prune
{
  const st = new FakeStorage();
  const d = new Directory({ storage: st });
  const realNow = Date.now;
  await d.fetch(req("/add", { code: "OLD01", name: "X", race: 0 }));
  Date.now = () => realNow() + 16 * 60 * 1000; // past the 15-min TTL
  const putsBefore = st.puts;
  const list = JSON.parse(await (await d.fetch(req("/lobbies"))).text());
  ok(list.length === 0, "stale lobby is pruned from the list");
  ok(st.puts === putsBefore + 1, "prune writes exactly once");
  Date.now = realNow;
}

// --- Lobby meter -----------------------------------------------------------
console.log("Lobby relay caps");
const mkPair = async () => {
  const l = new Lobby({}, { DIRECTORY: null });
  await l.fetch(wsReq("/ws/ABCDE", "role=host&private=1"));
  const host = sockets[sockets.length - 1];
  await l.fetch(wsReq("/ws/ABCDE", "role=join&private=1"));
  const join = sockets[sockets.length - 1];
  return { l, host, join };
};

// Relay control frames ({relay_slot}/{relay_fill}) ride the same socket;
// game-frame assertions must not count them.
const game = (sock) => sock.sent.filter((d) => !/^\{"relay_/.test(d));

{
  const { host, join } = await mkPair();
  host.emit("hello");
  const g = game(join);
  ok(g.length === 1 && g[0] === "hello", "normal frame relays to peer");
  ok(host.closed === null, "normal frame does not close");
}

{
  // 24 Hz lockstep for 30 simulated seconds must never trip a limit.
  const { host, join } = await mkPair();
  const realNow = Date.now;
  let t = realNow();
  Date.now = () => t;
  for (let i = 0; i < 24 * 30; i++) { t += 1000 / 24; host.emit("Cmd(...)"); }
  Date.now = realNow;
  ok(host.closed === null, "24 Hz traffic for 30s stays under the rate cap");
  ok(game(join).length === 720, "all 720 lockstep frames relayed");
}

{
  const { host } = await mkPair();
  host.emit("x".repeat(200 * 1024)); // over the 160 KiB handshake allowance
  ok(host.closed?.reason === "frame too large", "oversized frame is rejected");
}

{
  // Burst is 300 tokens; 400 messages in the same instant must trip.
  const { host, join } = await mkPair();
  const realNow = Date.now;
  const frozen = realNow(); // freeze at "now", so no refill and no backwards step
  Date.now = () => frozen;
  for (let i = 0; i < 400; i++) host.emit("f");
  Date.now = realNow;
  ok(host.closed?.reason === "message rate exceeded", "instant burst of 400 trips the bucket");
  ok(game(join).length === 300, "exactly the 300-token burst got through");
}

{
  const { host } = await mkPair();
  const realNow = Date.now;
  let t = realNow();
  Date.now = () => t;
  let closedBy = null;
  // 16 KB frames at the sustained rate until the 128 MB budget runs out.
  for (let i = 0; i < 12000 && !host.closed; i++) { t += 10; host.emit("x".repeat(16 * 1024)); }
  closedBy = host.closed?.reason;
  Date.now = realNow;
  ok(closedBy === "byte budget exhausted", `byte budget terminates a data pipe (got: ${closedBy})`);
}

// --- Team rooms (4 seats) ---------------------------------------------------
console.log("Team rooms");
{
  const l = new Lobby({}, { DIRECTORY: null });
  await l.fetch(wsReq("/ws/TEAMS", "role=host&private=1&slots=4"));
  const host = sockets[sockets.length - 1];
  const joiners = [];
  for (let k = 0; k < 3; k++) {
    await l.fetch(wsReq("/ws/TEAMS", "role=join&private=1"));
    joiners.push(sockets[sockets.length - 1]);
  }
  const slotOf = (sock) => {
    const f = sock.sent.find((d) => /^\{"relay_slot/.test(d));
    return f ? JSON.parse(f).relay_slot : -1;
  };
  ok(slotOf(host) === 0, "host takes seat 0");
  ok(
    joiners.map(slotOf).join(",") === "1,2,3",
    "joiners seat 1..3 in arrival order",
  );
  const before = joiners.map((j) => game(j).length);
  host.emit("frame-from-host");
  ok(
    joiners.every((j, i) => game(j).length === before[i] + 1),
    "host frame broadcasts to all three joiners",
  );
  const h0 = game(host).length;
  joiners[1].emit("frame-from-seat-2");
  ok(game(host).length === h0 + 1, "joiner frame reaches the host");
  ok(game(joiners[0]).length === before[0] + 2, "and the other joiners");
  // A fifth connection is refused.
  await l.fetch(wsReq("/ws/TEAMS", "role=join&private=1"));
  const fifth = sockets[sockets.length - 1];
  ok(fifth.closed?.reason === "lobby full", "seat five is refused");
  // Any departure closes the room.
  joiners[2].close(1000, "bye");
  ok(host.closed !== null, "a leaver closes the room for everyone");
}

console.log(`\n${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
