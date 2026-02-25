import { execa } from "execa";
import { promises as fs } from "fs";
import type { TokenProvider } from "./auth.js";

export interface PreviewMeta {
  extraHosts?: { prefix: string; routes: unknown[] }[];
}

export async function readPreviewMeta(slug: string): Promise<PreviewMeta | null> {
  try {
    const content = await fs.readFile(`/var/lib/preview-deploys/${slug}.meta`, "utf-8");
    return JSON.parse(content) as PreviewMeta;
  } catch {
    return null;
  }
}

const PREVIEW_BIN = "/run/current-system/sw/bin/preview";

export function prToSlug(repoName: string, prNumber: number): string {
  const name = repoName.split("/").pop() ?? repoName;
  return `${name}-${prNumber}`.toLowerCase().replace(/[^a-z0-9-]/g, "-");
}

async function runPreview(args: string[]): Promise<void> {
  console.log(`[preview] Running: ${PREVIEW_BIN} ${args.join(" ")}`);
  try {
    const { stdout, stderr } = await execa(PREVIEW_BIN, args, {
      timeout: 1_200_000, // 20 minute timeout for builds (Elixir builds can be slow)
    });
    if (stdout) console.log(`[preview] stdout: ${stdout}`);
    if (stderr) console.log(`[preview] stderr: ${stderr}`);
  } catch (error) {
    console.error(`[preview] Command failed: preview ${args.join(" ")}`, error);
    throw error;
  }
}

export async function listActiveSlugs(): Promise<string[]> {
  try {
    const { stdout } = await execa(PREVIEW_BIN, ["list"], { timeout: 10_000 });
    // Parse slugs from the first column of each line (skip header)
    return stdout
      .split("\n")
      .slice(1)
      .map((line) => line.trim().split(/\s+/)[0])
      .filter(Boolean);
  } catch (error) {
    console.error(`[preview] Failed to list active slugs:`, error);
    return [];
  }
}

export async function createPreview(
  repo: string,
  branch: string,
  slug: string
): Promise<void> {
  await runPreview(["create", repo, branch, "--slug", slug]);
}

export async function updatePreview(slug: string): Promise<void> {
  await runPreview(["update", slug]);
}

export async function destroyPreview(slug: string): Promise<void> {
  await runPreview(["destroy", slug]);
}

const PREVIEW_LINK_MARKER = "<!-- preview-link -->";

export async function addPreviewLinkToPR(
  repo: string,
  prNumber: number,
  previewUrl: string,
  provider: TokenProvider,
  extraUrls: string[] = []
): Promise<void> {
  console.log(`[preview] Adding preview link to ${repo}#${prNumber}`);
  try {
    const token = await provider.getToken();
    const body = await getPRBody(repo, prNumber, token);
    if (body === null) return;

    const newBody = appendPreviewLink(body, previewUrl, extraUrls);
    await patchPRBody(repo, prNumber, newBody, token);
    console.log(`[preview] Added preview link to ${repo}#${prNumber}`);
  } catch (error) {
    console.error(`[preview] Failed to add preview link:`, error);
  }
}

export async function ensurePreviewLinkOnPR(
  repo: string,
  prNumber: number,
  slug: string,
  previewDomain: string,
  provider: TokenProvider
): Promise<void> {
  try {
    const token = await provider.getToken();
    const body = await getPRBody(repo, prNumber, token);
    if (body === null) return;

    // Already has the preview link, nothing to do
    if (body.includes(PREVIEW_LINK_MARKER)) return;

    const previewUrl = `https://${slug}.${previewDomain}`;
    const meta = await readPreviewMeta(slug);
    const extraUrls = (meta?.extraHosts ?? []).map(h => `https://${h.prefix}-${slug}.${previewDomain}`);
    console.log(`[preview] Re-adding missing preview link to ${repo}#${prNumber}`);
    const newBody = appendPreviewLink(body, previewUrl, extraUrls);
    await patchPRBody(repo, prNumber, newBody, token);
    console.log(`[preview] Re-added preview link to ${repo}#${prNumber}`);
  } catch (error) {
    console.error(`[preview] Failed to ensure preview link:`, error);
  }
}

async function getPRBody(repo: string, prNumber: number, token: string): Promise<string | null> {
  const res = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}`, {
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
    },
  });
  if (!res.ok) {
    console.error(`[preview] Failed to get PR: ${res.status}`);
    return null;
  }
  const pr = await res.json() as { body: string | null };
  return pr.body ?? "";
}

function appendPreviewLink(body: string, previewUrl: string, extraUrls: string[] = []): string {
  // Remove existing preview link if present
  const markerIdx = body.indexOf(PREVIEW_LINK_MARKER);
  if (markerIdx !== -1) {
    body = body.slice(0, markerIdx).trimEnd();
  }
  let link = `\n\n${PREVIEW_LINK_MARKER}\n---\n**Preview:** ${previewUrl}`;
  for (const url of extraUrls) {
    link += `\n**URL:** ${url}`;
  }
  return body + link;
}

async function patchPRBody(repo: string, prNumber: number, body: string, token: string): Promise<void> {
  const res = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}`, {
    method: "PATCH",
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ body }),
  });
  if (!res.ok) {
    console.error(`[preview] Failed to update PR: ${res.status}`);
  }
}
