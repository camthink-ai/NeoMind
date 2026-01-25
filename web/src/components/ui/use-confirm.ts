import * as React from "react"

type ConfirmDialog = {
  id: string
  title?: React.ReactNode
  description?: React.ReactNode
  confirmText?: string
  cancelText?: string
  variant?: "default" | "destructive"
  resolve: (value: boolean) => void
}

const CONFIRM_LIMIT = 1

type ActionType =
  | { type: "ADD_CONFIRM"; dialog: ConfirmDialog }
  | { type: "REMOVE_CONFIRM"; dialogId?: string }

interface State {
  dialogs: ConfirmDialog[]
}

const reducer = (state: State, action: ActionType): State => {
  switch (action.type) {
    case "ADD_CONFIRM":
      return {
        ...state,
        dialogs: [action.dialog, ...state.dialogs].slice(0, CONFIRM_LIMIT),
      }
    case "REMOVE_CONFIRM":
      if (action.dialogId === undefined) {
        return {
          ...state,
          dialogs: [],
        }
      }
      return {
        ...state,
        dialogs: state.dialogs.filter((d) => d.id !== action.dialogId),
      }
  }
}

const listeners: Array<(state: State) => void> = []

let memoryState: State = { dialogs: [] }

function dispatch(action: ActionType) {
  memoryState = reducer(memoryState, action)
  listeners.forEach((listener) => {
    listener(memoryState)
  })
}

let count = 0

function genId() {
  count = (count + 1) % Number.MAX_SAFE_INTEGER
  return count.toString()
}

type ConfirmOptions = {
  title?: React.ReactNode
  description?: React.ReactNode
  confirmText?: string
  cancelText?: string
  variant?: "default" | "destructive"
}

/**
 * Show a confirmation dialog and return a promise that resolves to true/false
 * based on user choice.
 *
 * @example
 * ```tsx
 * const confirmed = await confirm({
 *   title: "Delete item?",
 *   description: "This action cannot be undone.",
 *   confirmText: "Delete",
 *   cancelText: "Cancel",
 *   variant: "destructive"
 * })
 *
 * if (confirmed) {
 *   // User clicked confirm
 * }
 * ```
 */
function confirm(options: ConfirmOptions = {}): Promise<boolean> {
  return new Promise((resolve) => {
    const id = genId()

    dispatch({
      type: "ADD_CONFIRM",
      dialog: {
        id,
        ...options,
        resolve,
      },
    })
  })
}

function useConfirm() {
  const [state, setState] = React.useState<State>(memoryState)

  React.useEffect(() => {
    listeners.push(setState)
    return () => {
      const index = listeners.indexOf(setState)
      if (index > -1) {
        listeners.splice(index, 1)
      }
    }
  }, [state])

  return {
    ...state,
    confirm,
    close: (dialogId?: string) =>
      dispatch({ type: "REMOVE_CONFIRM", dialogId }),
  }
}

export { useConfirm, confirm }
