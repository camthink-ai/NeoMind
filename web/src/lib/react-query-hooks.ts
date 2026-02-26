/**
 * React Query Hooks - Centralized Data Fetching
 * 
 * This module provides React Query hooks for common data fetching patterns.
 * Benefits:
 * - Automatic caching and deduplication
 * - Background refetching
 * - Retry logic
 * - Loading/error states
 * 
 * Usage:
 * ```tsx
 * import { useDevices } from '@/lib/react-query-hooks'
 * 
 * function DevicesPage() {
 *   const { data: devices, isLoading, error } = useDevices()
 *   // ... render
 * }
 * ```
 */

import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { fetchAPI } from './api'
import type { Device, DeviceType, AiAgent, Extension, Rule, Automation } from '@/types'

// ============================================================================
// Query Keys - Centralized for consistency
// ============================================================================

export const queryKeys = {
  devices: {
    all: ['devices'] as const,
    lists: () => [...queryKeys.devices.all, 'list'] as const,
    list: (filters?: Record<string, unknown>) => [...queryKeys.devices.lists(), filters] as const,
    details: () => [...queryKeys.devices.all, 'detail'] as const,
    detail: (id: string) => [...queryKeys.devices.details(), id] as const,
    types: () => [...queryKeys.devices.all, 'types'] as const,
  },
  agents: {
    all: ['agents'] as const,
    lists: () => [...queryKeys.agents.all, 'list'] as const,
    list: (filters?: Record<string, unknown>) => [...queryKeys.agents.lists(), filters] as const,
    details: () => [...queryKeys.agents.all, 'detail'] as const,
    detail: (id: string) => [...queryKeys.agents.details(), id] as const,
  },
  extensions: {
    all: ['extensions'] as const,
    lists: () => [...queryKeys.extensions.all, 'list'] as const,
    details: () => [...queryKeys.extensions.all, 'detail'] as const,
    detail: (id: string) => [...queryKeys.extensions.details(), id] as const,
  },
  rules: {
    all: ['rules'] as const,
    lists: () => [...queryKeys.rules.all, 'list'] as const,
  },
  automations: {
    all: ['automations'] as const,
    lists: () => [...queryKeys.automations.all, 'list'] as const,
  },
}

// ============================================================================
// Device Hooks
// ============================================================================

/**
 * Fetch all devices with automatic caching.
 * Data is cached for 5 minutes (staleTime) and 10 minutes (gcTime).
 */
export function useDevices(filters?: Record<string, unknown>) {
  return useQuery({
    queryKey: queryKeys.devices.list(filters),
    queryFn: () => fetchAPI<Device[]>(`/api/devices`),
  })
}

/**
 * Fetch a single device by ID.
 */
export function useDevice(id: string) {
  return useQuery({
    queryKey: queryKeys.devices.detail(id),
    queryFn: () => fetchAPI<Device>(`/api/devices/${id}`),
    enabled: !!id, // Only fetch if ID is provided
  })
}

/**
 * Fetch all device types.
 */
export function useDeviceTypes() {
  return useQuery({
    queryKey: queryKeys.devices.types(),
    queryFn: () => fetchAPI<DeviceType[]>(`/api/device-types`),
  })
}

// ============================================================================
// Agent Hooks
// ============================================================================

/**
 * Fetch all AI agents.
 */
export function useAgents(filters?: Record<string, unknown>) {
  return useQuery({
    queryKey: queryKeys.agents.list(filters),
    queryFn: () => fetchAPI<AiAgent[]>(`/api/agents`),
  })
}

/**
 * Fetch a single agent by ID.
 */
export function useAgent(id: string) {
  return useQuery({
    queryKey: queryKeys.agents.detail(id),
    queryFn: () => fetchAPI<AiAgent>(`/api/agents/${id}`),
    enabled: !!id,
  })
}

// ============================================================================
// Extension Hooks
// ============================================================================

/**
 * Fetch all extensions.
 */
export function useExtensions() {
  return useQuery({
    queryKey: queryKeys.extensions.lists(),
    queryFn: () => fetchAPI<Extension[]>(`/api/extensions`),
  })
}

/**
 * Fetch a single extension by ID.
 */
export function useExtension(id: string) {
  return useQuery({
    queryKey: queryKeys.extensions.detail(id),
    queryFn: () => fetchAPI<Extension>(`/api/extensions/${id}`),
    enabled: !!id,
  })
}

// ============================================================================
// Rules & Automations Hooks
// ============================================================================

/**
 * Fetch all rules.
 */
export function useRules() {
  return useQuery({
    queryKey: queryKeys.rules.lists(),
    queryFn: () => fetchAPI<Rule[]>(`/api/rules`),
  })
}

/**
 * Fetch all automations.
 */
export function useAutomations() {
  return useQuery({
    queryKey: queryKeys.automations.lists(),
    queryFn: () => fetchAPI<Automation[]>(`/api/automations`),
  })
}

// ============================================================================
// Mutation Hooks (Write Operations)
// ============================================================================

/**
 * Example mutation hook for updating a device.
 * Automatically invalidates device list cache on success.
 */
export function useUpdateDevice() {
  const queryClient = useQueryClient()
  
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: Partial<Device> }) =>
      fetchAPI<Device>(`/api/devices/${id}`, {
        method: 'PUT',
        body: JSON.stringify(data),
      }),
    onSuccess: () => {
      // Invalidate device list to trigger refetch
      queryClient.invalidateQueries({ queryKey: queryKeys.devices.lists() })
    },
  })
}

/**
 * Example mutation hook for deleting a device.
 */
export function useDeleteDevice() {
  const queryClient = useQueryClient()
  
  return useMutation({
    mutationFn: (id: string) =>
      fetchAPI<void>(`/api/devices/${id}`, {
        method: 'DELETE',
      }),
    onSuccess: () => {
      // Invalidate device list to trigger refetch
      queryClient.invalidateQueries({ queryKey: queryKeys.devices.lists() })
      // Also invalidate all device details
      queryClient.invalidateQueries({ queryKey: queryKeys.devices.details() })
    },
  })
}
