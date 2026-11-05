import type { FastifyInstance } from "fastify";
import { type TokenProvider, TokenRequestError, parseRepo } from "./auth.js";

export interface InternalTokenDeps {
  tokenProvider: TokenProvider;
  allowedRepos: readonly string[];
}

/**
 * GET /internal/token?repo=owner/name[&min_ttl=seconds]
 * Returns a contents:read installation token for one repo, to direct localhost callers only.
 */
export function registerInternalTokenRoute(fastify: FastifyInstance, deps: InternalTokenDeps): void {
  fastify.get("/internal/token", async (request, reply) => {
    // Only allow direct localhost requests (not proxied through Caddy).
    // Caddy reverse-proxies all paths on webhook.DOMAIN to localhost:3100,
    // so request.ip is always 127.0.0.1.  We reject proxied requests by
    // checking for X-Forwarded-For which Caddy always sets.
    const remoteAddr = request.ip;
    const isLocalhost = remoteAddr === "127.0.0.1" || remoteAddr === "::1" || remoteAddr === "::ffff:127.0.0.1";
    const isProxied = !!request.headers["x-forwarded-for"];
    if (!isLocalhost || isProxied) {
      return reply.code(403).send({ error: "Forbidden: localhost only" });
    }

    const query = request.query as Record<string, unknown>;
    const repo = query["repo"];
    if (typeof repo !== "string" || !parseRepo(repo)) {
      return reply.code(400).send({ error: "repo must be owner/name" });
    }
    if (
      deps.allowedRepos.length > 0 &&
      !deps.allowedRepos.some((r) => r.toLowerCase() === repo.toLowerCase())
    ) {
      return reply.code(403).send({ error: "repo not in allowlist" });
    }

    let minTtlSeconds: number | undefined;
    const minTtl = query["min_ttl"];
    if (minTtl !== undefined) {
      if (typeof minTtl !== "string" || !/^\d{1,6}$/.test(minTtl)) {
        return reply.code(400).send({ error: "min_ttl must be a non-negative integer" });
      }
      minTtlSeconds = Number(minTtl);
    }

    try {
      const { token, expiresAt } = await deps.tokenProvider.getToken({
        repo,
        permissions: { contents: "read" },
        minTtlSeconds,
      });
      return { token, expires_at: expiresAt };
    } catch (error) {
      if (error instanceof TokenRequestError) {
        return reply.code(403).send({ error: error.message });
      }
      console.error(`[internal/token] Failed to get token for ${repo}:`, (error as Error).message);
      return reply.code(502).send({ error: "Failed to obtain token" });
    }
  });
}
