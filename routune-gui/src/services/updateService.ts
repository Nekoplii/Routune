import { invoke } from "@tauri-apps/api/core";

export interface ReleaseInfo {
  version: string;
  tagName: string;
  name: string;
  body: string;
  publishedAt: string;
  htmlUrl: string;
}

/**
 * Compare two semver strings (e.g. "0.2.3", "v0.2.4", "0.0.0-dev").
 * Returns:
 *   > 0 if v1 > v2
 *   < 0 if v1 < v2
 *   0 if v1 === v2
 */
export function compareSemver(v1: string, v2: string): number {
  const clean1 = v1.trim().replace(/^v/, "");
  const clean2 = v2.trim().replace(/^v/, "");

  // If one is dev and other is release, release is considered newer
  const isDev1 = clean1.includes("dev") || clean1 === "0.0.0";
  const isDev2 = clean2.includes("dev") || clean2 === "0.0.0";
  if (isDev1 && !isDev2) return -1;
  if (!isDev1 && isDev2) return 1;

  const parts1 = clean1.split("-")[0].split(".").map((n) => parseInt(n, 10) || 0);
  const parts2 = clean2.split("-")[0].split(".").map((n) => parseInt(n, 10) || 0);

  for (let i = 0; i < Math.max(parts1.length, parts2.length); i++) {
    const num1 = parts1[i] ?? 0;
    const num2 = parts2[i] ?? 0;
    if (num1 > num2) return 1;
    if (num1 < num2) return -1;
  }

  return 0;
}

/**
 * Query GitHub Releases API for the latest published Routune version.
 */
export async function checkForUpdates(currentVersion: string): Promise<ReleaseInfo | null> {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 6000);

  try {
    const res = await fetch("https://api.github.com/repos/Nekoplii/Routune/releases/latest", {
      headers: {
        Accept: "application/vnd.github.v3+json",
      },
      signal: controller.signal,
    });

    if (!res.ok) {
      return null;
    }

    const data = await res.json();
    const latestTag = data.tag_name || "";
    const latestVersion = latestTag.replace(/^v/, "");

    if (compareSemver(latestVersion, currentVersion) > 0) {
      return {
        version: latestVersion,
        tagName: latestTag,
        name: data.name || latestTag,
        body: data.body || "",
        publishedAt: data.published_at || "",
        htmlUrl: data.html_url || `https://github.com/Nekoplii/Routune/releases/tag/${latestTag}`,
      };
    }

    return null;
  } catch {
    return null;
  } finally {
    clearTimeout(timeoutId);
  }
}

/**
 * Execute the automated one-line installer in background to update the local installation.
 */
export async function runInAppUpdate(): Promise<string> {
  return await invoke<string>("run_in_app_update");
}

/**
 * Restart the Routune desktop application.
 */
export async function restartApp(): Promise<void> {
  try {
    await invoke("restart_app");
  } catch (err) {
    console.error("restart_app error:", err);
  }
}

/**
 * Open external link using the system's default browser (xdg-open on Linux).
 */
export async function openExternalUrl(url: string): Promise<void> {
  try {
    await invoke("open_external_url", { url });
  } catch {
    window.open(url, "_blank");
  }
}
