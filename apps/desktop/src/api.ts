import { invoke } from "@tauri-apps/api/core";

export interface CommandFailure {
  code: string;
  message: string;
}

function commandFailure(error: unknown): CommandFailure {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<CommandFailure>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return { code: candidate.code, message: candidate.message };
    }
  }
  if (typeof error === "string" && error.length <= 500) {
    return { code: "command_failed", message: error };
  }
  return {
    code: "command_failed",
    message: "The local desktop command failed without exposing internal details.",
  };
}

export async function command<T>(name: string, input?: unknown): Promise<T> {
  try {
    return await invoke<T>(name, input === undefined ? undefined : { input });
  } catch (error) {
    throw commandFailure(error);
  }
}

export function failureMessage(error: unknown): string {
  return commandFailure(error).message;
}
