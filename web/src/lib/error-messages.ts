/**
 * Maps raw errors to user-friendly messages for toast notifications.
 *
 * This module provides a centralized way to convert technical error objects
 * into human-readable messages suitable for display in toast notifications.
 * It integrates with the existing error utilities in @/lib/errors.
 */

import { getErrorMessage, isNetworkError } from '@/lib/errors'

interface ApiErrorLike {
  status?: number;
  message?: string;
}

function isApiError(error: unknown): error is ApiErrorLike {
  return (
    typeof error === "object" &&
    error !== null &&
    ("status" in error || "statusCode" in error)
  );
}

/**
 * Maps an error to a user-friendly message suitable for toast notifications.
 *
 * Handles:
 * - API errors with HTTP status codes (400, 401, 403, 404, 409, 422, 500, 502, 503)
 * - Network errors (fetch failures, connection refused)
 * - AbortError (cancelled requests)
 * - Generic Error objects with technical messages
 * - String errors
 */
export function getUserFriendlyError(error: unknown): string {
  // Handle API errors with status codes
  if (isApiError(error)) {
    const status = (error as ApiErrorLike).status || (error as any).statusCode;
    const serverMessage = (error as ApiErrorLike).message;
    switch (status) {
      case 400:
        return serverMessage || "Invalid request. Please check your input.";
      case 401:
        return "Session expired. Please refresh the page.";
      case 403:
        return "You don't have permission to perform this action.";
      case 404:
        return "The requested resource was not found.";
      case 409:
        return "A conflict occurred. The resource may already exist.";
      case 422:
        return serverMessage || "Invalid data provided. Please check your input.";
      case 500:
        return "Server error. Please try again later.";
      case 502:
        return "Service temporarily unavailable.";
      case 503:
        return "Service is starting up. Please wait a moment.";
      default:
        return (error as ApiErrorLike).message || "An unexpected error occurred.";
    }
  }

  // Use existing error classification from @/lib/errors
  if (isNetworkError(error)) {
    const message = getErrorMessage(error).toLowerCase();
    if (message.includes("timeout")) {
      return "Request timed out. Please try again later.";
    }
    return "Unable to connect to the server. Is it running?";
  }

  // Handle AbortError
  if (error instanceof DOMException && error.name === "AbortError") {
    return "Request was cancelled.";
  }

  // Handle string errors
  if (typeof error === "string") {
    return error;
  }

  // Handle Error objects - check for network-like messages in the raw message
  if (error instanceof Error) {
    const msg = error.message.toLowerCase();
    if (msg.includes("fetch") || msg.includes("failed to fetch") || msg.includes("load failed")) {
      return "Unable to connect to the server. Is it running?";
    }
    if (msg.includes("timeout")) {
      return "Request timed out. Please try again later.";
    }
    // For API errors that arrive as Error with message from api.ts parseErrorMessage,
    // pass through the server-provided message (it's already somewhat user-friendly)
    return error.message;
  }

  // Handle errors with message property
  if (error && typeof error === "object") {
    const err = error as Record<string, unknown>;
    if (typeof err.message === "string") {
      return err.message;
    }
  }

  return "An unexpected error occurred.";
}

/**
 * Shows an error toast with a user-friendly message.
 * Usage: showErrorToast(toast, error)
 *
 * @param toastFn - The toast function from useToast()
 * @param error - The error to display
 * @param title - Optional title, defaults to "Error"
 */
export function showErrorToast(
  toastFn: (opts: any) => any,
  error: unknown,
  title = "Error"
) {
  const description = getUserFriendlyError(error);
  toastFn({
    title,
    description,
    variant: "destructive",
  });
}
