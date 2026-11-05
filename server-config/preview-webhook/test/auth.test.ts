import { test } from "node:test";
import assert from "node:assert/strict";
import { createPublicKey, createVerify } from "node:crypto";
import { GitHubAppTokenProvider, TokenRequestError, parseRepo } from "../src/auth.js";
import { FakeGitHub, TEST_PRIVATE_KEY } from "./helpers.js";

const MIN = 60 * 1000;

test("mints a token narrowed to the repo name and requested permissions", async () => {
  const gh = new FakeGitHub();
  const result = await gh.provider().getToken({
    repo: "lambdaclass/tekton",
    permissions: { pull_requests: "write" },
  });

  assert.equal(result.token, "tok-1");
  assert.equal(result.expiresAt, new Date(gh.nowMs + 60 * MIN).toISOString());
  const [mint] = gh.mintCalls();
  assert.equal(mint.url, "https://api.github.com/app/installations/42/access_tokens");
  assert.deepEqual(mint.body, { repositories: ["tekton"], permissions: { pull_requests: "write" } });
});

test("signs requests with a valid RS256 app JWT", async () => {
  const gh = new FakeGitHub();
  let auth = "";
  const inner = gh.fetch;
  const provider = new GitHubAppTokenProvider({
    appId: "1234",
    installationId: "42",
    privateKey: TEST_PRIVATE_KEY,
    now: () => gh.nowMs,
    fetch: async (input, init) => {
      auth = (init?.headers as Record<string, string>).Authorization;
      return inner(input, init);
    },
  });
  await provider.getToken({ repo: "lambdaclass/tekton", permissions: { contents: "read" } });

  const jwt = auth.replace(/^Bearer /, "");
  const [h, p, sig] = jwt.split(".");
  const verify = createVerify("RSA-SHA256");
  verify.update(`${h}.${p}`);
  assert.ok(verify.verify(createPublicKey(TEST_PRIVATE_KEY), sig, "base64url"));
  const payload = JSON.parse(Buffer.from(p, "base64url").toString());
  const now = gh.nowMs / 1000;
  assert.deepEqual(payload, { iat: now - 60, exp: now + 300, iss: "1234" });
});

test("caches per (repo, permissions)", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();

  const a = await provider.getToken({ repo: "lambdaclass/a", permissions: { contents: "read" } });
  const a2 = await provider.getToken({ repo: "lambdaclass/a", permissions: { contents: "read" } });
  const aWrite = await provider.getToken({ repo: "lambdaclass/a", permissions: { pull_requests: "write" } });
  const b = await provider.getToken({ repo: "lambdaclass/b", permissions: { contents: "read" } });

  assert.equal(a.token, a2.token);
  assert.notEqual(a.token, aWrite.token);
  assert.notEqual(a.token, b.token);
  assert.equal(gh.mintCalls().length, 3);
});

test("permission order does not affect the cache key", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();
  await provider.getToken({ repo: "lambdaclass/a", permissions: { contents: "read", pull_requests: "write" } });
  await provider.getToken({ repo: "lambdaclass/a", permissions: { pull_requests: "write", contents: "read" } });
  assert.equal(gh.mintCalls().length, 1);
  assert.deepEqual(gh.mintCalls()[0].body, {
    repositories: ["a"],
    permissions: { contents: "read", pull_requests: "write" },
  });
});

test("refreshes a cached token within 5 minutes of expiry", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();
  const req = { repo: "lambdaclass/a", permissions: { contents: "read" as const } };

  const first = await provider.getToken(req);
  gh.nowMs += 54 * MIN; // 6 minutes left
  assert.equal((await provider.getToken(req)).token, first.token);
  gh.nowMs += 2 * MIN; // 4 minutes left
  const refreshed = await provider.getToken(req);
  assert.notEqual(refreshed.token, first.token);
  assert.equal(gh.mintCalls().length, 2);
});

test("handoff after build: minTtlSeconds=3000 replaces a token with 20 minutes left", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();
  const req = { repo: "lambdaclass/a", permissions: { contents: "read" as const } };

  const first = await provider.getToken(req);
  gh.nowMs += 40 * MIN; // 20 minutes left

  assert.equal((await provider.getToken(req)).token, first.token);
  const fresh = await provider.getToken({ ...req, minTtlSeconds: 3000 });
  assert.notEqual(fresh.token, first.token);
  assert.equal(fresh.expiresAt, new Date(gh.nowMs + 60 * MIN).toISOString());

  // The fresh token replaced the cache entry.
  assert.equal((await provider.getToken(req)).token, fresh.token);
  assert.equal(gh.mintCalls().length, 2);
});

test("minTtlSeconds is capped at 55 minutes so a fresh token satisfies it", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();
  const req = { repo: "lambdaclass/a", permissions: { contents: "read" as const }, minTtlSeconds: 999_999 };
  const first = await provider.getToken(req);
  assert.equal((await provider.getToken(req)).token, first.token);
  assert.equal(gh.mintCalls().length, 1);
});

test("owner check: installation looked up once, owner compared case-insensitively", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();

  await provider.getToken({ repo: "LambdaClass/foo", permissions: { contents: "read" } });
  await provider.getToken({ repo: "lambdaclass/bar", permissions: { contents: "read" } });
  await assert.rejects(
    provider.getToken({ repo: "other/foo", permissions: { contents: "read" } }),
    TokenRequestError
  );

  assert.equal(gh.lookupCalls().length, 1);
  assert.equal(gh.lookupCalls()[0].url, "https://api.github.com/app/installations/42");
  // The foreign-owner request never reached the mint endpoint.
  assert.deepEqual(gh.mintCalls().map((c) => (c.body as any).repositories), [["foo"], ["bar"]]);
});

test("a failed installation lookup is retried on the next request", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();
  gh.installationStatus = 404;
  await assert.rejects(provider.getToken({ repo: "lambdaclass/a", permissions: { contents: "read" } }), /404/);
  gh.installationStatus = 200;
  await provider.getToken({ repo: "lambdaclass/a", permissions: { contents: "read" } });
  assert.equal(gh.lookupCalls().length, 2);
});

test("mint failures surface GitHub's message only", async () => {
  const gh = new FakeGitHub();
  gh.mintStatus = 422;
  await assert.rejects(
    gh.provider().getToken({ repo: "lambdaclass/a", permissions: { contents: "read" } }),
    /\(422\): Validation Failed$/
  );
});

test("rejects malformed repos without calling GitHub", async () => {
  const gh = new FakeGitHub();
  const provider = gh.provider();
  for (const repo of ["tekton", "a/b/c", "lambdaclass/..", "-x/y", "lambdaclass/", "a b/c"]) {
    await assert.rejects(provider.getToken({ repo, permissions: { contents: "read" } }), TokenRequestError, repo);
  }
  assert.equal(gh.calls.length, 0);
});

test("parseRepo", () => {
  assert.deepEqual(parseRepo("lambdaclass/tekton"), { owner: "lambdaclass", name: "tekton" });
  assert.deepEqual(parseRepo("Lambda-Class/my_repo.js"), { owner: "Lambda-Class", name: "my_repo.js" });
  assert.equal(parseRepo("lambdaclass/."), null);
  assert.equal(parseRepo("lambdaclass/tekton?x=1"), null);
});
