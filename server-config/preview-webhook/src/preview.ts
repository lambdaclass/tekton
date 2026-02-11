import { execaCommand } from "execa";

export function prToSlug(repoName: string, prNumber: number): string {
  return `${repoName}-pr-${prNumber}`;
}

async function runPreview(args: string): Promise<void> {
  console.log(`[preview] Running: preview ${args}`);
  try {
    const { stdout, stderr } = await execaCommand(`preview ${args}`, {
      timeout: 600_000, // 10 minute timeout for builds
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
  slug: string
): Promise<void> {
  await runPreview(`create ${repo} ${branch} --slug ${slug}`);
}

export async function updatePreview(slug: string): Promise<void> {
  await runPreview(`update ${slug}`);
}

export async function destroyPreview(slug: string): Promise<void> {
  await runPreview(`destroy ${slug}`);
}

export async function postPRComment(
  repo: string,
  prNumber: number,
  body: string
): Promise<void> {
  console.log(`[preview] Posting comment on ${repo}#${prNumber}`);
  try {
    await execaCommand(
      `gh pr comment ${prNumber} --repo ${repo} --body "${body.replace(/"/g, '\\"')}"`,
      { timeout: 30_000 }
    );
  } catch (error) {
    console.error(`[preview] Failed to post PR comment:`, error);
  }
}
