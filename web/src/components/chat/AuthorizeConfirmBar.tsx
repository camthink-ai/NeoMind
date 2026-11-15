/**
 * Explicit Unknown re-run confirm buttons.
 *
 * Multi-candidate → user picks one op; phrase shortcuts only work when exactly
 * one active pending confirm exists (resolved by the parent, not by history
 * body inference).
 */

import { useTranslation } from "react-i18next"
import { RefreshCw } from "lucide-react"
import { Button } from "@/components/ui/button"
import type { AuthorizeNewOperationConfirm } from "@/lib/chat/authorizeNewOperation"

interface AuthorizeConfirmBarProps {
  pending: AuthorizeNewOperationConfirm[]
  disabled?: boolean
  onConfirm: (confirm: AuthorizeNewOperationConfirm) => void
}

function shortOpId(id: string): string {
  if (id.length <= 12) return id
  return `${id.slice(0, 8)}…`
}

export function AuthorizeConfirmBar({
  pending,
  disabled,
  onConfirm,
}: AuthorizeConfirmBarProps) {
  const { t } = useTranslation("chat")
  if (pending.length === 0) return null

  return (
    <div
      className="mb-2 rounded-lg border border-border bg-muted-30 px-3 py-2"
      role="group"
      aria-label={t("authorizeConfirm.title")}
    >
      <p className="text-xs text-muted-foreground mb-1.5">
        {pending.length > 1
          ? t("authorizeConfirm.pickOne")
          : t("authorizeConfirm.title")}
      </p>
      <div className="flex flex-wrap gap-1.5">
        {pending.map((confirm) => (
          <Button
            key={confirm.requestId}
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled}
            className="h-8 gap-1.5 text-xs"
            onClick={() => onConfirm(confirm)}
          >
            <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
            {t("authorizeConfirm.button", {
              opId: shortOpId(confirm.priorOperationId),
            })}
          </Button>
        ))}
      </div>
    </div>
  )
}
