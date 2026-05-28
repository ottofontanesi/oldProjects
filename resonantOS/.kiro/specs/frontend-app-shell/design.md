# Design Document: Frontend App Shell

## Overview

The top-level React application shell providing layout, routing, navigation, error boundaries, theme support, and loading states. Connects all existing screen components (dashboard, wizard, companion, settings) into a cohesive SPA with hash-based routing for Tauri compatibility.

### Design Principles

1. **Lazy loading**: Each screen is code-split for fast initial load.
2. **Error isolation**: Screen crashes are contained by error boundaries.
3. **Responsive**: Sidebar collapses on narrow screens.
4. **Accessible**: Full keyboard navigation, WCAG 2.1 AA contrast.
5. **Theme-aware**: Dark (default) and light modes with persistence.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         App.tsx                                   │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  ThemeProvider + DashboardProvider                         │    │
│  │                                                          │    │
│  │  ┌────────────┐  ┌──────────────────────────────────┐    │    │
│  │  │  Sidebar   │  │  Content Area (Router Outlet)    │    │    │
│  │  │            │  │                                  │    │    │
│  │  │  Dashboard │  │  ┌────────────────────────────┐  │    │    │
│  │  │  Network   │  │  │  ErrorBoundary             │  │    │    │
│  │  │  Models    │  │  │  ┌──────────────────────┐  │  │    │    │
│  │  │  Agents    │  │  │  │  <ActiveScreen />    │  │  │    │    │
│  │  │  Companion │  │  │  │  (lazy loaded)       │  │  │    │    │
│  │  │  Settings  │  │  │  └──────────────────────┘  │  │    │    │
│  │  │            │  │  └────────────────────────────┘  │    │    │
│  │  └────────────┘  └──────────────────────────────────┘    │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  ConnectionBanner (shown when backend disconnected)       │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Route Map

| Path | Component | Lazy | Sidebar Visible |
|------|-----------|------|-----------------|
| `/#/` | DashboardScreen | Yes | Yes |
| `/#/network` | NetworkTopologyScreen | Yes | Yes |
| `/#/models` | ModelManagementScreen | Yes | Yes |
| `/#/agents` | AgentWorkflowScreen | Yes | Yes |
| `/#/companion` | CompanionScreen | Yes | Yes |
| `/#/settings` | SettingsScreen | Yes | Yes |
| `/#/debug` | DebugPanelsScreen | Yes | Yes |
| `/#/wizard` | OnboardingWizard | Yes | **No** |
| `/#/*` | Redirect to `/` | — | — |

## Components

### App.tsx (root)

```tsx
export function App() {
    const { isFirstRun, isLoading } = useAppState();

    if (isLoading) return <LoadingScreen />;
    if (isFirstRun) return <OnboardingWizard onComplete={() => setFirstRun(false)} />;

    return (
        <ThemeProvider>
            <DashboardProvider>
                <HashRouter>
                    <AppLayout>
                        <Routes>
                            <Route path="/" element={<LazyDashboard />} />
                            <Route path="/network" element={<LazyNetwork />} />
                            <Route path="/models" element={<LazyModels />} />
                            <Route path="/agents" element={<LazyAgents />} />
                            <Route path="/companion" element={<LazyCompanion />} />
                            <Route path="/settings" element={<LazySettings />} />
                            <Route path="/debug" element={<LazyDebug />} />
                            <Route path="*" element={<Navigate to="/" />} />
                        </Routes>
                    </AppLayout>
                </HashRouter>
            </DashboardProvider>
        </ThemeProvider>
    );
}
```

### NavigationSidebar

```tsx
const NAV_ITEMS = [
    { path: '/', icon: '📊', label: 'Dashboard' },
    { path: '/network', icon: '🌐', label: 'Network' },
    { path: '/models', icon: '🧠', label: 'Models' },
    { path: '/agents', icon: '🤖', label: 'Agents' },
    { path: '/companion', icon: '📱', label: 'Companion' },
    { path: '/settings', icon: '⚙️', label: 'Settings' },
];
```

### ErrorBoundary

```tsx
class ScreenErrorBoundary extends React.Component {
    state = { hasError: false, error: null };

    static getDerivedStateFromError(error) {
        return { hasError: true, error };
    }

    componentDidCatch(error, info) {
        console.error('Screen crashed:', error, info.componentStack);
    }

    render() {
        if (this.state.hasError) {
            return <ErrorFallback error={this.state.error} onRetry={() => this.setState({ hasError: false })} />;
        }
        return this.props.children;
    }
}
```

### LoadingScreen

```tsx
function LoadingScreen() {
    const [timedOut, setTimedOut] = useState(false);

    useEffect(() => {
        const timer = setTimeout(() => setTimedOut(true), 10_000);
        return () => clearTimeout(timer);
    }, []);

    if (timedOut) {
        return <div>Backend failed to start. <button onClick={retry}>Retry</button></div>;
    }

    return <div className="loading-screen"><Spinner /> Starting ResonantOS...</div>;
}
```

## Theme System

```tsx
type Theme = 'dark' | 'light';

const ThemeContext = createContext<{ theme: Theme; toggle: () => void }>();

function ThemeProvider({ children }) {
    const [theme, setTheme] = useState<Theme>(() => {
        return localStorage.getItem('theme') as Theme || 'dark';
    });

    const toggle = () => {
        const next = theme === 'dark' ? 'light' : 'dark';
        setTheme(next);
        localStorage.setItem('theme', next);
        document.documentElement.setAttribute('data-theme', next);
    };

    return <ThemeContext.Provider value={{ theme, toggle }}>{children}</ThemeContext.Provider>;
}
```

## Correctness Properties

### Property 1: Route Validity
All defined routes SHALL render without crashing.

### Property 2: Error Containment
A crash in one screen SHALL NOT affect other screens.

### Property 3: First-Run Routing
First-run users SHALL see the wizard; returning users SHALL see the dashboard.

### Property 4: Keyboard Navigation
All navigation items SHALL be reachable via Tab + Enter.

## File Structure

```
src/resonantos-vnext/src/
├── App.tsx                     # Root component, router setup
├── AppLayout.tsx               # Sidebar + content area layout
├── components/
│   ├── NavigationSidebar.tsx   # Sidebar navigation
│   ├── ConnectionBanner.tsx    # Backend disconnection banner
│   ├── LoadingScreen.tsx       # Startup loading state
│   ├── ErrorBoundary.tsx       # Screen error boundary
│   └── ThemeProvider.tsx       # Dark/light theme context
├── screens/
│   ├── DashboardScreen.tsx     # Main dashboard (lazy)
│   ├── NetworkScreen.tsx       # Network topology (lazy)
│   ├── ModelsScreen.tsx        # Model management (lazy)
│   ├── AgentsScreen.tsx        # Agent workflows (lazy)
│   ├── CompanionScreen.tsx     # Phone companion (lazy)
│   ├── SettingsScreen.tsx      # App settings (lazy)
│   └── DebugScreen.tsx         # Debug panels (lazy)
└── hooks/
    └── useAppState.ts          # First-run detection, backend readiness
```
