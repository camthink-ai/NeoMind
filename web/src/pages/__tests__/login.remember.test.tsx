/// "Remember me" on the login page must keep the user signed in WITHOUT
/// keeping the password. With the box ticked, a successful login remembers
/// the username only; without it, nothing is remembered. No storage entry may
/// ever carry the password — it used to be saved in plaintext under
/// `neomind_remembered_credentials`.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { LoginPage } from '../login'
import { useStore } from '@/store'

const PASSWORD = 'correct-horse-battery'

// jsdom has no ResizeObserver; the Radix checkbox measures itself on mount.
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

function everyStoredValue(): string[] {
  const read = (storage: Storage) =>
    Array.from({ length: storage.length }, (_, i) => storage.getItem(storage.key(i) ?? '') ?? '')
  return [...read(localStorage), ...read(sessionStorage)]
}

async function submitWith(username: string, remember: boolean) {
  // These two inputs are labelled by placeholder only (no <label for>).
  fireEvent.change(screen.getByPlaceholderText('auth:username'), { target: { value: username } })
  fireEvent.change(screen.getByPlaceholderText('auth:password'), { target: { value: PASSWORD } })
  if (remember) fireEvent.click(screen.getByRole('checkbox', { name: 'auth:rememberMe' }))
  else {
    // A remembered username makes the page start with the box ticked.
    const box = screen.getByRole('checkbox', { name: 'auth:rememberMe' })
    if (box.getAttribute('aria-checked') === 'true') fireEvent.click(box)
  }
  fireEvent.click(screen.getByRole('button', { name: 'auth:login' }))
  await waitFor(() => expect(useStore.getState().login).toHaveBeenCalledTimes(1))
}

describe('LoginPage — remember me never stores the password', () => {
  const login = vi.fn().mockResolvedValue(undefined)

  beforeEach(() => {
    localStorage.clear()
    sessionStorage.clear()
    login.mockClear()
    // The page reads `login` / `checkAuthStatus` off the store; replace both so
    // nothing reaches the network. Its own raw `fetch` calls get a rejection.
    useStore.setState({ login, checkAuthStatus: vi.fn() } as never)
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')))
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('remembers only the username when the box is ticked', async () => {
    render(
      <MemoryRouter>
        <LoginPage />
      </MemoryRouter>,
    )

    await submitWith('ming', true)

    await waitFor(() => expect(localStorage.getItem('neomind_remembered_username')).toBe('ming'))
    expect(useStore.getState().login).toHaveBeenCalledWith('ming', PASSWORD, true)
    expect(everyStoredValue().filter((value) => value.includes(PASSWORD))).toEqual([])
    expect(localStorage.getItem('neomind_remembered_credentials')).toBeNull()
  })

  it('remembers nothing when the box is not ticked', async () => {
    localStorage.setItem('neomind_remembered_username', 'ming')
    render(
      <MemoryRouter>
        <LoginPage />
      </MemoryRouter>,
    )

    await submitWith('ming', false)

    await waitFor(() => expect(localStorage.getItem('neomind_remembered_username')).toBeNull())
    expect(useStore.getState().login).toHaveBeenCalledWith('ming', PASSWORD, false)
    expect(everyStoredValue().filter((value) => value.includes(PASSWORD))).toEqual([])
  })

  it('prefills a remembered username and leaves the password field empty', async () => {
    localStorage.setItem('neomind_remembered_username', 'ming')
    render(
      <MemoryRouter>
        <LoginPage />
      </MemoryRouter>,
    )

    await waitFor(() => expect(screen.getByPlaceholderText('auth:username')).toHaveValue('ming'))
    expect(screen.getByPlaceholderText('auth:password')).toHaveValue('')
    expect(screen.getByRole('checkbox', { name: 'auth:rememberMe' })).toHaveAttribute('aria-checked', 'true')
  })
})
