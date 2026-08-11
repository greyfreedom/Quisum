import { invoke } from "@tauri-apps/api/core";

export interface CommandFailure {
  code: string;
  message: string;
  crisisGuidance?: CrisisGuidance;
}

export interface CrisisGuidance {
  packVersion: string;
  region: string;
  sourceCheckedAt: string;
  sourceCheckDue: string;
  isCurrent: boolean;
  assistantResponse: string;
  sourceUrls: string[];
  limitations: string;
}

function commandFailure(error: unknown): CommandFailure {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<CommandFailure>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      const guidance = validCrisisGuidance(candidate.crisisGuidance)
        ? candidate.crisisGuidance
        : undefined;
      return { code: candidate.code, message: candidate.message, crisisGuidance: guidance };
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

export function failureGuidance(error: unknown): CrisisGuidance | undefined {
  return commandFailure(error).crisisGuidance;
}

function validCrisisGuidance(value: unknown): value is CrisisGuidance {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<CrisisGuidance>;
  return typeof candidate.packVersion === "string" && candidate.packVersion.length <= 32
    && typeof candidate.region === "string"
    && typeof candidate.sourceCheckedAt === "string"
    && typeof candidate.sourceCheckDue === "string"
    && typeof candidate.isCurrent === "boolean"
    && typeof candidate.assistantResponse === "string" && candidate.assistantResponse.length <= 8000
    && Array.isArray(candidate.sourceUrls) && candidate.sourceUrls.length <= 8
    && candidate.sourceUrls.every((source) => typeof source === "string" && source.length <= 2048 && source.startsWith("https://"))
    && typeof candidate.limitations === "string" && candidate.limitations.length <= 4000;
}
