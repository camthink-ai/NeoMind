/** devices api — split from api.ts 2026-09. */
import type { Device, DeviceType, AddDeviceRequest, DeviceCurrentStateResponse, BatchCurrentValuesResponse, DraftDevice, SuggestedDeviceType } from '@/types'
import { fetchAPI } from "./client"

export const devicesApi = {
  // Devices
  getDevices: () => fetchAPI<{ devices: Device[]; count: number }>('/devices'),
  getDevice: (id: string) => fetchAPI<Device>(`/devices/${id}`),
  getDeviceCurrent: (id: string) => fetchAPI<DeviceCurrentStateResponse>(`/devices/${id}/current`),
  getDevicesCurrentBatch: (deviceIds: string[], signal?: AbortSignal) =>
    fetchAPI<BatchCurrentValuesResponse>('/devices/current-batch', {
      method: 'POST',
      body: JSON.stringify({ device_ids: deviceIds }),
      skipErrorToast: true, // Skip error toast if endpoint not implemented
      signal,
    }),
  addDevice: (req: AddDeviceRequest) =>
    // updated_existing=true means an EXISTING device with this id was
    // replaced (the backend upserts) — callers can warn before silently
    // overwriting a device another client created.
    fetchAPI<{ device_id: string; added: boolean; updated_existing?: boolean }>('/devices', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  updateDevice: (id: string, req: Partial<AddDeviceRequest>) =>
    fetchAPI<{ device_id: string; updated: boolean }>(`/devices/${id}`, {
      method: 'PUT',
      body: JSON.stringify(req),
    }),
  deleteDevice: (id: string) =>
    fetchAPI<{ device_id: string; deleted: boolean }>(`/devices/${id}`, {
      method: 'DELETE',
    }),
  sendCommand: (deviceId: string, command: string, params: Record<string, unknown> = {}) =>
    fetchAPI<{
      device_id: string
      command: string
      sent: boolean
      /** CommandDelivery serde (snake_case); absent on older servers. */
      delivery?: import('@/lib/commandDeliveryLadder').CommandDeliveryJson | null
      delivery_note?: string | null
    }>(`/devices/${deviceId}/command/${command}`, {
      method: 'POST',
      body: JSON.stringify({ params }),
    }),

  // Device Types
  getDeviceTypes: () => fetchAPI<{ device_types: DeviceType[]; count: number }>('/device-types'),
  getDeviceType: (id: string) => fetchAPI<DeviceType>(`/device-types/${id}`),
  addDeviceType: (definition: DeviceType) =>
    fetchAPI<{ error?: string }>('/device-types', {
      method: 'POST',
      body: JSON.stringify(definition),
    }),
  deleteDeviceType: (id: string) =>
    fetchAPI<{ error?: string }>(`/device-types/${id}`, {
      method: 'DELETE',
    }),
  validateDeviceType: (definition: DeviceType) =>
    fetchAPI<{ valid: boolean; errors?: string[]; warnings?: string[]; message: string }>('/device-types', {
      method: 'PUT',
      body: JSON.stringify(definition),
    }),
  generateMDL: (req: { device_name: string; description?: string; uplink_example: string; downlink_example?: string }) =>
    fetchAPI<DeviceType>('/devices/generate-mdl', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  // ========== Draft Devices API (Auto-onboarding) ==========
  // List all draft devices discovered through auto-onboarding
  getDraftDevices: () =>
    fetchAPI<{ items: Array<{
      id: string
      device_id: string
      source: string
      original_topic?: string
      status: string
      sample_count: number
      max_samples: number
      generated_type?: {
        device_type: string
        name: string
        description: string
        category: string
        metrics: Array<{
          name: string
          path: string
          semantic_type: string
          display_name: string
          confidence: number
        }>
        confidence: number
        summary: {
          samples_analyzed: number
          fields_discovered: number
          metrics_generated: number
          inferred_category: string
          insights: string[]
          warnings: string[]
          recommendations: string[]
        }
      }
      discovered_at: number
      updated_at: number
      error_message?: string
      user_name?: string
      association_exception?: boolean
      association_exception_kind?: string
      blocked_device_ids?: string[]
      claimed_identity?: string
      sample_preview?: Record<string, unknown> | null
    }>; count: number }>('/devices/drafts'),

  // Get a specific draft device
  getDraftDevice: (deviceId: string) =>
    fetchAPI<{
      id: string
      device_id: string
      source: string
      status: string
      sample_count: number
      max_samples: number
      generated_type?: {
        device_type: string
        name: string
        description: string
        category: string
        metrics: Array<{
          name: string
          path: string
          semantic_type: string
          display_name: string
          confidence: number
        }>
        confidence: number
        summary: {
          samples_analyzed: number
          fields_discovered: number
          metrics_generated: number
          inferred_category: string
          insights: string[]
          warnings: string[]
          recommendations: string[]
        }
      }
      discovered_at: number
      updated_at: number
      error_message?: string
      user_name?: string
    }>(`/devices/drafts/${deviceId}`),

  // Approve a draft device - register it as a real device
  approveDraftDevice: (deviceId: string) =>
    fetchAPI<{
      original_device_id: string
      system_device_id: string
      device_type: string
      recommended_topic: string
      registered: boolean
      message: string
    }>(`/devices/drafts/${deviceId}/approve`, {
      method: 'POST',
    }),

  // Reject a draft device
  rejectDraftDevice: (deviceId: string, request: { reason: string }) =>
    fetchAPI<{ device_id: string; rejected: boolean }>(`/devices/drafts/${deviceId}/reject`, {
      method: 'POST',
      body: JSON.stringify(request),
    }),

  // Trigger manual analysis of a draft device
  triggerDraftAnalysis: (deviceId: string) =>
    fetchAPI<{ device_id: string; analysis_triggered: boolean }>(`/devices/drafts/${deviceId}/analyze`, {
      method: 'POST',
    }),

  // Update draft device (user edits)
  updateDraftDevice: (deviceId: string, request: { name?: string; description?: string }) =>
    fetchAPI<{ device_id: string; updated: boolean }>(`/devices/drafts/${deviceId}`, {
      method: 'PUT',
      body: JSON.stringify(request),
    }),

  // Enhance draft device with LLM (manual trigger for Chinese names, descriptions, units)
  enhanceDraftWithLLM: (deviceId: string) =>
    fetchAPI<DraftDevice>(`/devices/drafts/${deviceId}/enhance`, {
      method: 'POST',
    }),

  // Clean up old draft devices
  cleanupDraftDevices: () =>
    fetchAPI<{ cleaned: number; message: string }>('/devices/drafts/cleanup', {
      method: 'POST',
    }),

  // Get all registered type signatures (for type reuse)
  getTypeSignatures: () =>
    fetchAPI<{ signatures: Record<string, string>; count: string }>('/devices/drafts/type-signatures'),

  // Get suggested device types for a draft device
  suggestDeviceTypes: (deviceId: string) =>
    fetchAPI<{
      suggestions: SuggestedDeviceType[]
      exact_match: string | null
    }>(`/devices/drafts/${deviceId}/suggest-types`),

  // Approve draft device with optional existing type assignment or new type details
  approveDraftDeviceWithType: (
    deviceId: string,
    existingType?: string,
    newTypeInfo?: { device_type: string; name: string; description: string },
    deviceName?: string
  ) => {
    const body: Record<string, unknown> = {}
    if (existingType) {
      body.existing_type = existingType
    }
    if (newTypeInfo) {
      body.new_type = newTypeInfo
    }
    if (deviceName) {
      body.device_name = deviceName
    }
    return fetchAPI<{
      original_device_id: string
      system_device_id: string
      device_type: string
      recommended_topic: string
      registered: boolean
      message: string
    }>(`/devices/drafts/${deviceId}/approve`, {
      method: 'POST',
      body: JSON.stringify(body),
    })
  },

  // Confirm real identity for an MQTT association-exception draft
  confirmAssociationDraft: (
    draftId: string,
    request: {
      device_id: string
      identity_field: string
      identity_source?: string
      existing_type?: string
      device_name?: string
      overwrite_blocked?: boolean
    }
  ) =>
    fetchAPI<{
      original_device_id: string
      system_device_id: string
      device_type: string
      recommended_topic: string
      registered: boolean
      subscription_ready?: boolean
      online?: boolean
      message: string
    }>(`/devices/drafts/${draftId}/confirm-association`, {
      method: 'POST',
      body: JSON.stringify(request),
      // BindIncomplete is app-level 503 (config saved, sub not done) — not a
      // gateway blip. Do not auto-retry or toast success; dialog shows reason.
      skipRetry: true,
      skipErrorToast: true,
    }),

}
