/**
 * API surface — composed from the domain modules under lib/api/.
 * Historically a single 2700-line file; split 2026-09 for maintainability.
 * The "@/lib/api" import path is unchanged for all consumers.
 */
export * from "./api/client";
export * from "./api/types";

import { httpApi } from "./api/http";
import { authApi } from "./api/auth";
import { devicesApi } from "./api/devices";
import { onboardingApi } from "./api/onboarding";
import { channelsApi } from "./api/channels";
import { llmApi } from "./api/llm";
import { imBridgesApi } from "./api/imBridges";
import { brokersApi } from "./api/brokers";
import { sessionsApi } from "./api/sessions";
import { telemetryApi } from "./api/telemetry";
import { systemApi } from "./api/system";
import { automationApi } from "./api/automation";
import { memoryApi } from "./api/memory";
import { extensionsApi } from "./api/extensions";
import { agentsApi } from "./api/agents";
import { dataPushApi } from "./api/dataPush";

export const api = {
  ...httpApi,
  ...authApi,
  ...devicesApi,
  ...onboardingApi,
  ...channelsApi,
  ...llmApi,
  ...imBridgesApi,
  ...brokersApi,
  ...sessionsApi,
  ...telemetryApi,
  ...systemApi,
  ...automationApi,
  ...memoryApi,
  ...extensionsApi,
  ...agentsApi,
  ...dataPushApi,
};
