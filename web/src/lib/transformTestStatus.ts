/**
 * Derive an honest Transform test verdict for the UI (1.3 iter D).
 *
 * Backend may return HTTP 200 with failures spelled out in `warnings`, and
 * dry-run extension blocks in `unverified`. The UI must never paint those as
 * plain green success.
 */

export type TransformTestVerdict =
  | "failed"
  | "no_output"
  | "partial_success"
  | "unverified"
  | "verified_success"

export interface TransformTestStatusInput {
  /** Metrics produced by the preview. */
  count: number
  warnings?: string[] | null
  unverified?: string[] | null
  /**
   * From `POST /transforms/test-code` only. When false, the run is a hard
   * failure regardless of other fields. Omitted for `/transforms/:id/test`
   * (that path always answers 200 and uses warnings).
   */
  codeSuccess?: boolean
}

export interface TransformTestStatus {
  verdict: TransformTestVerdict
  /** True only for a fully verified success — green badge allowed. */
  allowGreenSuccess: boolean
  warningCount: number
  unverifiedCount: number
  metricCount: number
}

function looksLikeHardFailure(warnings: string[]): boolean {
  return warnings.some((w) => {
    const s = w.toLowerCase()
    return (
      s.includes("failed") ||
      s.includes("failure") ||
      s.includes("error") ||
      s.includes("throw")
    )
  })
}

export function deriveTransformTestStatus(
  input: TransformTestStatusInput,
): TransformTestStatus {
  const warnings = (input.warnings ?? []).filter(Boolean)
  const unverified = (input.unverified ?? []).filter(Boolean)
  const metricCount = Math.max(0, input.count | 0)

  if (input.codeSuccess === false) {
    return {
      verdict: "failed",
      allowGreenSuccess: false,
      warningCount: warnings.length,
      unverifiedCount: unverified.length,
      metricCount,
    }
  }

  // Plan: unverified non-empty must NOT show ordinary green success —
  // even when pure JS metrics also came back.
  if (unverified.length > 0) {
    return {
      verdict: "unverified",
      allowGreenSuccess: false,
      warningCount: warnings.length,
      unverifiedCount: unverified.length,
      metricCount,
    }
  }

  if (metricCount === 0) {
    if (looksLikeHardFailure(warnings)) {
      return {
        verdict: "failed",
        allowGreenSuccess: false,
        warningCount: warnings.length,
        unverifiedCount: 0,
        metricCount: 0,
      }
    }
    return {
      verdict: "no_output",
      allowGreenSuccess: false,
      warningCount: warnings.length,
      unverifiedCount: 0,
      metricCount: 0,
    }
  }

  if (warnings.length > 0) {
    return {
      verdict: "partial_success",
      allowGreenSuccess: false,
      warningCount: warnings.length,
      unverifiedCount: 0,
      metricCount,
    }
  }

  return {
    verdict: "verified_success",
    allowGreenSuccess: true,
    warningCount: 0,
    unverifiedCount: 0,
    metricCount,
  }
}
