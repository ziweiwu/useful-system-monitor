/** Human-readable formatting. Units are dimmed separately in the UI. */

export function bytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '—';
  if (n < 1024) return `${n}B`;
  const units = ['K', 'M', 'G', 'T'];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return v >= 100 ? `${Math.round(v)}${units[i]}` : `${v.toFixed(1)}${units[i]}`;
}

export function percent(n: number | null, digits = 1): string {
  return n === null ? '—' : n.toFixed(digits);
}

export function duration(sec: number): string {
  const d = Math.floor(sec / 86400);
  const h = Math.floor((sec % 86400) / 3600);
  const m = Math.floor((sec % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function minutesToHm(min: number | null): string {
  if (min === null) return '—';
  const h = Math.floor(min / 60);
  const m = min % 60;
  return `${h}:${String(m).padStart(2, '0')}`;
}

export function clockTime(ms: number): string {
  const d = new Date(ms);
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, '0'))
    .join(':');
}

/** "3s ago" style sample age — panels are per-tier, so age is visible (I-4). */
export function age(sampledAt: number, now: number): string {
  const s = Math.max(0, Math.round((now - sampledAt) / 1000));
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m`;
}
