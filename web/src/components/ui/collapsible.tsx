"use client"
import * as React from "react"
import * as CollapsiblePrimitive from "@radix-ui/react-collapsible"
import { cn } from "@/lib/utils"
import { forwardRef } from "react"

import type { ElementRef } from "react"

import type { ComponentPropsWithoutRef } from "react"

type CollapsibleProps = ComponentPropsWithoutRef<typeof CollapsiblePrimitive.Root> & {
  asChild?: boolean
  className?: string
  children?: React.ReactNode
}

const Collapsible = forwardRef<
  ElementRef<typeof CollapsiblePrimitive.Root>,
  CollapsibleProps
>((props, ref) => {
  const { className, children, ...rest } = props
  return (
    <CollapsiblePrimitive.Root
      {...rest}
      className={cn(className)}
      ref={ref}
    >
      {children}
    </CollapsiblePrimitive.Root>
  )
})
Collapsible.displayName = "Collapsible"

const CollapsibleTrigger = forwardRef<
  ElementRef<typeof CollapsiblePrimitive.Trigger>,
  ComponentPropsWithoutRef<typeof CollapsiblePrimitive.Trigger> & {
    asChild?: boolean
    className?: string
    children?: React.ReactNode
  }
>((props, ref) => {
  const { className, children, ...rest } = props
  return (
    <CollapsiblePrimitive.Trigger
      {...rest}
      className={cn(className)}
      ref={ref}
    >
      {children}
    </CollapsiblePrimitive.Trigger>
  )
})
CollapsibleTrigger.displayName = "CollapsibleTrigger"
type CollapsibleContentProps = ComponentPropsWithoutRef<typeof CollapsiblePrimitive.Content> & {
  asChild?: boolean
  className?: string
  children?: React.ReactNode
}
const CollapsibleContent = forwardRef<
  ElementRef<typeof CollapsiblePrimitive.Content>,
  CollapsibleContentProps
>((props, ref) => {
  const { className, children, ...rest } = props
  return (
    <CollapsiblePrimitive.Content
      {...rest}
      className={cn(className)}
      ref={ref}
    >
      {children}
    </CollapsiblePrimitive.Content>
  )
})
CollapsibleContent.displayName = "CollapsibleContent"
export { Collapsible, CollapsibleTrigger, CollapsibleContent }
