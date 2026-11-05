import { generateKeyPairSync } from "node:crypto";
import { GitHubAppTokenProvider } from "../src/auth.js";

export const { privateKey: TEST_PRIVATE_KEY } = generateKeyPairSync("rsa", {
  modulusLength: 2048,
  privateKeyEncoding: { type: "pkcs8", format: "pem" },
  publicKeyEncoding: { type: "spki", format: "pem" },
});

export interface RecordedCall {
  method: string;
  url: string;
  body: unknown;
}

/**
 * A fake GitHub API. Every mint returns a token "tok-N" that lives `tokenLifetimeMs`
 * from the fake clock's current time.
 */
export class FakeGitHub {
  calls: RecordedCall[] = [];
  nowMs = Date.parse("2026-01-01T00:00:00Z");
  accountLogin = "lambdaclass";
  tokenLifetimeMs = 60 * 60 * 1000;
  installationStatus = 200;
  mintStatus = 201;
  private minted = 0;

  readonly fetch: typeof fetch = async (input, init) => {
    const url = String(input);
    const method = init?.method ?? "GET";
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    this.calls.push({ method, url, body });

    if (method === "GET" && /\/app\/installations\/\d+$/.test(url)) {
      if (this.installationStatus !== 200) {
        return Response.json({ message: "Not Found" }, { status: this.installationStatus });
      }
      return Response.json({ id: 42, account: { login: this.accountLogin } });
    }
    if (method === "POST" && /\/app\/installations\/\d+\/access_tokens$/.test(url)) {
      if (this.mintStatus !== 201) {
        return Response.json({ message: "Validation Failed" }, { status: this.mintStatus });
      }
      this.minted += 1;
      return Response.json(
        {
          token: `tok-${this.minted}`,
          expires_at: new Date(this.nowMs + this.tokenLifetimeMs).toISOString(),
        },
        { status: 201 }
      );
    }
    return Response.json({ message: "unexpected request" }, { status: 599 });
  };

  mintCalls(): RecordedCall[] {
    return this.calls.filter((c) => c.method === "POST");
  }

  lookupCalls(): RecordedCall[] {
    return this.calls.filter((c) => c.method === "GET");
  }

  provider(): GitHubAppTokenProvider {
    return new GitHubAppTokenProvider({
      appId: "1234",
      installationId: "42",
      privateKey: TEST_PRIVATE_KEY,
      fetch: this.fetch,
      now: () => this.nowMs,
    });
  }
}
