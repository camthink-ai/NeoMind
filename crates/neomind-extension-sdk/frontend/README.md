# NeoMind Extension Frontend SDK

This directory contains templates and utilities for building frontend components for NeoMind extensions.

## Overview

Extensions can provide custom frontend components that will be displayed in the NeoMind dashboard. This SDK provides:

- **TypeScript type definitions** for component props and API calls
- **React component templates** for building cards, widgets, and panels
- **Build configuration templates** (Vite, TypeScript)
- **Manifest schema** for declaring frontend components

## Directory Structure

```
frontend/
├── src/
│   ├── types.ts          # TypeScript type definitions
│   ├── component.tsx     # React component template
│   └── index.ts          # Main exports
├── package.template.json # Template for package.json
├── vite.template.ts      # Template for Vite configuration
├── tsconfig.template.json # Template for TypeScript config
└── manifest.schema.json  # JSON schema for frontend manifest
```

## Quick Start

### 1. Create Frontend Directory

In your extension directory, create a `frontend/` folder:

```bash
mkdir -p extensions/my-extension/frontend/src
cd extensions/my-extension/frontend
```

### 2. Initialize Package

Copy the template files:

```bash
cp /path/to/sdk/frontend/package.template.json package.json
cp /path/to/sdk/frontend/vite.template.ts vite.config.ts
cp /path/to/sdk/frontend/tsconfig.template.json tsconfig.json
```

Update `package.json` with your extension name:

```json
{
  "name": "@your-org/my-extension-frontend",
  "version": "1.0.0"
}
```

### 3. Create Component

Create `src/index.tsx`:

```tsx
import { forwardRef, useState, useEffect, useCallback } from 'react'
import {
  type ExtensionComponentProps,
  executeExtensionCommand,
} from '@neomind/extension-sdk/frontend'

interface MyData {
  value: number
  timestamp: string
}

export const MyCard = forwardRef<HTMLDivElement, ExtensionComponentProps>(
  function MyCard(props, ref) {
    const { title = 'My Extension', dataSource, className = '' } = props
    const [data, setData] = useState<MyData | null>(null)
    const [loading, setLoading] = useState(true)

    const extensionId = dataSource?.extensionId || 'my-extension'

    const fetchData = useCallback(async () => {
      setLoading(true)
      try {
        const result = await executeExtensionCommand<MyData>(
          extensionId,
          'get_status',
          {}
        )
        if (result.success && result.data) {
          setData(result.data)
        }
      } finally {
        setLoading(false)
      }
    }, [extensionId])

    useEffect(() => {
      fetchData()
    }, [fetchData])

    return (
      <div ref={ref} className={className}>
        {loading ? (
          <div>Loading...</div>
        ) : data ? (
          <div>
            <h3>{title}</h3>
            <p>Value: {data.value}</p>
            <p>Updated: {new Date(data.timestamp).toLocaleString()}</p>
          </div>
        ) : (
          <div>No data</div>
        )}
      </div>
    )
  }
)

export default { MyCard }
```

### 4. Build

```bash
npm install
npm run build
```

### 5. Create Manifest

Create `frontend.json` in your extension's manifest:

```json
{
  "id": "my-extension",
  "version": "1.0.0",
  "entrypoint": "my-extension-components.umd.js",
  "components": [
    {
      "name": "MyCard",
      "type": "card",
      "displayName": "My Extension Card",
      "description": "Displays data from my extension",
      "defaultSize": { "width": 300, "height": 200 },
      "refreshable": true,
      "refreshInterval": 5000
    }
  ]
}
```

## Component Types

| Type | Description |
|------|-------------|
| `card` | Dashboard card component |
| `widget` | Interactive widget |
| `panel` | Full-height panel |
| `dialog` | Modal dialog |
| `settings` | Settings configuration |

## API Reference

### `executeExtensionCommand<T>(extensionId, command, args)`

Execute a command on an extension:

```typescript
const result = await executeExtensionCommand<{ status: string }>(
  'my-extension',
  'get_status',
  { detailed: true }
)

if (result.success) {
  console.log(result.data.status)
}
```

### `getExtensionMetrics(extensionId)`

Get current metrics from an extension:

```typescript
const result = await getExtensionMetrics('my-extension')
if (result.success) {
  console.log(result.data) // { metric1: 42, metric2: "hello" }
}
```

### `getExtensionInfo(extensionId)`

Get extension metadata:

```typescript
const result = await getExtensionInfo('my-extension')
if (result.success) {
  console.log(result.data.name)
  console.log(result.data.commands)
}
```

## Styling

### CSS Variables

Components can use these CSS variables for theming:

```css
.my-component {
  --ext-bg: rgba(255, 255, 255, 0.25);
  --ext-fg: hsl(240 10% 10%);
  --ext-muted: hsl(240 5% 40%);
  --ext-border: rgba(255, 255, 255, 0.5);
  --ext-accent: hsl(221 83% 53%);
}
```

### Dark Mode

Dark mode is applied via `.dark` class on parent:

```css
.dark .my-component {
  --ext-bg: rgba(30, 30, 30, 0.4);
  --ext-fg: hsl(0 0% 95%);
}
```

## Rust Integration

In your extension's Rust code, declare the frontend manifest:

```rust
use neomind_extension_sdk::{
    FrontendManifestBuilder,
    FrontendComponentType,
};

fn get_frontend_manifest() -> FrontendManifest {
    FrontendManifestBuilder::new("my-extension", "1.0.0")
        .entrypoint("my-extension-components.umd.js")
        .card("MyCard", "My Extension Card")
        .build()
}
```

## Best Practices

1. **Small bundle size**: Keep components under 100KB
2. **Use host's React**: Don't bundle React; use the host's version
3. **Graceful degradation**: Handle API errors gracefully
4. **Responsive design**: Support different container sizes
5. **Accessibility**: Include ARIA labels and keyboard navigation
6. **Dark mode**: Test both light and dark themes

## Examples

See the following extensions for complete examples:

- `extensions/weather-forecast-v2/frontend/` - Weather card component
- `extensions/image-analyzer-v2/frontend/` - Image analysis widget
- `extensions/yolo-video-v2/frontend/` - Video stream panel
