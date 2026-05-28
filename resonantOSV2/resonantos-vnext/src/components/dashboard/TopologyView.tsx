// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// TopologyView — force-directed graph of network nodes and connections

import React, { useState, useCallback } from 'react';
import type { NodeInfo, ConnectionInfo } from './types/dashboard';
import { TRANSPORT_COLORS, DEVICE_ICONS } from './utils/colors';

interface TopologyViewProps {
  nodes: NodeInfo[];
  connections: ConnectionInfo[];
  onNodeClick?: (nodeId: string) => void;
  selectedNodeId?: string | null;
}

export const TopologyView = React.memo(function TopologyView({ nodes, connections, onNodeClick, selectedNodeId }: TopologyViewProps) {
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });

  const handleZoomIn = useCallback(() => setZoom(z => Math.min(z * 1.2, 3)), []);
  const handleZoomOut = useCallback(() => setZoom(z => Math.max(z / 1.2, 0.3)), []);
  const handleReset = useCallback(() => { setZoom(1); setPan({ x: 0, y: 0 }); }, []);

  if (nodes.length === 0) {
    return (
      <div className="topology-view" role="region" aria-label="Network topology">
        <h3>Network Topology</h3>
        <p className="empty-state">No nodes discovered yet</p>
      </div>
    );
  }

  // Simple circular layout (in production, use d3-force)
  const centerX = 300;
  const centerY = 200;
  const radius = Math.min(150, nodes.length * 20);

  const nodePositions = nodes.map((node, i) => {
    const angle = (2 * Math.PI * i) / nodes.length;
    const x = node.position?.x ?? centerX + radius * Math.cos(angle);
    const y = node.position?.y ?? centerY + radius * Math.sin(angle);
    return { ...node, x, y };
  });

  return (
    <div className="topology-view" role="region" aria-label="Network topology">
      <div className="topology-header">
        <h3>Network Topology</h3>
        <div className="topology-controls">
          <button type="button" onClick={handleZoomIn} aria-label="Zoom in">+</button>
          <button type="button" onClick={handleZoomOut} aria-label="Zoom out">−</button>
          <button type="button" onClick={handleReset} aria-label="Reset view">⟲</button>
        </div>
      </div>

      <svg
        className="topology-svg"
        viewBox="0 0 600 400"
        style={{ transform: `scale(${zoom}) translate(${pan.x}px, ${pan.y}px)` }}
        role="img"
        aria-label="Network topology graph"
      >
        {/* Connections */}
        {connections.map((conn, i) => {
          const source = nodePositions.find(n => n.nodeId === conn.sourceNode);
          const target = nodePositions.find(n => n.nodeId === conn.targetNode);
          if (!source || !target) return null;

          const color = TRANSPORT_COLORS[conn.transport];
          const thickness = Math.max(1, Math.min(4, conn.bandwidthMbps / 250));

          return (
            <line
              key={i}
              x1={source.x}
              y1={source.y}
              x2={target.x}
              y2={target.y}
              stroke={color}
              strokeWidth={thickness}
              strokeDasharray={conn.isHealthy ? undefined : '4,4'}
              opacity={conn.isHealthy ? 0.8 : 0.4}
            />
          );
        })}

        {/* Nodes */}
        {nodePositions.map(node => {
          const isSelected = node.nodeId === selectedNodeId;
          const utilPercent = node.utilization.cpuPercent;

          return (
            <g
              key={node.nodeId}
              transform={`translate(${node.x}, ${node.y})`}
              onClick={() => onNodeClick?.(node.nodeId)}
              className="topology-node"
              role="button"
              tabIndex={0}
              aria-label={`${node.hostname} (${node.deviceType}, ${node.isOnline ? 'online' : 'offline'})`}
            >
              {/* Utilization ring */}
              <circle r="22" fill="none" stroke="#e5e7eb" strokeWidth="3" />
              <circle r="22" fill="none" stroke={node.isOnline ? '#22c55e' : '#ef4444'} strokeWidth="3"
                strokeDasharray={`${utilPercent * 1.38} 138`} transform="rotate(-90)" />

              {/* Node circle */}
              <circle r="18" fill={isSelected ? '#dbeafe' : '#ffffff'} stroke={isSelected ? '#3b82f6' : '#d1d5db'} strokeWidth="2" />

              {/* Status dot */}
              <circle cx="14" cy="-14" r="4" fill={node.isOnline ? '#22c55e' : '#ef4444'} />

              {/* Label */}
              <text y="35" textAnchor="middle" fontSize="10" fill="#374151">
                {node.hostname.length > 12 ? node.hostname.slice(0, 12) + '…' : node.hostname}
              </text>

              {/* Device icon */}
              <text textAnchor="middle" dominantBaseline="central" fontSize="14">
                {DEVICE_ICONS[node.deviceType]}
              </text>
            </g>
          );
        })}
      </svg>

      {/* Legend */}
      <div className="topology-legend" aria-label="Legend">
        {Object.entries(TRANSPORT_COLORS).map(([type, color]) => (
          <span key={type} className="legend-item">
            <span className="legend-line" style={{ backgroundColor: color }} aria-hidden="true" />
            {type.toUpperCase()}
          </span>
        ))}
      </div>
    </div>
  );
});
