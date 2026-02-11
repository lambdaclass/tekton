import { createHmac, timingSafeEqual } from "node:crypto";

export function verifySignature(
  payload: string,
  signature: string | undefined,
  secret: string
): boolean {
  if (!signature) return false;

  const expected =
    "sha256=" +
    createHmac("sha256", secret).update(payload, "utf8").digest("hex");

  if (expected.length !== signature.length) return false;

  return timingSafeEqual(Buffer.from(expected), Buffer.from(signature));
}

export interface PullRequestEvent {
  action: string;
  number: number;
  repository: {
    full_name: string;
    name: string;
  };
  pull_request: {
    head: {
      ref: string;
    };
    merged: boolean;
  };
}

export function parsePREvent(body: unknown): PullRequestEvent | null {
  const event = body as Record<string, unknown>;
  if (
    !event ||
    typeof event.action !== "string" ||
    typeof event.number !== "number" ||
    !event.repository ||
    !event.pull_request
  ) {
    return null;
  }
  return event as unknown as PullRequestEvent;
}
