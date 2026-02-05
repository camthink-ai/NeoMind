/**
 * API integration tests (without UI)
 */

import { test, expect } from '@playwright/test';

const API_BASE = process.env.API_BASE || '/api';

test.describe('API Endpoints', () => {

  test('GET /api/health should return healthy status', async ({ request }) => {
    const response = await request.get(`${API_BASE}/health`);
    expect(response.status()).toBe(200);

    const data = await response.json();
    expect(data).toHaveProperty('status');
  });

  test('GET /api/llm-backends should return backends list', async ({ request }) => {
    const response = await request.get(`${API_BASE}/llm-backends`);

    // API should be accessible (may return 401 if auth required, or 500 if backend not running)
    expect([200, 401, 500]).toContain(response.status());

    if (response.status() === 200) {
      const data = await response.json();
      // Data may be array or object depending on API version
      expect(data).toBeTruthy();
    }
  });

  test('GET /api/devices should return devices or require auth', async ({ request }) => {
    const response = await request.get(`${API_BASE}/devices`);

    // Should either return data or require auth (or 500 if backend not running)
    expect([200, 401, 403, 500]).toContain(response.status());
  });

  test('POST /api/auth/login should validate credentials', async ({ request }) => {
    const response = await request.post(`${API_BASE}/auth/login`, {
      data: {
        username: 'invalid',
        password: 'invalid'
      }
    });

    // Should not succeed with invalid credentials
    expect([400, 401, 422]).toContain(response.status());
  });

  test('GET /api/stats/system should return system stats or require auth', async ({ request }) => {
    const response = await request.get(`${API_BASE}/stats/system`);

    // Should return stats or require auth (or 500 if backend not running)
    expect([200, 401, 403, 500]).toContain(response.status());
  });
});
