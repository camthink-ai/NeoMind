/**
 * Device-binding health for dashboard widgets.
 *
 * Resolves a widget's data source(s) to their bound devices and reports:
 *  - dangling: the bound device no longer exists in the registry (deleted
 *    while the dashboard kept the binding) — the widget can never receive
 *    data again and the UI should say so instead of a generic "No Data"
 *  - stale: the device exists but is not `online` per the 4-state connection
 *    model (offline / connectedIdle) — the widget keeps showing the last
 *    reported value, which may be arbitrarily old
 *
 * info/command bindings are excluded: they read the device record itself and
 * stay accurate regardless of telemetry freshness.
 */

import { useMemo } from 'react'
import { useStore } from '@/store'
import type { Device } from '@/types'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { isDataSourceList, resolveDataSource } from '@/types/dashboard'
import { getDeviceState } from '@/lib/utils/deviceStatus'

function toList(ds: DataSourceOrList | undefined): DataSource[] {
  if (!ds) return []
  return isDataSourceList(ds) ? ds : [ds]
}

/**
 * A bound device whose data is not fresh — the widget keeps showing its last
 * reported value. `state` distinguishes WHY it is stale: `offline` (transport
 * gone, value may be arbitrarily old — the 4-state model colors it warning)
 * vs `connectedIdle` (transport alive, awaiting data — a calm state the badge
 * must not paint with the same urgency).
 */
export interface StaleDeviceRef {
  id: string
  /** Device display name at classification time, for tooltip rendering. */
  name: string
  state: 'offline' | 'connectedIdle'
  /** Epoch ms of the last report; null when never/unknown. */
  lastSeen: number | null
}

export interface DeviceBindingStatus {
  /** Widget has at least one device-sourced latest/timeseries binding. */
  hasDeviceBinding: boolean
  /** Every device binding points at a device that no longer exists in the registry. */
  allDangling: boolean
  /** IDs of bound devices that no longer exist in the registry. */
  danglingDeviceIds: string[]
  /** Flat id list of every stale device (offline + connectedIdle), in binding order. */
  staleDeviceIds: string[]
  /** Stale bindings carrying state + last report time, for differentiated rendering. */
  staleDevices: StaleDeviceRef[]
}

const NO_BINDING: DeviceBindingStatus = {
  hasDeviceBinding: false,
  allDangling: false,
  danglingDeviceIds: [],
  staleDeviceIds: [],
  staleDevices: [],
}

/**
 * Pure classification of a widget's device bindings against the device
 * registry. Exported for direct unit testing; useDeviceBindingStatus wraps
 * it with the store subscription.
 */
export function classifyDeviceBindings(
  sources: DataSource[],
  devices: Device[] | null | undefined,
): DeviceBindingStatus {
  const deviceIds = new Set<string>()
  for (const raw of sources) {
    const ds = resolveDataSource(raw)
    if (ds.mode === 'info' || ds.mode === 'command') continue
    if (ds.source === 'device' && typeof ds.id === 'string' && ds.id) {
      deviceIds.add(ds.id)
    }
  }
  if (deviceIds.size === 0) return NO_BINDING

  const deviceList = devices ?? []
  // An empty registry may mean we are still bootstrapping — never declare
  // bindings dangling until at least one device is known.
  const canResolve = deviceList.length > 0
  const deviceById = new Map(deviceList.map(d => [d.id, d]))

  const danglingDeviceIds: string[] = []
  const staleDevices: StaleDeviceRef[] = []
  for (const id of deviceIds) {
    const device = deviceById.get(id)
    if (!device) {
      if (canResolve) danglingDeviceIds.push(id)
      continue
    }
    // connectedIdle means transport is alive but data is not fresh — the
    // displayed value is still the last one, hence stale. `disconnected`
    // (never reported) is excluded: those cards show no value at all, so
    // a stale badge would only be noise.
    const state = getDeviceState(device).state
    if (state === 'offline' || state === 'connectedIdle') {
      const lastSeenEpoch = Date.parse(device.last_seen ?? '')
      staleDevices.push({
        id,
        name: device.name || id,
        state,
        lastSeen: Number.isFinite(lastSeenEpoch) && lastSeenEpoch > 0 ? lastSeenEpoch : null,
      })
    }
  }

  return {
    hasDeviceBinding: true,
    allDangling: danglingDeviceIds.length > 0 && danglingDeviceIds.length === deviceIds.size,
    danglingDeviceIds,
    staleDeviceIds: staleDevices.map((d) => d.id),
    staleDevices,
  }
}

export function useDeviceBindingStatus(dataSource: DataSourceOrList | undefined): DeviceBindingStatus {
  const devices = useStore((s) => s.devices)

  return useMemo(
    () => classifyDeviceBindings(toList(dataSource), devices),
    [dataSource, devices],
  )
}
