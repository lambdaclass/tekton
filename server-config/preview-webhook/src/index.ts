import Fastify from "fastify";
import { config } from "./config.js";
import { verifySignature, parsePREvent } from "./github.js";
import {
  prToSlug,
  createPreview,
  updatePreview,
  destroyPreview,
  postPRComment,
} from "./preview.js";

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
            const url = `https://${slug}.${config.previewDomain}`;
            await postPRComment(
              repo,
              prNumber,
              `Preview deployment is building.\n\nURL: ${url}\n\nCheck build progress on the server with: \`preview logs ${slug} --follow\``
            );
            break;
          }
          case "synchronize": {
            await updatePreview(slug);
            break;
          }
          case "closed": {
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
