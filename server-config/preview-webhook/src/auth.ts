import { createPrivateKey, createSign } from "node:crypto";
import { readFileSync } from "node:fs";

export type Permission = "read" | "write";

export interface TokenRequest {
  /** "owner/name". The owner must match the App installation's account. */
  repo: string;
  permissions: Record<string, Permission>;
  /** Minimum remaining lifetime of the returned token. Default 300, capped at 3300. */
  minTtlSeconds?: number;
}

export interface InstallationToken {
  token: string;
  expiresAt: string;
}

export interface TokenProvider {
  getToken(opts: TokenRequest): Promise<InstallationToken>;
}

/** The request is invalid for this installation (bad repo or foreign owner). */
export class TokenRequestError extends Error {}

export const DEFAULT_MIN_TTL_SECONDS = 5 * 60;
// Installation tokens live 60 minutes; never demand more than a fresh one can give.
export const MAX_MIN_TTL_SECONDS = 55 * 60;

const REPO_RE = /^([A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)\/([A-Za-z0-9._-]+)$/;

export function parseRepo(repo: string): { owner: string; name: string } | null {
  const m = REPO_RE.exec(repo);
  if (!m || m[2] === "." || m[2] === "..") return null;
  return { owner: m[1], name: m[2] };
}

export interface GitHubAppOptions {
  appId: string;
  installationId: string;
  privateKey: string;
  apiUrl?: string;
  fetch?: typeof fetch;
  now?: () => number;
}

interface CachedToken {
  token: string;
  expiresAtMs: number;
  expiresAt: string;
}

export class GitHubAppTokenProvider implements TokenProvider {
  private readonly appId: string;
  private readonly installationId: string;
  private readonly privateKey: string;
  private readonly apiUrl: string;
  private readonly fetchImpl: typeof fetch;
  private readonly now: () => number;

  private accountLogin: Promise<string> | null = null;
  private readonly cache = new Map<string, CachedToken>();

  constructor(opts: GitHubAppOptions) {
    this.appId = opts.appId;
    this.installationId = opts.installationId;
    this.privateKey = opts.privateKey;
    this.apiUrl = opts.apiUrl ?? "https://api.github.com";
    this.fetchImpl = opts.fetch ?? fetch;
    this.now = opts.now ?? Date.now;
  }

  /** Reads and validates the key file. Errors never include key material. */
  static fromKeyFile(appId: string, installationId: string, privateKeyPath: string): GitHubAppTokenProvider {
    let privateKey: string;
    try {
      privateKey = readFileSync(privateKeyPath, "utf-8");
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code ?? "unknown error";
      throw new Error(`Cannot read GitHub App private key at ${privateKeyPath} (${code})`);
    }
    try {
      const key = createPrivateKey(privateKey);
      if (key.asymmetricKeyType !== "rsa") throw new Error("not RSA");
    } catch {
      throw new Error(`GitHub App private key at ${privateKeyPath} is not a valid RSA PEM private key`);
    }
    return new GitHubAppTokenProvider({ appId, installationId, privateKey });
  }

  async getToken(opts: TokenRequest): Promise<InstallationToken> {
    const parsed = parseRepo(opts.repo);
    if (!parsed) {
      throw new TokenRequestError(`Invalid repo "${opts.repo}", expected owner/name`);
    }
    const permEntries = Object.entries(opts.permissions).sort(([a], [b]) => a.localeCompare(b));
    if (permEntries.length === 0) {
      throw new TokenRequestError("At least one permission is required");
    }

    const login = await this.getAccountLogin();
    if (parsed.owner.toLowerCase() !== login.toLowerCase()) {
      throw new TokenRequestError(
        `Repo ${opts.repo} is not owned by the App installation account (${login})`
      );
    }

    const minTtl = Math.min(
      Math.max(opts.minTtlSeconds ?? DEFAULT_MIN_TTL_SECONDS, 0),
      MAX_MIN_TTL_SECONDS
    );
    const key = `${parsed.name.toLowerCase()}|${permEntries.map(([k, v]) => `${k}:${v}`).join(",")}`;
    const cached = this.cache.get(key);
    if (cached && cached.expiresAtMs - this.now() >= minTtl * 1000) {
      return { token: cached.token, expiresAt: cached.expiresAt };
    }

    const minted = await this.mint(parsed.name, Object.fromEntries(permEntries));
    this.cache.set(key, minted);
    console.log(
      `[auth] Minted installation token for ${opts.repo} ` +
        `(${permEntries.map(([k, v]) => `${k}:${v}`).join(", ")}; expires ${minted.expiresAt})`
    );
    return { token: minted.token, expiresAt: minted.expiresAt };
  }

  private getAccountLogin(): Promise<string> {
    if (!this.accountLogin) {
      this.accountLogin = this.fetchAccountLogin().catch((err) => {
        this.accountLogin = null; // retry on the next request
        throw err;
      });
    }
    return this.accountLogin;
  }

  private async fetchAccountLogin(): Promise<string> {
    const res = await this.fetchImpl(`${this.apiUrl}/app/installations/${this.installationId}`, {
      headers: this.appHeaders(),
    });
    if (!res.ok) {
      throw new Error(`Failed to look up App installation ${this.installationId} (${res.status}): ${await errorMessage(res)}`);
    }
    const data = (await res.json()) as { account?: { login?: string } };
    const login = data.account?.login;
    if (!login) {
      throw new Error(`App installation ${this.installationId} has no account login`);
    }
    return login;
  }

  private async mint(repoName: string, permissions: Record<string, Permission>): Promise<CachedToken> {
    const res = await this.fetchImpl(
      `${this.apiUrl}/app/installations/${this.installationId}/access_tokens`,
      {
        method: "POST",
        headers: { ...this.appHeaders(), "Content-Type": "application/json" },
        body: JSON.stringify({ repositories: [repoName], permissions }),
      }
    );
    if (!res.ok) {
      throw new Error(`Failed to get installation token (${res.status}): ${await errorMessage(res)}`);
    }
    const data = (await res.json()) as { token: string; expires_at: string };
    const expiresAtMs = new Date(data.expires_at).getTime();
    if (!data.token || Number.isNaN(expiresAtMs)) {
      throw new Error("Installation token response is missing token or expires_at");
    }
    return { token: data.token, expiresAtMs, expiresAt: data.expires_at };
  }

  private appHeaders(): Record<string, string> {
    return {
      Authorization: `Bearer ${this.generateJWT()}`,
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
    };
  }

  private generateJWT(): string {
    const now = Math.floor(this.now() / 1000);
    const header = { alg: "RS256", typ: "JWT" };
    const payload = {
      iat: now - 60, // 60 seconds in the past to allow for clock drift
      exp: now + 5 * 60, // 5 minutes (leave headroom for clock drift; GitHub max is 10)
      iss: this.appId,
    };

    const encodedHeader = base64url(JSON.stringify(header));
    const encodedPayload = base64url(JSON.stringify(payload));
    const signingInput = `${encodedHeader}.${encodedPayload}`;

    const sign = createSign("RSA-SHA256");
    sign.update(signingInput);
    const signature = sign.sign(this.privateKey, "base64url");

    return `${signingInput}.${signature}`;
  }
}

// GitHub error bodies are JSON with a "message"; keep only that so nothing else is logged.
async function errorMessage(res: Response): Promise<string> {
  try {
    const body = (await res.json()) as { message?: unknown };
    return typeof body.message === "string" ? body.message : "no message";
  } catch {
    return "unparseable response";
  }
}

function base64url(str: string): string {
  return Buffer.from(str, "utf-8").toString("base64url");
}
