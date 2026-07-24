// Main dialog container
export {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
  FullScreenDialogSidebar,
  FullScreenDialogMain,
} from './FullScreenDialog'
export type {
  FullScreenDialogProps,
  FullScreenDialogHeaderProps,
  FullScreenDialogContentProps,
  FullScreenDialogFooterProps,
  FullScreenDialogSidebarProps,
  FullScreenDialogMainProps,
} from './FullScreenDialog'

// Step progress
export {
  ProgressStepper,
  VerticalStepper,
  HorizontalStepper,
} from './ProgressStepper'
export type {
  Step,
  StepStatus,
  ProgressStepperProps,
  VerticalStepperProps,
  HorizontalStepperProps,
} from './ProgressStepper'

// Re-export commonly used icons for convenience
export { X, Check, ArrowLeft, Save, Loader2, Play } from 'lucide-react'
