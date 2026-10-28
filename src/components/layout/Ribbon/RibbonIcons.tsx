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
