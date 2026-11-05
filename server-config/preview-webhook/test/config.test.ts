import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createTokenProvider } from "../src/config.js";
import { GitHubAppTokenProvider } from "../src/auth.js";
import { TEST_PRIVATE_KEY } from "./helpers.js";

const dir = mkdtempSync(join(tmpdir(), "preview-webhook-test-"));
const keyPath = join(dir, "app.pem");
writeFileSync(keyPath, TEST_PRIVATE_KEY);

const validEnv = {
  GITHUB_APP_ID: "1234",
  GITHUB_APP_INSTALLATION_ID: "42",
  GITHUB_APP_PRIVATE_KEY_PATH: keyPath,
};

test("creates the App provider from valid config", () => {
  assert.ok(createTokenProvider(validEnv) instanceof GitHubAppTokenProvider);
});

test("fails when GITHUB_TOKEN is set, even if empty", () => {
  for (const value of ["ghp_leftover", ""]) {
    assert.throws(() => createTokenProvider({ ...validEnv, GITHUB_TOKEN: value }), /GITHUB_TOKEN is set/);
  }
});

test("fails when any App variable is missing", () => {
  for (const name of Object.keys(validEnv)) {
    const env: Record<string, string | undefined> = { ...validEnv, [name]: undefined };
    assert.throws(() => createTokenProvider(env), new RegExp(`Missing GitHub App config: ${name}`));
  }
});

test("fails when the IDs are not numeric", () => {
  assert.throws(() => createTokenProvider({ ...validEnv, GITHUB_APP_INSTALLATION_ID: "42/../1" }), /must be numeric/);
});

test("fails when the key is unreadable", () => {
  assert.throws(
    () => createTokenProvider({ ...validEnv, GITHUB_APP_PRIVATE_KEY_PATH: join(dir, "missing.pem") }),
    /Cannot read GitHub App private key .*ENOENT/
  );
});

test("fails when the key is not a valid PEM, without echoing its contents", () => {
  const bad = join(dir, "bad.pem");
  writeFileSync(bad, "-----BEGIN RSA PRIVATE KEY-----\nSECRETGARBAGE\n-----END RSA PRIVATE KEY-----\n");
  assert.throws(
    () => createTokenProvider({ ...validEnv, GITHUB_APP_PRIVATE_KEY_PATH: bad }),
    (err: Error) => /not a valid RSA PEM/.test(err.message) && !err.message.includes("SECRETGARBAGE")
  );
});
