import { useState, useEffect, useCallback, useRef, DependencyList } from 'react'

export interface UseApiDataOptions<T> {
  deps?: DependencyList
  immediate?: boolean
  onSuccess?: (data: T) => void
  onError?: (error: Error) => void
}

export interface UseApiDataReturn<T> {
  data: T | null
  loading: boolean
  error: Error | null
  refetch: () => Promise<void>
  setData: (data: T) => void
}

/**
 * Generic data fetching hook
 *
 * Handles loading, error, and data states for API calls.
 *
 * @example
 * const { data, loading, error, refetch } = useApiData(
 *   () => api.getDevices(),
 *   { immediate: true }
 * )
 */
export function useApiData<T>(
  apiCall: () => Promise<T>,
  options: UseApiDataOptions<T> = {}
): UseApiDataReturn<T> {
  const { deps = [], immediate = true, onSuccess, onError } = options
  const [data, setData] = useState<T | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<Error | null>(null)

  // Use refs to store the latest callbacks without causing fetch to change
  const apiCallRef = useRef(apiCall)
  const onSuccessRef = useRef(onSuccess)
  const onErrorRef = useRef(onError)

  // Update refs when callbacks change
  useEffect(() => {
    apiCallRef.current = apiCall
    onSuccessRef.current = onSuccess
    onErrorRef.current = onError
  }, [apiCall, onSuccess, onError])

  // fetch function only depends on refs, so it stays stable
  const fetch = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const result = await apiCallRef.current()
      setData(result)
      onSuccessRef.current?.(result)
    } catch (err) {
      const error = err instanceof Error ? err : new Error('请求失败')
      setError(error)
      onErrorRef.current?.(error)
    } finally {
      setLoading(false)
    }
  }, []) // Empty deps array - fetch function is stable

  useEffect(() => {
    if (immediate) {
      fetch()
    }
  }, [fetch, immediate, ...deps])

  return {
    data,
    loading,
    error,
    refetch: fetch,
    setData,
  }
}

/**
 * Paginated data fetching hook
 */
export interface UsePaginatedDataOptions {
  pageSize?: number
  immediate?: boolean
}

export interface UsePaginatedDataReturn<T> {
  data: T[]
  loading: boolean
  error: Error | null
  total: number
  page: number
  pageSize: number
  totalPages: number
  setPage: (page: number) => void
  refresh: () => Promise<void>
}

/**
 * Hook for paginated API calls
 *
 * @example
 * const { data, page, setPage, totalPages, loading } = usePaginatedData(
 *   (page, limit) => api.listCommands({ page, limit }),
 *   { pageSize: 20 }
 * )
 */
export function usePaginatedData<T>(
  apiCall: (page: number, limit: number) => Promise<{ data: T[]; total: number }>,
  options: UsePaginatedDataOptions = {}
): UsePaginatedDataReturn<T> {
  const { pageSize = 20, immediate = true } = options

  const [data, setData] = useState<T[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<Error | null>(null)
  const [page, setPage] = useState(1)
  const [total, setTotal] = useState(0)

  // Use ref to store the latest apiCall
  const apiCallRef = useRef(apiCall)

  useEffect(() => {
    apiCallRef.current = apiCall
  }, [apiCall])

  const fetch = useCallback(async (currentPage: number = page) => {
    setLoading(true)
    setError(null)
    try {
      const result = await apiCallRef.current(currentPage, pageSize)
      setData(result.data)
      setTotal(result.total)
    } catch (err) {
      setError(err instanceof Error ? err : new Error('请求失败'))
    } finally {
      setLoading(false)
    }
  }, [pageSize, page])

  useEffect(() => {
    if (immediate) {
      fetch()
    }
  }, [fetch, immediate])

  const handleSetPage = useCallback((newPage: number) => {
    setPage(newPage)
  }, [])

  const totalPages = Math.ceil(total / pageSize)

  return {
    data,
    loading,
    error,
    total,
    page,
    pageSize,
    totalPages,
    setPage: handleSetPage,
    refresh: () => fetch(page),
  }
}
