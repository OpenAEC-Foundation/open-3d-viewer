export function WireframeIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 2L2 7l10 5 10-5-10-5z" />
      <path d="M2 17l10 5 10-5" />
      <path d="M2 12l10 5 10-5" />
    </svg>
  );
}

export function SolidIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 2L2 7l10 5 10-5-10-5z" fill="currentColor" fillOpacity="0.3" />
      <path d="M2 17l10 5 10-5" />
      <path d="M2 12l10 5 10-5" />
    </svg>
  );
}

export function PointsIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <circle cx="5" cy="5" r="1.5" fill="currentColor" />
      <circle cx="12" cy="3" r="1.5" fill="currentColor" />
      <circle cx="19" cy="6" r="1.5" fill="currentColor" />
      <circle cx="7" cy="12" r="1.5" fill="currentColor" />
      <circle cx="14" cy="11" r="1.5" fill="currentColor" />
      <circle cx="4" cy="19" r="1.5" fill="currentColor" />
      <circle cx="11" cy="18" r="1.5" fill="currentColor" />
      <circle cx="18" cy="16" r="1.5" fill="currentColor" />
    </svg>
  );
}

export function HiddenLineIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {/* Cube front face */}
      <path d="M4 8L12 4L20 8L12 12Z" fill="rgba(255,255,255,0.15)" />
      {/* Visible edges - solid */}
      <path d="M4 8L12 12L20 8" />
      <path d="M12 12V20" />
      <path d="M4 8V16L12 20L20 16V8" />
      <path d="M12 4L20 8" />
      <path d="M12 4L4 8" />
      {/* Hidden edges - dashed */}
      <line x1="4" y1="16" x2="12" y2="12" strokeDasharray="2 2" opacity="0.4" />
    </svg>
  );
}

export function SectionBoxIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="18" height="18" rx="1" strokeDasharray="4 2" />
      <line x1="3" y1="12" x2="21" y2="12" />
      <line x1="12" y1="3" x2="12" y2="21" />
    </svg>
  );
}

export function SectionPlaneIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="4" y="9" width="12" height="11" rx="1" strokeDasharray="3 2" />
      <path d="M2 14L12 4L22 8L12 18Z" fill="currentColor" fillOpacity="0.25" />
      <line x1="17" y1="11" x2="20" y2="5" />
      <path d="M18 5.5L20 5L20.5 7" />
    </svg>
  );
}

export function OrbitIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <ellipse cx="12" cy="12" rx="10" ry="4" />
    </svg>
  );
}

export function PerspectiveIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 4L18 4L21 20H3L6 4Z" />
      <line x1="12" y1="4" x2="12" y2="20" strokeDasharray="3 2" />
    </svg>
  );
}

export function OrthoIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="4" y="4" width="16" height="16" />
      <line x1="12" y1="4" x2="12" y2="20" strokeDasharray="3 2" />
    </svg>
  );
}

export function WalkIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="5" r="2" />
      <path d="M10 22V16l-2-4 4-2 4 2-2 4v6" />
    </svg>
  );
}

// ============================================================================
// Measure tools
// ============================================================================

export function MeasurePointIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 20L12 16L20 20" opacity="0.5" />
      <line x1="12" y1="16" x2="12" y2="4" opacity="0.5" />
      <rect x="9.5" y="13.5" width="5" height="5" fill="currentColor" fillOpacity="0.3" />
      <path d="M15 6h5M15 9h3" />
    </svg>
  );
}

export function MeasureDistanceIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <line x1="5" y1="19" x2="19" y2="5" />
      <rect x="3" y="17" width="4" height="4" fill="currentColor" fillOpacity="0.3" />
      <rect x="17" y="3" width="4" height="4" fill="currentColor" fillOpacity="0.3" />
      <path d="M9 13l2 2M13 9l2 2" opacity="0.6" />
    </svg>
  );
}

export function MeasurePerpendicularIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <line x1="3" y1="20" x2="21" y2="20" />
      <line x1="12" y1="20" x2="12" y2="5" />
      <path d="M12 16h4v4" opacity="0.7" />
      <rect x="10" y="3" width="4" height="4" fill="currentColor" fillOpacity="0.3" />
    </svg>
  );
}

export function MeasureAreaIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 9L12 4L21 9L12 14Z" fill="currentColor" fillOpacity="0.35" />
      <path d="M3 9V15L12 20V14" opacity="0.6" />
      <path d="M21 9V15L12 20" opacity="0.6" />
    </svg>
  );
}

