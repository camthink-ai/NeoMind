/** channels api — split from api.ts 2026-09. */
import type { AlertChannel, ChannelListResponse, ChannelStats, ChannelTypeInfo, ChannelTestResult, ChannelSchemaResponse, CreateChannelRequest, ChannelFilter } from '@/types'
import { fetchAPI } from "./client"

export const channelsApi = {
  // ========== Message Channels API (replaces Alert Channels) ==========
  listMessageChannels: () => fetchAPI<ChannelListResponse>('/messages/channels'),
  getMessageChannel: (name: string) => fetchAPI<AlertChannel>(`/messages/channels/${encodeURIComponent(name)}`),
  listChannelTypes: () => fetchAPI<{ types: ChannelTypeInfo[]; count: number }>('/messages/channels/types'),
  getChannelSchema: (type: string) =>
    fetchAPI<ChannelSchemaResponse>(`/messages/channels/types/${encodeURIComponent(type)}/schema`),
  createMessageChannel: (req: CreateChannelRequest) =>
    fetchAPI<{ message: string; message_zh: string; channel: AlertChannel }>('/messages/channels', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  deleteMessageChannel: (name: string) =>
    fetchAPI<{ message: string; message_zh: string; name: string }>(
      `/messages/channels/${encodeURIComponent(name)}`,
      { method: 'DELETE' }
    ),
  updateMessageChannel: (name: string, config: Record<string, unknown>) =>
    fetchAPI<{ message: string; message_zh: string; channel: AlertChannel }>(
      `/messages/channels/${encodeURIComponent(name)}`,
      { method: 'PUT', body: JSON.stringify({ config }) }
    ),
  testMessageChannel: (name: string) =>
    fetchAPI<ChannelTestResult>(`/messages/channels/${encodeURIComponent(name)}/test`, {
      method: 'POST',
    }),
  updateChannelEnabled: (name: string, enabled: boolean) =>
    fetchAPI<{ message: string }>(`/messages/channels/${encodeURIComponent(name)}/enabled`, {
      method: 'PUT',
      body: JSON.stringify({ enabled }),
    }),
  getChannelStats: () => fetchAPI<ChannelStats>('/messages/channels/stats'),
  // Channel Filter API
  getChannelFilter: (name: string) =>
    fetchAPI<ChannelFilter>(`/messages/channels/${encodeURIComponent(name)}/filter`),
  updateChannelFilter: (name: string, filter: ChannelFilter) =>
    fetchAPI<{ message: string; message_zh: string; channel: string; filter: ChannelFilter }>(
      `/messages/channels/${encodeURIComponent(name)}/filter`,
      {
        method: 'PUT',
        body: JSON.stringify(filter),
      }
    ),
  // Recipient management for email channels
  listChannelRecipients: (name: string) =>
    fetchAPI<{ channel: string; recipients: string[]; count: number }>(
      `/messages/channels/${encodeURIComponent(name)}/recipients`
    ),
  addChannelRecipient: (name: string, email: string) =>
    fetchAPI<{ message: string; message_zh: string; channel: string; recipients: string[] }>(
      `/messages/channels/${encodeURIComponent(name)}/recipients`,
      { method: 'POST', body: JSON.stringify({ email }) }
    ),
  removeChannelRecipient: (name: string, email: string) =>
    fetchAPI<{ message: string; message_zh: string; channel: string; recipients: string[] }>(
      `/messages/channels/${encodeURIComponent(name)}/recipients/${encodeURIComponent(email)}`,
      { method: 'DELETE' }
    ),
  cleanupMessages: (req: { older_than_days: number }) =>
    fetchAPI<{ cleaned: number; message: string; message_zh: string }>('/messages/cleanup', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

}
