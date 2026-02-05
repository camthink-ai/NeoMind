/**
 * API fixtures for backend interaction
 */

import { APIRequestContext, APIResponse } from '@playwright/test';

const API_BASE = process.env.API_BASE || '/api';

export class ApiClient {
  constructor(private request: APIRequestContext) {}

  async get(endpoint: string, token?: string): Promise<APIResponse> {
    const headers = token ? { Authorization: `Bearer ${token}` } : undefined;
    return this.request.get(`${API_BASE}${endpoint}`, { headers });
  }

  async post(endpoint: string, data: any, token?: string): Promise<APIResponse> {
    const headers = token ? { Authorization: `Bearer ${token}` } : undefined;
    return this.request.post(`${API_BASE}${endpoint}`, {
      headers,
      data: JSON.stringify(data),
    });
  }

  async put(endpoint: string, data: any, token?: string): Promise<APIResponse> {
    const headers = token ? { Authorization: `Bearer ${token}` } : undefined;
    return this.request.put(`${API_BASE}${endpoint}`, {
      headers,
      data: JSON.stringify(data),
    });
  }

  async delete(endpoint: string, token?: string): Promise<APIResponse> {
    const headers = token ? { Authorization: `Bearer ${token}` } : undefined;
    return this.request.delete(`${API_BASE}${endpoint}`, { headers });
  }

  async login(username: string, password: string): Promise<{ token: string }> {
    const response = await this.post('/auth/login', { username, password });
    if (response.status() !== 200) {
      throw new Error(`Login failed: ${response.status()}`);
    }
    return await response.json();
  }

  async getHealth(): Promise<{ status: string }> {
    const response = await this.get('/health');
    return await response.json();
  }

  async getDevices(token?: string): Promise<any[]> {
    const response = await this.get('/devices', token);
    if (response.status() === 401) {
      return [];
    }
    return await response.json();
  }

  async getLlmBackends(): Promise<any[]> {
    const response = await this.get('/llm-backends');
    return await response.json();
  }
}
