import { execaCommand } from "execa";

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
  token: string,
  landingUrl?: string
): Promise<void> {
  console.log(`[preview] Adding preview link to ${repo}#${prNumber}`);
  try {
    // Get current PR body
    const getRes = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}`, {
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/vnd.github+json",
      },
    });
    if (!getRes.ok) {
      console.error(`[preview] Failed to get PR: ${getRes.status}`);
      return;
    }
    const pr = await getRes.json() as { body: string | null };
    let body = pr.body ?? "";

    // Remove existing preview link if present
    const markerIdx = body.indexOf(PREVIEW_LINK_MARKER);
    if (markerIdx !== -1) {
      body = body.slice(0, markerIdx).trimEnd();
    }

    // Append preview link
    body += `\n\n${PREVIEW_LINK_MARKER}\n---\n**Preview:** ${previewUrl}`;
    if (landingUrl) {
      body += `\n**Landing:** ${landingUrl}`;
    }

    // Update PR
    const patchRes = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}`, {
      method: "PATCH",
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/vnd.github+json",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ body }),
    });
    if (!patchRes.ok) {
      console.error(`[preview] Failed to update PR: ${patchRes.status}`);
      return;
    }
    console.log(`[preview] Added preview link to ${repo}#${prNumber}`);
  } catch (error) {
    console.error(`[preview] Failed to add preview link:`, error);
  }
}
