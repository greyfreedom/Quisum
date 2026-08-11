import { describe, expect, it } from "vitest";
import { failureGuidance } from "./api";

const validGuidance = {
  packVersion: "1.0.0",
  region: "US",
  sourceCheckedAt: "2026-08-12",
  sourceCheckDue: "2026-11-12",
  isCurrent: true,
  assistantResponse: "Contact emergency services and a trusted person.",
  sourceUrls: ["https://example.invalid/authority"],
  limitations: "Availability can change.",
};

describe("structured command failures", () => {
  it("preserves bounded HTTPS-only crisis guidance", () => {
    expect(failureGuidance({
      code: "crisis_route",
      message: validGuidance.assistantResponse,
      crisisGuidance: validGuidance,
    })).toEqual(validGuidance);
  });

  it("drops malformed crisis guidance while retaining ordinary error handling", () => {
    expect(failureGuidance({
      code: "crisis_route",
      message: "Stopped locally.",
      crisisGuidance: { ...validGuidance, sourceUrls: ["http://unsafe.invalid"] },
    })).toBeUndefined();
  });
});
