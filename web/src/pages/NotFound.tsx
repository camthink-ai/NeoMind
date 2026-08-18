import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { FileQuestion } from 'lucide-react'
import { Button } from '@/components/ui/button'

export default function NotFound() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  return (
    <div className="flex min-h-screen flex-col items-center justify-center px-4 text-center animate-fade-in-up">
      <div className="mb-6 flex h-20 w-20 items-center justify-center rounded-xl bg-muted text-muted-foreground">
        <FileQuestion className="h-16 w-16" />
      </div>
      <h1 className="text-2xl font-bold">{t('notFound.title')}</h1>
      <p className="mt-2 max-w-sm text-sm text-muted-foreground leading-relaxed">
        {t('notFound.description')}
      </p>
      <div className="mt-8 flex items-center gap-3">
        <Button onClick={() => navigate('/chat')}>{t('notFound.goHome')}</Button>
        <Button variant="outline" onClick={() => navigate(-1)}>{t('notFound.goBack')}</Button>
      </div>
    </div>
  )
}
