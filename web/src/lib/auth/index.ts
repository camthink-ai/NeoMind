/**
 * Authentication Module
 *
 * Exports unified authentication utilities.
 */

export { tokenManager } from './tokenManager'
export { LEGACY_CREDENTIALS_KEY, purgeLegacyCredentials } from './legacyCredentials'
export {
  getToken,
  setToken,
  clearToken,
  hasToken,
  getUser,
  setUser,
  clearUser,
  clearAll,
} from './tokenManager'
