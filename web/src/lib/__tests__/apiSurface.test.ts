/// Runtime smoke for the api.ts split (2026-09): the barrel composes 17
/// domain modules; static checks (tsc + member-conservation script) cover
/// types, this guards the RUNTIME composition — every expected member is a
/// function on the composed object, and the legacy helper exports still
/// resolve from "@/lib/api".
import { describe, it, expect } from 'vitest'
import {
  api,
  fetchAPI,
  getApiBase,
  isTauriEnv,
  setApiBase,
  getApiKey,
  setApiKey,
  clearApiKey,
  tokenManager,
} from '@/lib/api'

describe('api surface (post-split composition)', () => {
  it('exposes the full member set — spot-check across every domain module', () => {
    // One representative member per split module (17; system has two blocks)
    const expected = [
      'get', 'post', // http
      'login', // auth
      'getDevices', // devices
      'getDraftDevices', // onboarding
      'listMessageChannels', // channels
      'listLlmBackends', // llm
      'listImBridges', // imBridges
      'getMqttStatus', // brokers
      'listSessions', // sessions
      'getDeviceTelemetrySummary', // telemetry
      'checkServerUpgrade', 'bulkAcknowledgeMessages', // system (two blocks)
      'getRule', // automation
      'getMidTermMemory', // memory
      'listExtensionTypes', // extensions
      'listAgents', // agents
      'createPushTarget', // dataPush
    ]
    for (const key of expected) {
      expect(typeof (api as Record<string, unknown>)[key], `api.${key}`).toBe('function')
    }
  })

  it('keeps the legacy helper exports importable from "@/lib/api"', () => {
    expect(typeof fetchAPI).toBe('function')
    expect(typeof getApiBase).toBe('function')
    expect(typeof isTauriEnv).toBe('function')
    expect(typeof setApiBase).toBe('function')
    expect(typeof getApiKey).toBe('function')
    expect(typeof setApiKey).toBe('function')
    expect(typeof clearApiKey).toBe('function')
    expect(tokenManager).toBeTruthy()
  })

  // A ratchet, not a snapshot. The split had to conserve the pre-split member
  // set exactly, and every later addition to the surface has to be
  // acknowledged here. 266 -> 267 when the agent editor's dry-run added
  // `testAgentPreview` (bcd026fe) — a real, called member, not surface bloat.
  it('member count grows only by deliberate additions (267)', () => {
    const keys = Object.keys(api)
    expect(keys.length).toBe(267)
  })
})
