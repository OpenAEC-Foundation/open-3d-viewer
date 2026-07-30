export function formatPoints(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
  return n.toString();
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  if (bytes >= 1_024) return `${(bytes / 1_024).toFixed(0)} KB`;
  return `${bytes} B`;
}

export type DisplayUnit = 'mm' | 'cm' | 'm';

export function formatDistance(meters: number, unit: DisplayUnit): string {
  switch (unit) {
    case 'mm': return `${(meters * 1000).toFixed(1)} mm`;
    case 'cm': return `${(meters * 100).toFixed(2)} cm`;
    case 'm':  return `${meters.toFixed(3)} m`;
  }
}
