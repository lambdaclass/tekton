import type { Flags } from "lighthouse";
import lighthouse from "lighthouse";
import * as chromeLauncher from "chrome-launcher";

/** Score thresholds (0-100). Tests fail when a score drops below these. */
export const THRESHOLDS = {
  performance: 70,
  accessibility: 85,
  "best-practices": 85,
  seo: 70,
} as const;

export type CategoryId = keyof typeof THRESHOLDS;

/** Lighthouse configuration targeting a desktop viewport with simulated throttling. */
export const desktopConfig = {
  extends: "lighthouse:default",
  settings: {
    formFactor: "desktop" as const,
    screenEmulation: {
      mobile: false,
      width: 1350,
      height: 940,
      deviceScaleFactor: 1,
      disabled: false,
    },
    throttling: {
      rttMs: 40,
      throughputKbps: 10240,
      cpuSlowdownMultiplier: 1,
      requestLatencyMs: 0,
      downloadThroughputKbps: 0,
      uploadThroughputKbps: 0,
    },
    onlyCategories: [
      "performance",
      "accessibility",
      "best-practices",
      "seo",
    ],
  },
};

export interface AuditScores {
  performance: number;
  accessibility: number;
  "best-practices": number;
  seo: number;
}

/**
 * Launch a headless Chrome instance suitable for Lighthouse.
 * Returns the chrome instance (for cleanup) and the debugging port.
 */
export async function launchChrome(): Promise<chromeLauncher.LaunchedChrome> {
  return chromeLauncher.launch({
    chromeFlags: ["--headless", "--no-sandbox", "--disable-gpu"],
  });
}

/**
 * Run a Lighthouse audit against `url` using the Chrome instance on `port`.
 * Optionally pass extra HTTP headers (e.g. an auth cookie).
 */
export async function runLighthouseAudit(
  url: string,
  port: number,
  extraHeaders?: Record<string, string>,
): Promise<AuditScores> {
  const flags: Flags = {
    port,
    output: "html",
    logLevel: "error",
  };

  if (extraHeaders) {
    flags.extraHeaders = extraHeaders;
  }

  const result = await lighthouse(url, flags, desktopConfig);

  if (!result || !result.lhr) {
    throw new Error(`Lighthouse returned no results for ${url}`);
  }

  const { categories } = result.lhr;

  const scores: AuditScores = {
    performance: Math.round((categories.performance?.score ?? 0) * 100),
    accessibility: Math.round((categories.accessibility?.score ?? 0) * 100),
    "best-practices": Math.round(
      (categories["best-practices"]?.score ?? 0) * 100,
    ),
    seo: Math.round((categories.seo?.score ?? 0) * 100),
  };

  return scores;
}
