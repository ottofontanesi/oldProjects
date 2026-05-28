# Requirements Document: Frontend App Shell

## Introduction

This document specifies the requirements for the top-level React application shell that provides navigation, routing, and layout for all frontend screens (dashboard, onboarding wizard, companion, settings, debug panels). Currently the React components exist in isolation — this feature connects them into a cohesive single-page application with proper routing and state management.

## Glossary

- **AppShell**: The top-level React component that provides layout (sidebar, header, content area) and routing.
- **Router**: The client-side routing system (React Router or equivalent) that maps URL paths to screen components.
- **NavigationSidebar**: The persistent sidebar showing available screens and current selection.
- **ScreenRegistry**: The mapping of route paths to React components.

## Requirements

### Requirement 1: Application Layout

**User Story:** As a ResonantOS user, I want a consistent layout with navigation, so that I can move between different screens easily.

#### Acceptance Criteria

1. THE AppShell SHALL provide a sidebar navigation (collapsible) on the left and a content area on the right.
2. THE sidebar SHALL show navigation items: Dashboard, Network, Models, Agents, Companion, Settings.
3. THE currently active screen SHALL be highlighted in the sidebar.
4. THE layout SHALL be responsive: sidebar collapses to icons on narrow screens (<768px).
5. THE header SHALL show: app name, connection status indicator, system tray minimize button.

### Requirement 2: Client-Side Routing

**User Story:** As a ResonantOS user, I want to navigate between screens without page reloads, so that the app feels native.

#### Acceptance Criteria

1. THE router SHALL support these routes: `/` (dashboard), `/network` (topology), `/models` (model management), `/agents` (workflow list), `/companion` (phone status), `/settings` (configuration), `/wizard` (onboarding), `/debug` (debug panels).
2. THE router SHALL use hash-based routing (`/#/path`) for Tauri compatibility (no server-side routing).
3. NAVIGATION SHALL preserve component state (e.g., scrolling position) when switching between screens.
4. THE router SHALL support deep linking (opening the app to a specific screen via URL).
5. UNKNOWN routes SHALL redirect to the dashboard.

### Requirement 3: Onboarding Flow Integration

**User Story:** As a first-time user, I want the app to show the onboarding wizard automatically, so that I can set up my network before seeing the dashboard.

#### Acceptance Criteria

1. IF the backend reports `first_run: true`, THE app SHALL render the onboarding wizard at `/wizard` and hide the sidebar.
2. AFTER the wizard completes, THE app SHALL navigate to `/` (dashboard) and show the sidebar.
3. THE wizard SHALL be accessible later from Settings for re-configuration.
4. THE wizard completion state SHALL be persisted (not shown again on restart).

### Requirement 4: Screen Components

**User Story:** As a developer, I want each screen to be a self-contained component, so that screens can be developed independently.

#### Acceptance Criteria

1. EACH screen SHALL be a lazy-loaded React component (code splitting for faster initial load).
2. EACH screen SHALL receive its data from hooks (useNodeStatus, usePlacementPlan, etc.) — not props from the shell.
3. THE shell SHALL provide a `DashboardProvider` context that initializes all data hooks once.
4. SCREENS SHALL handle their own loading states (skeleton UI while data loads).

### Requirement 5: Error Boundaries

**User Story:** As a ResonantOS user, I want screen crashes to be contained, so that one broken screen doesn't crash the whole app.

#### Acceptance Criteria

1. EACH screen SHALL be wrapped in a React Error Boundary.
2. IF a screen crashes, THE error boundary SHALL show a "Something went wrong" message with a retry button.
3. THE error boundary SHALL log the error to the console with component stack trace.
4. OTHER screens SHALL remain functional when one screen crashes.

### Requirement 6: Theme and Accessibility

**User Story:** As a ResonantOS user, I want the app to be accessible and support dark/light themes.

#### Acceptance Criteria

1. THE app SHALL support dark mode (default) and light mode, togglable from settings.
2. THE theme preference SHALL be persisted across restarts.
3. ALL interactive elements SHALL be keyboard-navigable (tab order, Enter/Space activation).
4. ALL screens SHALL pass WCAG 2.1 AA contrast requirements.
5. THE sidebar navigation SHALL be navigable via keyboard (arrow keys + Enter).

### Requirement 7: Loading and Connection States

**User Story:** As a ResonantOS user, I want clear feedback when the app is loading or disconnected from the backend.

#### Acceptance Criteria

1. DURING startup (before backend is ready), THE app SHALL show a loading screen with progress indication.
2. IF the backend connection is lost, THE app SHALL show a banner: "Backend disconnected — reconnecting..."
3. WHEN the backend reconnects, THE banner SHALL disappear and data SHALL refresh.
4. THE loading screen SHALL timeout after 10 seconds and show a "Backend failed to start" error with retry option.
