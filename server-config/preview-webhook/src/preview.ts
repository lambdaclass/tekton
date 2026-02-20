import { execaCommand } from "execa";
import type { TokenProvider } from "./auth.js";

const PREVIEW_BIN = "/run/current-system/sw/bin/preview";

export function prToSlug(_repoName: string, prNumber: number): string {
  return `${prNumber}`;
}

async function runPreview(args: string): Promise<void> {
  console.log(`[preview] Running: ${PREVIEW_BIN} ${args}`);
  try {
    const { stdout, stderr } = await execaCommand(`${PREVIEW_BIN} ${args}`, {
      timeout: 1_200_000, // 20 minute timeout for builds (vertex/Elixir can be slow)
    });
    if (stdout) console.log(`[preview] stdout: ${stdout}`);
    if (stderr) console.log(`[preview] stderr: ${stderr}`);
  } catch (error) {
    console.error(`[preview] Command failed: preview ${args}`, error);
    throw error;
  }
}

export async function listActiveSlugs(): Promise<string[]> {
  try {
    const { stdout } = await execaCommand(`${PREVIEW_BIN} list`, { timeout: 10_000 });
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
  slug: string,
  type: string = "node"
): Promise<void> {
  const typeFlag = type !== "node" ? ` --type ${type}` : "";
  await runPreview(`create ${repo} ${branch} --slug ${slug}${typeFlag}`);
}

export async function updatePreview(slug: string): Promise<void> {
  await runPreview(`update ${slug}`);
}

export async function destroyPreview(slug: string): Promise<void> {
  await runPreview(`destroy ${slug}`);
}

const PREVIEW_LINK_MARKER = "<!-- preview-link -->";

export async function addPreviewLinkToPR(
  repo: string,
  prNumber: number,
  previewUrl: string,
  provider: TokenProvider,
  landingUrl?: string
): Promise<void> {
  console.log(`[preview] Adding preview link to ${repo}#${prNumber}`);
  try {
    const token = await provider.getToken();
    const body = await getPRBody(repo, prNumber, token);
    if (body === null) return;

    const newBody = appendPreviewLink(body, previewUrl, landingUrl);
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
  provider: TokenProvider,
  type: string = "node"
): Promise<void> {
  try {
    const token = await provider.getToken();
    const body = await getPRBody(repo, prNumber, token);
    if (body === null) return;

    // Already has the preview link, nothing to do
    if (body.includes(PREVIEW_LINK_MARKER)) return;

    const previewUrl = `https://${slug}.${previewDomain}`;
    const landingUrl = type === "vertex" ? `https://landing-${slug}.${previewDomain}` : undefined;
    console.log(`[preview] Re-adding missing preview link to ${repo}#${prNumber}`);
    const newBody = appendPreviewLink(body, previewUrl, landingUrl);
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

function appendPreviewLink(body: string, previewUrl: string, landingUrl?: string): string {
  // Remove existing preview link if present
  const markerIdx = body.indexOf(PREVIEW_LINK_MARKER);
  if (markerIdx !== -1) {
    body = body.slice(0, markerIdx).trimEnd();
  }
  let link = `\n\n${PREVIEW_LINK_MARKER}\n---\n**Preview:** ${previewUrl}`;
  if (landingUrl) {
    link += `\n**Landing:** ${landingUrl}`;
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
