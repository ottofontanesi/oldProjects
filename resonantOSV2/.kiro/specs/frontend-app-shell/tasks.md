# Implementation Plan: Frontend App Shell

## Overview

Create the top-level React application shell with routing, navigation sidebar, error boundaries, theme support, and loading states. Connects all existing screen components into a cohesive SPA.

**Build verification:** `npx tsc --noEmit` and `npx vitest --run` from `src/resonantos-vnext`.

## Tasks

- [ ] 1. Core shell components
  - [x] 1.1 Create `App.tsx` with HashRouter and route definitions
    - Hash-based routing for Tauri compatibility
    - All routes defined with lazy-loaded components
    - First-run detection (show wizard vs dashboard)
    - _Requirements: 2.1, 2.2, 2.5, 3.1, 3.2, 3.3_

  - [x] 1.2 Create `AppLayout.tsx` with sidebar + content area
    - Responsive layout (sidebar collapses <768px)
    - Header with app name, connection status, minimize button
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [x] 1.3 Create `NavigationSidebar.tsx`
    - Navigation items: Dashboard, Network, Models, Agents, Companion, Settings
    - Active item highlighting
    - Keyboard navigation (arrow keys + Enter)
    - Collapsible to icons on narrow screens
    - _Requirements: 1.2, 1.3, 1.4, 6.5_

- [ ] 2. Error handling and loading
  - [x] 2.1 Create `ErrorBoundary.tsx`
    - Wrap each screen in error boundary
    - Show "Something went wrong" with retry button
    - Log error with component stack
    - Other screens remain functional
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

  - [x] 2.2 Create `LoadingScreen.tsx`
    - Show during startup (before backend ready)
    - Timeout after 10 seconds with error + retry
    - _Requirements: 7.1, 7.4_

  - [x] 2.3 Create `ConnectionBanner.tsx`
    - Show when backend disconnected
    - Auto-hide when reconnected
    - _Requirements: 7.2, 7.3_

- [ ] 3. Theme and accessibility
  - [x] 3.1 Create `ThemeProvider.tsx`
    - Dark mode (default) and light mode
    - Persist preference to localStorage
    - Toggle from settings
    - _Requirements: 6.1, 6.2_

  - [x] 3.2 Implement keyboard navigation
    - All interactive elements tab-navigable
    - Sidebar navigable via keyboard
    - WCAG 2.1 AA contrast
    - _Requirements: 6.3, 6.4, 6.5_

- [ ] 4. Screen wrappers (lazy loading)
  - [x] 4.1 Create lazy-loaded screen wrappers
    - Each screen as React.lazy() with Suspense fallback
    - Screens get data from hooks (not props)
    - DashboardProvider context wraps all screens
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

  - [x] 4.2 Create `useAppState.ts` hook
    - Detect first-run via Tauri command
    - Track backend readiness
    - Provide routing decision to App.tsx
    - _Requirements: 3.1, 3.4, 7.1_

- [ ] 5. Onboarding integration
  - [x] 5.1 Wire onboarding wizard as first-run entry point
    - Full-screen (no sidebar) during wizard
    - Navigate to dashboard on completion
    - Accessible later from Settings
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [x] 6. Final checkpoint
  - Verify `npx tsc --noEmit` passes.
  - Verify `npx vitest --run` passes.

## Notes

- Uses react-router-dom v6 with HashRouter
- All screens already exist as components — this just connects them
- The DashboardProvider from dashboard-data-polling provides all data hooks
- Theme CSS uses CSS custom properties (variables) for easy switching
