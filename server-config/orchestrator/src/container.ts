import { execa } from "execa";
import type { ContainerInfo } from "./types.js";

const SSH_OPTS = [
  "-o", "StrictHostKeyChecking=no",
  "-o", "UserKnownHostsFile=/dev/null",
  "-o", "LogLevel=ERROR",
];

export async function createContainer(name: string): Promise<ContainerInfo> {
  const result = await execa("agent", ["create", name]);
  // Parse container IP from agent output: "  Container IP:  10.100.0.3"
  const ipMatch = result.stdout.match(/Container IP:\s+([\d.]+)/);
  if (!ipMatch) {
    throw new Error(`Failed to parse container IP from output: ${result.stdout}`);
  }
  return { name, ip: ipMatch[1] };
}

export async function destroyContainer(name: string): Promise<void> {
  await execa("agent", ["destroy", name]);
}

export async function listContainers(): Promise<string> {
  const result = await execa("agent", ["list"]);
  return result.stdout;
}

export async function sshExec(
  containerIp: string,
  command: string,
  options?: { user?: string; timeout?: number },
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const user = options?.user ?? "agent";
  const timeout = options?.timeout ?? 30_000;

  const result = await execa(
    "ssh",
    [...SSH_OPTS, `${user}@${containerIp}`, command],
    { timeout, reject: false },
  );

  return {
    stdout: result.stdout,
    stderr: result.stderr,
    exitCode: result.exitCode ?? 1,
  };
}

export async function sshExecStreaming(
  containerIp: string,
  command: string,
  onData: (data: string) => void,
  options?: { user?: string; timeout?: number },
): Promise<number> {
  const user = options?.user ?? "agent";
  const timeout = options?.timeout ?? 600_000; // 10 min default for Claude

  const proc = execa(
    "ssh",
    [...SSH_OPTS, `${user}@${containerIp}`, command],
    { timeout, reject: false, buffer: false },
  );

  if (proc.stdout) {
    proc.stdout.on("data", (chunk: Buffer) => {
      onData(chunk.toString());
    });
  }

  if (proc.stderr) {
    proc.stderr.on("data", (chunk: Buffer) => {
      onData(chunk.toString());
    });
  }

  const result = await proc;
  return result.exitCode ?? 1;
}

export async function waitForSsh(containerIp: string, maxWaitMs = 30_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < maxWaitMs) {
    try {
      const result = await sshExec(containerIp, "echo ready", { timeout: 5_000 });
      if (result.exitCode === 0) return;
    } catch {
      // SSH not ready yet
    }
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error(`SSH to ${containerIp} not ready after ${maxWaitMs}ms`);
}
