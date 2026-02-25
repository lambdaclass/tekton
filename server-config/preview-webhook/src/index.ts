import Fastify from "fastify";
import { config, tokenProvider } from "./config.js";
import { verifySignature, parsePREvent } from "./github.js";
import {
  prToSlug,
  createPreview,
  updatePreview,
  destroyPreview,
  addPreviewLinkToPR,
  ensurePreviewLinkOnPR,
  listActiveSlugs,
  readPreviewMeta,
} from "./preview.js";

// Track active preview slugs so we can re-add links on PR body edits
const activePreviews = new Set<string>();

// Keyed on the raw IncomingMessage so the route handler can retrieve the original bytes
// without a JSON round-trip that could normalise unicode escapes and break HMAC.
import type { IncomingMessage } from "http";
const rawBodyStore = new WeakMap<IncomingMessage, string>();

async function main(): Promise<void> {
  console.log("Starting preview webhook server...");

  const fastify = Fastify({
    logger: false,
    bodyLimit: 1_048_576, // 1MB
  });

  // Parse JSON but keep the original byte string for signature verification.
  // GitHub HMAC-SHA256 is computed over the exact bytes it sends; re-serialising
  // with JSON.stringify can differ (e.g. unicode escapes: \u003c vs <).
  fastify.addContentTypeParser(
    "application/json",
    { parseAs: "string" },
    (req, body, done) => {
      try {
        const raw = body as string;
        rawBodyStore.set(req.raw, raw);
        done(null, JSON.parse(raw));
      } catch (err) {
        done(err as Error, undefined);
      }
    }
  );

  fastify.get("/health", async () => {
    return { status: "ok", uptime: process.uptime() };
  });

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

    try {
      const token = await tokenProvider.getToken();
      return { token, mode: tokenProvider.mode };
    } catch (error) {
      console.error("[internal/token] Failed to get token:", error);
      return reply.code(500).send({ error: "Failed to obtain token" });
    }
  });

  fastify.post("/webhook/github", async (request, reply) => {
    const eventType = request.headers["x-github-event"];
    const signature = request.headers["x-hub-signature-256"] as
      | string
      | undefined;

    // Verify webhook signature using the original bytes (not re-serialised JSON)
    const rawBody = rawBodyStore.get(request.raw) ?? JSON.stringify(request.body);
    if (!verifySignature(rawBody, signature, config.githubWebhookSecret)) {
      console.warn("[webhook] Invalid signature");
      return reply.code(401).send({ error: "Invalid signature" });
    }

    // Only handle pull_request events
    if (eventType !== "pull_request") {
      return reply.code(200).send({ status: "ignored", event: eventType });
    }

    const event = parsePREvent(request.body);
    if (!event) {
      return reply.code(400).send({ error: "Invalid pull_request payload" });
    }

    const repo = event.repository.full_name;
    const repoName = event.repository.name;
    const prNumber = event.number;
    const branch = event.pull_request.head.ref;
    const slug = prToSlug(repoName, prNumber);

    // Check repo allowlist
    if (
      config.allowedRepos.length > 0 &&
      !config.allowedRepos.includes(repo)
    ) {
      console.log(`[webhook] Repo ${repo} not in allowlist, ignoring`);
      return reply.code(200).send({ status: "ignored", reason: "not_allowed" });
    }

    console.log(
      `[webhook] PR #${prNumber} ${event.action} on ${repo} (branch: ${branch})`
    );

    // Respond 202 immediately, process in background
    void reply.code(202).send({ status: "accepted", slug });

    // Process asynchronously
    setImmediate(async () => {
      try {
        switch (event.action) {
          case "opened":
          case "reopened": {
            await createPreview(repo, branch, slug);
            activePreviews.add(slug);
            const url = `https://${slug}.${config.previewDomain}`;
            const meta = await readPreviewMeta(slug);
            const extraUrls = (meta?.extraHosts ?? []).map(
              (h) => `https://${h.prefix}-${slug}.${config.previewDomain}`
            );
            await addPreviewLinkToPR(repo, prNumber, url, tokenProvider, extraUrls);
            break;
          }
          case "synchronize": {
            await updatePreview(slug);
            break;
          }
          case "edited": {
            if (activePreviews.has(slug)) {
              await ensurePreviewLinkOnPR(repo, prNumber, slug, config.previewDomain, tokenProvider);
            }
            break;
          }
          case "closed": {
            activePreviews.delete(slug);
            await destroyPreview(slug);
            break;
          }
          default:
            console.log(`[webhook] Ignoring action: ${event.action}`);
        }
      } catch (error) {
        console.error(
          `[webhook] Error processing PR #${prNumber} ${event.action}:`,
          error
        );
      }
    });
  });

  // Populate active previews from existing containers on startup
  const existingSlugs = await listActiveSlugs();
  for (const slug of existingSlugs) {
    activePreviews.add(slug);
  }
  if (existingSlugs.length > 0) {
    console.log(`[preview] Loaded ${existingSlugs.length} active previews: ${existingSlugs.join(", ")}`);
  }

  await fastify.listen({ port: config.port, host: "0.0.0.0" });
  console.log(`Webhook server listening on http://0.0.0.0:${config.port}`);

  // Graceful shutdown
  const shutdown = async () => {
    console.log("Shutting down...");
    await fastify.close();
    process.exit(0);
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);

  console.log("Preview webhook ready!");
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
