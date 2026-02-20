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
} from "./preview.js";

// Track active preview slugs so we can re-add links on PR body edits
const activePreviews = new Set<string>();

async function main(): Promise<void> {
  console.log("Starting preview webhook server...");

  const fastify = Fastify({
    logger: false,
    bodyLimit: 1_048_576, // 1MB
  });

  // Need raw body for signature verification
  fastify.addContentTypeParser(
    "application/json",
    { parseAs: "string" },
    (_req, body, done) => {
      try {
        done(null, JSON.parse(body as string));
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

    // Verify webhook signature
    const rawBody = JSON.stringify(request.body);
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

    const type = config.vertexRepos.includes(repo) ? "vertex" : "node";

    console.log(
      `[webhook] PR #${prNumber} ${event.action} on ${repo} (branch: ${branch}, type: ${type})`
    );

    // Respond 202 immediately, process in background
    void reply.code(202).send({ status: "accepted", slug, type });

    // Process asynchronously
    setImmediate(async () => {
      try {
        switch (event.action) {
          case "opened":
          case "reopened": {
            await createPreview(repo, branch, slug, type);
            activePreviews.add(slug);
            const url = `https://${slug}.${config.previewDomain}`;
            if (type === "vertex") {
              const landingUrl = `https://landing-${slug}.${config.previewDomain}`;
              await addPreviewLinkToPR(repo, prNumber, url, tokenProvider, landingUrl);
            } else {
              await addPreviewLinkToPR(repo, prNumber, url, tokenProvider);
            }
            break;
          }
          case "synchronize": {
            await updatePreview(slug);
            break;
          }
          case "edited": {
            if (activePreviews.has(slug)) {
              await ensurePreviewLinkOnPR(repo, prNumber, slug, config.previewDomain, tokenProvider, type);
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
