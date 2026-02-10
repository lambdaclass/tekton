import type { StreamEvent } from "./types.js";

/**
 * Parse Claude's stream-json output format.
 * Each line is a JSON object with different event types.
 *
 * Key event types from Claude's stream-json:
 * - {"type":"assistant","subtype":"text","text":"..."}
 * - {"type":"assistant","subtype":"tool_use","tool":"Write","input":{...}}
 * - {"type":"result","subtype":"success","result":"..."}
 * - {"type":"result","subtype":"error","error":"..."}
 */
export function parseStreamLine(line: string): StreamEvent | null {
  const trimmed = line.trim();
  if (!trimmed) return null;

  let data: Record<string, unknown>;
  try {
    data = JSON.parse(trimmed);
  } catch {
    return null;
  }

  const type = data.type as string;
  const subtype = data.subtype as string | undefined;

  if (type === "assistant") {
    if (subtype === "text") {
      return {
        type: "text",
        content: (data.text as string) ?? "",
      };
    }
    if (subtype === "tool_use") {
      const tool = data.tool as string;
      const input = data.input as Record<string, unknown> | undefined;
      const filePath = input?.file_path as string | undefined;
      return {
        type: "tool_use",
        content: `Using tool: ${tool}${filePath ? ` on ${filePath}` : ""}`,
        tool,
        filePath,
      };
    }
  }

  if (type === "tool_result") {
    return {
      type: "tool_result",
      content: (data.output as string) ?? "",
    };
  }

  if (type === "result") {
    if (subtype === "success") {
      return {
        type: "completion",
        content: (data.result as string) ?? "Task completed",
      };
    }
    if (subtype === "error") {
      return {
        type: "error",
        content: (data.error as string) ?? "Unknown error",
      };
    }
  }

  return null;
}

/**
 * Process a chunk of streaming data, handling line buffering.
 * Returns parsed events and any incomplete trailing data.
 */
export function processStreamChunk(
  buffer: string,
  chunk: string,
  onEvent: (event: StreamEvent) => void,
): string {
  buffer += chunk;
  const lines = buffer.split("\n");

  // Last element may be incomplete — keep it in buffer
  const remaining = lines.pop() ?? "";

  for (const line of lines) {
    const event = parseStreamLine(line);
    if (event) {
      onEvent(event);
    }
  }

  return remaining;
}

/**
 * Summarize a tool_use event for Slack display.
 */
export function summarizeToolUse(event: StreamEvent): string {
  if (event.type !== "tool_use") return event.content;

  switch (event.tool) {
    case "Write":
      return `Writing ${event.filePath ?? "file"}`;
    case "Edit":
      return `Editing ${event.filePath ?? "file"}`;
    case "Read":
      return `Reading ${event.filePath ?? "file"}`;
    case "Bash":
      return "Running command";
    case "Glob":
      return "Searching files";
    case "Grep":
      return "Searching code";
    default:
      return event.content;
  }
}
