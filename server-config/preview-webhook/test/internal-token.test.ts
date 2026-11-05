import { test } from "node:test";
import assert from "node:assert/strict";
import Fastify from "fastify";
import type { TokenProvider, TokenRequest } from "../src/auth.js";
import { registerInternalTokenRoute } from "../src/internal-token.js";
import { FakeGitHub } from "./helpers.js";

function server(tokenProvider: TokenProvider, allowedRepos: string[] = []) {
  const fastify = Fastify({ logger: false });
  registerInternalTokenRoute(fastify, { tokenProvider, allowedRepos });
  return fastify;
}

function recordingProvider() {
  const requests: TokenRequest[] = [];
  const provider: TokenProvider = {
    async getToken(opts) {
      requests.push(opts);
      return { token: "tok", expiresAt: "2026-01-01T01:00:00Z" };
    },
  };
  return { provider, requests };
}

test("returns a contents:read token for the repo", async () => {
  const { provider, requests } = recordingProvider();
  const res = await server(provider).inject({ url: "/internal/token?repo=lambdaclass%2Ftekton" });
  assert.equal(res.statusCode, 200);
  assert.deepEqual(res.json(), { token: "tok", expires_at: "2026-01-01T01:00:00Z" });
  assert.deepEqual(requests, [
    { repo: "lambdaclass/tekton", permissions: { contents: "read" }, minTtlSeconds: undefined },
  ]);
});

test("passes min_ttl through to the provider", async () => {
  const { provider, requests } = recordingProvider();
  const res = await server(provider).inject({ url: "/internal/token?repo=lambdaclass/tekton&min_ttl=3000" });
  assert.equal(res.statusCode, 200);
  assert.equal(requests[0].minTtlSeconds, 3000);
});

test("min_ttl=3000 reaches the real provider and forces a fresh token", async () => {
  const gh = new FakeGitHub();
  const app = server(gh.provider());
  const first = (await app.inject({ url: "/internal/token?repo=lambdaclass/a" })).json();
  gh.nowMs += 40 * 60 * 1000;
  const same = (await app.inject({ url: "/internal/token?repo=lambdaclass/a" })).json();
  const fresh = (await app.inject({ url: "/internal/token?repo=lambdaclass/a&min_ttl=3000" })).json();
  assert.equal(same.token, first.token);
  assert.notEqual(fresh.token, first.token);
});

test("rejects missing, malformed, and repeated repo", async () => {
  const { provider, requests } = recordingProvider();
  const app = server(provider);
  for (const qs of ["", "?repo=", "?repo=tekton", "?repo=a/b/c", "?repo=lambdaclass/..", "?repo=a/b&repo=c/d"]) {
    const res = await app.inject({ url: `/internal/token${qs}` });
    assert.equal(res.statusCode, 400, qs);
  }
  assert.equal(requests.length, 0);
});

test("rejects a malformed min_ttl", async () => {
  const { provider } = recordingProvider();
  const app = server(provider);
  for (const v of ["-1", "abc", "1.5", "9999999"]) {
    const res = await app.inject({ url: `/internal/token?repo=lambdaclass/a&min_ttl=${v}` });
    assert.equal(res.statusCode, 400, v);
  }
});

test("enforces a non-empty allowlist (case-insensitive)", async () => {
  const { provider } = recordingProvider();
  const app = server(provider, ["lambdaclass/tekton"]);
  assert.equal((await app.inject({ url: "/internal/token?repo=lambdaclass/other" })).statusCode, 403);
  assert.equal((await app.inject({ url: "/internal/token?repo=LambdaClass/Tekton" })).statusCode, 200);
});

test("rejects proxied and non-localhost requests", async () => {
  const { provider, requests } = recordingProvider();
  const app = server(provider);
  const proxied = await app.inject({
    url: "/internal/token?repo=lambdaclass/a",
    headers: { "x-forwarded-for": "203.0.113.5" },
  });
  const container = await app.inject({ url: "/internal/token?repo=lambdaclass/a", remoteAddress: "10.100.0.2" });
  assert.equal(proxied.statusCode, 403);
  assert.equal(container.statusCode, 403);
  assert.equal(requests.length, 0);
});

test("rejects a repo whose owner is not the installation account", async () => {
  const gh = new FakeGitHub();
  const res = await server(gh.provider()).inject({ url: "/internal/token?repo=other/tekton" });
  assert.equal(res.statusCode, 403);
  assert.match(res.json().error, /not owned by the App installation account/);
  assert.equal(gh.mintCalls().length, 0);
});

test("upstream failures return 502 without details", async () => {
  const gh = new FakeGitHub();
  gh.mintStatus = 500;
  const res = await server(gh.provider()).inject({ url: "/internal/token?repo=lambdaclass/a" });
  assert.equal(res.statusCode, 502);
  assert.deepEqual(res.json(), { error: "Failed to obtain token" });
});
