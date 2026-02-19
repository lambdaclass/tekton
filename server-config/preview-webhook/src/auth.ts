import { createSign } from "node:crypto";
import { readFileSync } from "node:fs";

export interface TokenProvider {
  getToken(): Promise<string>;
  readonly mode: string;
}

export class PatTokenProvider implements TokenProvider {
  readonly mode = "pat";
  constructor(private token: string) {}

  async getToken(): Promise<string> {
    return this.token;
  }
}

export class GitHubAppTokenProvider implements TokenProvider {
  readonly mode = "github-app";

  private appId: string;
  private installationId: string;
  private privateKey: string;

  private cachedToken: string | null = null;
  private tokenExpiresAt: number = 0;

  constructor(appId: string, installationId: string, privateKeyPath: string) {
    this.appId = appId;
    this.installationId = installationId;
    this.privateKey = readFileSync(privateKeyPath, "utf-8");
  }

  async getToken(): Promise<string> {
    // Refresh if token expires within 5 minutes
    if (this.cachedToken && Date.now() < this.tokenExpiresAt - 5 * 60 * 1000) {
      return this.cachedToken;
    }

    const jwt = this.generateJWT();
    const token = await this.exchangeForInstallationToken(jwt);
    return token;
  }

  private generateJWT(): string {
    const now = Math.floor(Date.now() / 1000);
    const header = { alg: "RS256", typ: "JWT" };
    const payload = {
      iat: now - 60, // 60 seconds in the past to allow for clock drift
      exp: now + 10 * 60, // 10 minutes
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

  private async exchangeForInstallationToken(jwt: string): Promise<string> {
    const url = `https://api.github.com/app/installations/${this.installationId}/access_tokens`;
    const res = await fetch(url, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${jwt}`,
        Accept: "application/vnd.github+json",
      },
    });

    if (!res.ok) {
      const body = await res.text();
      throw new Error(`Failed to get installation token (${res.status}): ${body}`);
    }

    const data = (await res.json()) as { token: string; expires_at: string };
    this.cachedToken = data.token;
    this.tokenExpiresAt = new Date(data.expires_at).getTime();

    console.log(`[auth] Obtained installation token (expires: ${data.expires_at})`);
    return data.token;
  }
}

function base64url(str: string): string {
  return Buffer.from(str, "utf-8").toString("base64url");
}