export function MeasurePolygonIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 18L6 6L15 4L20 12L13 20Z" fill="currentColor" fillOpacity="0.25" />
      <circle cx="4" cy="18" r="1.5" fill="currentColor" />
      <circle cx="6" cy="6" r="1.5" fill="currentColor" />
      <circle cx="15" cy="4" r="1.5" fill="currentColor" />
      <circle cx="20" cy="12" r="1.5" fill="currentColor" />
      <circle cx="13" cy="20" r="1.5" fill="currentColor" />
    </svg>
  );
}

export function MeasureVolumeIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 3L21 8V16L12 21L3 16V8Z" fill="currentColor" fillOpacity="0.25" />
      <path d="M3 8L12 13L21 8M12 13V21" />
    </svg>
  );
}

export function MeasureWeightIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="5.5" r="2.5" />
      <path d="M6.5 9h11l2.5 11H4Z" fill="currentColor" fillOpacity="0.25" />
      <text x="12" y="17.5" fontSize="6.5" fontWeight="700" textAnchor="middle" fill="currentColor" stroke="none">kg</text>
    </svg>
  );
}

export function MeasureCountIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="7" height="7" rx="1" fill="currentColor" fillOpacity="0.25" />
      <rect x="14" y="3" width="7" height="7" rx="1" fill="currentColor" fillOpacity="0.25" />
      <rect x="3" y="14" width="7" height="7" rx="1" fill="currentColor" fillOpacity="0.25" />
      <path d="M14.5 17.5l2 2 4-4.5" />
    </svg>
  );
}

export function SnapVertexIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 20L4 8M4 20H16" opacity="0.5" />
      <rect x="1" y="17" width="6" height="6" />
    </svg>
  );
}

export function SnapMidpointIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <line x1="2" y1="18" x2="22" y2="18" opacity="0.5" />
      <path d="M12 11L16 18H8Z" />
    </svg>
  );
}

export function SnapEdgeIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <line x1="3" y1="21" x2="21" y2="3" strokeWidth="2.6" />
      <path d="M11 9l4 4M9 11l4 4" opacity="0.6" />
    </svg>
  );
}

export function SnapFaceIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2 10L12 5L22 10L12 15Z" fill="currentColor" fillOpacity="0.35" />
      <rect x="9" y="7.5" width="6" height="5" opacity="0.8" />
    </svg>
  );
}

export function MeasureComponentsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="4" y1="20" x2="20" y2="4" stroke="currentColor" />
      <line x1="4" y1="20" x2="14" y2="20" stroke="#ff5252" />
      <line x1="14" y1="20" x2="20" y2="14" stroke="#4cd964" />
      <line x1="20" y1="14" x2="20" y2="4" stroke="#4d94ff" />
    </svg>
  );
}

export function MeasureAngleIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <line x1="3" y1="20" x2="21" y2="20" />
      <line x1="3" y1="20" x2="15" y2="5" />
      <path d="M11 20A8 8 0 0 0 8 13.8" />
      <path d="M8.9 11.9L8 13.8L10.1 13.9" />
    </svg>
  );
}

export function MultiSelectIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="11" height="11" rx="1.5" strokeDasharray="3 2" />
      <rect x="10" y="10" width="11" height="11" rx="1.5" fill="currentColor" fillOpacity="0.25" />
      <path d="M13 16h5M15.5 13.5v5" />
    </svg>
  );
}

export function MeasureUnitsIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="8" width="20" height="8" rx="1" />
      <path d="M6 8v3M10 8v4M14 8v3M18 8v4" />
      <text x="12" y="22.5" fontSize="5.5" textAnchor="middle" fill="currentColor" stroke="none">0,00</text>
    </svg>
  );
}

export function ExplodeStoreysIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {/* Three floor slabs pulled apart */}
      <path d="M4 5L12 2L20 5L12 8Z" fill="currentColor" fillOpacity="0.25" />
      <path d="M4 12L12 9L20 12L12 15Z" fill="currentColor" fillOpacity="0.25" />
      <path d="M4 19L12 16L20 19L12 22Z" fill="currentColor" fillOpacity="0.25" />
    </svg>
  );
}

export function ExplodeElementsIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {/* Four blocks moving away from the centre */}
      <rect x="2" y="2" width="7" height="7" rx="1" />
      <rect x="15" y="2" width="7" height="7" rx="1" />
      <rect x="2" y="15" width="7" height="7" rx="1" />
      <rect x="15" y="15" width="7" height="7" rx="1" />
      <path d="M10.5 10.5L12 12M13.5 10.5L12 12M10.5 13.5L12 12M13.5 13.5L12 12" opacity="0.6" />
    </svg>
  );
}
