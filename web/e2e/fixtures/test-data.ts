/**
 * Test data generation utilities
 */

export const generateDeviceId = (): string => {
  return `test-device-${Date.now()}-${Math.random().toString(36).substring(7)}`;
};

export const generateSessionId = (): string => {
  return `test-session-${Date.now()}`;
};

export const generateRuleName = (): string => {
  return `Test Rule ${Date.now()}`;
};

/**
 * Test device data
 */
export const testDevices = {
  temperatureSensor: {
    id: () => generateDeviceId(),
    name: 'Test Temperature Sensor',
    type: 'temperature_sensor',
    capabilities: {
      metrics: ['temperature', 'humidity'],
      commands: ['calibrate', 'reset'],
    },
  },
  smartSwitch: {
    id: () => generateDeviceId(),
    name: 'Test Smart Switch',
    type: 'smart_switch',
    capabilities: {
      metrics: ['power', 'energy'],
      commands: ['turn_on', 'turn_off', 'toggle'],
    },
  },
};

/**
 * Test rule data
 */
export const testRules = {
  simpleThreshold: {
    name: () => generateRuleName(),
    description: 'Temperature exceeds threshold',
    conditions: [
      {
        deviceId: () => generateDeviceId(),
        metric: 'temperature',
        operator: '>',
        threshold: 30,
      },
    ],
    actions: [
      {
        type: 'alert',
        message: 'Temperature too high!',
      },
    ],
  },
};
