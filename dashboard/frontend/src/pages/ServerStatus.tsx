import { useId, useMemo, useState } from 'react';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { Navigate } from 'react-router-dom';
import { ApiError, getMe, getServerMetrics } from '@/lib/api';
import type { MetricPoint, ServerMetricsRange } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { ArrowDownUp, HardDrive, MemoryStick, ServerCrash } from 'lucide-react';

const RANGE_OPTIONS: { value: ServerMetricsRange; label: string }[] = [
  { value: '1h', label: 'Last hour' },
  { value: '6h', label: 'Last 6 hours' },
  { value: '24h', label: 'Last 24 hours' },
  { value: '7d', label: 'Last 7 days' },
  { value: '30d', label: 'Last 30 days' },
];

const MEMORY_COLOR = { stroke: 'rgb(139, 92, 246)', dot: 'bg-violet-500' };
const DISK_COLOR = { stroke: 'rgb(14, 165, 233)', dot: 'bg-sky-500' };
const RX_COLOR = { stroke: 'rgb(16, 185, 129)', dot: 'bg-emerald-500' };
const TX_COLOR = { stroke: 'rgb(245, 158, 11)', dot: 'bg-amber-500' };

export default function ServerStatus() {
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });
  const [range, setRange] = useState<ServerMetricsRange>('24h');

  const metricsQ = useQuery({
    queryKey: ['server-metrics', range],
    queryFn: () => getServerMetrics(range),
    refetchInterval: 30_000,
    placeholderData: keepPreviousData,
    retry: false,
    enabled: me?.role === 'admin',
  });

  const m = metricsQ.data;
  // Traffic is charted in bits/s, the unit link speeds are quoted in.
  const rxBits = useMemo(() => toBits(m?.network.rx), [m]);
  const txBits = useMemo(() => toBits(m?.network.tx), [m]);

  if (me && me.role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  const unavailable =
    metricsQ.error instanceof ApiError && metricsQ.error.errorCode === 'metrics_unavailable';

  return (
    <div className="space-y-6 max-w-7xl">
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Server Status</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Memory, disk, and network usage on the host.
          </p>
        </div>
        <Select value={range} onValueChange={(v) => setRange(v as ServerMetricsRange)}>
          <SelectTrigger className="w-44">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {RANGE_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {metricsQ.error ? (
        <Card>
          <CardContent className="p-8 flex flex-col items-center text-center gap-2">
            <ServerCrash className="size-8 text-muted-foreground" />
            <p className="font-medium">
              {unavailable ? 'Prometheus is not reachable' : 'Failed to load server metrics'}
            </p>
            <p className="text-sm text-muted-foreground max-w-md">
              {unavailable
                ? 'The dashboard reads host metrics from Prometheus. Check that the prometheus and prometheus-node-exporter services are running.'
                : metricsQ.error.message}
            </p>
          </CardContent>
        </Card>
      ) : (
        <>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            <UsageStat
              icon={<MemoryStick className="size-4" />}
              label="Memory"
              used={lastValue(m?.memory.used)}
              total={m?.memory.total ?? null}
              loading={metricsQ.isLoading}
            />
            <UsageStat
              icon={<HardDrive className="size-4" />}
              label="Disk (/)"
              used={lastValue(m?.disk.used)}
              total={m?.disk.total ?? null}
              loading={metricsQ.isLoading}
            />
            <TrafficStat
              rx={lastValue(rxBits)}
              tx={lastValue(txBits)}
              loading={metricsQ.isLoading}
            />
          </div>

          <ChartCard
            title="Memory"
            description="Used memory (excludes reclaimable page cache)."
            loading={metricsQ.isLoading}
            legend={[{ label: 'Used', dot: MEMORY_COLOR.dot }]}
          >
            {m && (
              <TimeSeriesChart
                start={m.start}
                end={m.end}
                step={m.step}
                series={[{ label: 'Used', color: MEMORY_COLOR, points: m.memory.used }]}
                limit={m.memory.total}
                base={1024}
                format={formatBytes}
              />
            )}
          </ChartCard>

          <ChartCard
            title="Disk"
            description="Used space on the root filesystem."
            loading={metricsQ.isLoading}
            legend={[{ label: 'Used', dot: DISK_COLOR.dot }]}
          >
            {m && (
              <TimeSeriesChart
                start={m.start}
                end={m.end}
                step={m.step}
                series={[{ label: 'Used', color: DISK_COLOR, points: m.disk.used }]}
                limit={m.disk.total}
                base={1024}
                format={formatBytes}
              />
            )}
          </ChartCard>

          <ChartCard
            title="Network traffic"
            description="Throughput on the external interface, including all container traffic."
            loading={metricsQ.isLoading}
            legend={[
              { label: 'Inbound', dot: RX_COLOR.dot },
              { label: 'Outbound', dot: TX_COLOR.dot },
            ]}
          >
            {m && (
              <TimeSeriesChart
                start={m.start}
                end={m.end}
                step={m.step}
                series={[
                  { label: 'Inbound', color: RX_COLOR, points: rxBits },
                  { label: 'Outbound', color: TX_COLOR, points: txBits },
                ]}
                base={1000}
                format={formatBitRate}
              />
            )}
          </ChartCard>
        </>
      )}
    </div>
  );
}

/* ───────────────────────── Stat cards ───────────────────────── */

function UsageStat({
  icon,
  label,
  used,
  total,
  loading,
}: {
  icon: React.ReactNode;
  label: string;
  used: number | null;
  total: number | null;
  loading: boolean;
}) {
  const pct = used !== null && total ? (used / total) * 100 : null;
  const barClass =
    pct === null ? 'bg-foreground/70' : pct >= 90 ? 'bg-rose-500' : pct >= 80 ? 'bg-amber-500' : 'bg-emerald-500';

  return (
    <Card>
      <CardContent className="p-4">
        <div className="flex items-center gap-2 mb-2 text-muted-foreground">
          {icon}
          <span className="text-xs font-medium uppercase tracking-wider">{label}</span>
        </div>
        <div className="flex items-baseline gap-2">
          <p className="text-3xl font-semibold tabular-nums leading-none">
            {loading || pct === null ? '—' : `${pct.toFixed(0)}%`}
          </p>
          {used !== null && total !== null && (
            <span className="text-xs text-muted-foreground tabular-nums">
              {formatBytes(used)} of {formatBytes(total)}
            </span>
          )}
        </div>
        <div className="h-1.5 w-full rounded-full bg-secondary overflow-hidden mt-3">
          <div className={`h-full rounded-full ${barClass}`} style={{ width: `${pct ?? 0}%` }} />
        </div>
      </CardContent>
    </Card>
  );
}

function TrafficStat({
  rx,
  tx,
  loading,
}: {
  rx: number | null;
  tx: number | null;
  loading: boolean;
}) {
  return (
    <Card>
      <CardContent className="p-4">
        <div className="flex items-center gap-2 mb-2 text-muted-foreground">
          <ArrowDownUp className="size-4" />
          <span className="text-xs font-medium uppercase tracking-wider">Traffic now</span>
        </div>
        <div className="space-y-1 tabular-nums">
          <TrafficLine dot={RX_COLOR.dot} label="In" value={loading ? null : rx} />
          <TrafficLine dot={TX_COLOR.dot} label="Out" value={loading ? null : tx} />
        </div>
      </CardContent>
    </Card>
  );
}

function TrafficLine({ dot, label, value }: { dot: string; label: string; value: number | null }) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <span className="flex items-center gap-1.5 text-sm text-muted-foreground">
        <span className={`inline-block size-2 rounded-full ${dot}`} />
        {label}
      </span>
      <span className="text-lg font-semibold">{value === null ? '—' : formatBitRate(value)}</span>
    </div>
  );
}

/* ───────────────────────── Chart ───────────────────────── */

function ChartCard({
  title,
  description,
  legend,
  loading,
  children,
}: {
  title: string;
  description: string;
  legend: { label: string; dot: string }[];
  loading: boolean;
  children: React.ReactNode;
}) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between flex-wrap gap-3">
          <div>
            <CardTitle className="text-base">{title}</CardTitle>
            <p className="text-xs text-muted-foreground mt-0.5">{description}</p>
          </div>
          <div className="flex items-center gap-4 text-xs">
            {legend.map((l) => (
              <span key={l.label} className="flex items-center gap-1.5">
                <span className={`inline-block w-4 h-[2px] rounded-full ${l.dot}`} />
                <span className="text-muted-foreground">{l.label}</span>
              </span>
            ))}
          </div>
        </div>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="h-56 flex items-center justify-center text-sm text-muted-foreground">
            Loading chart…
          </div>
        ) : (
          children
        )}
      </CardContent>
    </Card>
  );
}

type Series = {
  label: string;
  color: { stroke: string; dot: string };
  points: MetricPoint[];
};

function TimeSeriesChart({
  start,
  end,
  step,
  series,
  limit,
  base,
  format,
}: {
  start: number;
  end: number;
  step: number;
  series: Series[];
  /** Capacity drawn as a dashed line; the y-axis always includes it. */
  limit?: number | null;
  base: number;
  format: (v: number) => string;
}) {
  const width = 1000;
  const height = 220;
  const padLeft = 64;
  const padRight = 16;
  const padTop = 16;
  const padBottom = 28;
  const chartW = width - padLeft - padRight;
  const chartH = height - padTop - padBottom;
  const baseline = padTop + chartH;
  const gradientId = useId();

  const [hoverT, setHoverT] = useState<number | null>(null);

  const allValues = series.flatMap((s) => s.points.map((p) => p[1]));
  if (allValues.length === 0) {
    return (
      <div className="h-56 flex items-center justify-center text-sm text-muted-foreground">
        No samples in this range yet.
      </div>
    );
  }

  const { top, ticks } = niceScale(Math.max(...allValues, limit ?? 0), base);
  const xFor = (t: number) => padLeft + ((t - start) / (end - start)) * chartW;
  const yFor = (v: number) => padTop + chartH - (v / top) * chartH;

  // Prometheus omits steps with no samples (e.g. host was down); break the line there.
  const segmentsOf = (points: MetricPoint[]) => {
    const segments: MetricPoint[][] = [];
    for (const p of points) {
      const cur = segments[segments.length - 1];
      if (cur && p[0] - cur[cur.length - 1][0] <= step * 1.5) cur.push(p);
      else segments.push([p]);
    }
    return segments;
  };
  const linePath = (seg: MetricPoint[]) =>
    'M ' + seg.map((p) => `${xFor(p[0])} ${yFor(p[1])}`).join(' L ');
  const areaPath = (seg: MetricPoint[]) =>
    `M ${xFor(seg[0][0])} ${baseline} L ${seg
      .map((p) => `${xFor(p[0])} ${yFor(p[1])}`)
      .join(' L ')} L ${xFor(seg[seg.length - 1][0])} ${baseline} Z`;
  const onlySeries = series.length === 1;

  const timeTicks = buildTimeTicks(start, end);
  const shortRange = end - start <= 86_400;

  const hoverRows =
    hoverT !== null
      ? series.map((s) => ({ s, p: s.points.find((p) => p[0] === hoverT) }))
      : [];
  const hoverX = hoverT !== null ? xFor(hoverT) : 0;
  const tooltipLeftPct = (hoverX / width) * 100;

  const handleMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const x = ((e.clientX - rect.left) / rect.width) * width;
    const t = start + ((x - padLeft) / chartW) * (end - start);
    const ref = series.reduce((a, b) => (b.points.length > a.points.length ? b : a)).points;
    let best: number | null = null;
    for (const p of ref) {
      if (best === null || Math.abs(p[0] - t) < Math.abs(best - t)) best = p[0];
    }
    setHoverT(best);
  };

  return (
    <div className="w-full relative">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="w-full h-56 block"
        preserveAspectRatio="none"
        onMouseMove={handleMove}
        onMouseLeave={() => setHoverT(null)}
      >
        <defs>
          {series.map((s, i) => (
            <linearGradient key={i} id={`${gradientId}-${i}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={s.color.stroke} stopOpacity="0.2" />
              <stop offset="100%" stopColor={s.color.stroke} stopOpacity="0" />
            </linearGradient>
          ))}
        </defs>

        {ticks.map((v, idx) => (
          <g key={idx}>
            <line
              x1={padLeft}
              y1={yFor(v)}
              x2={width - padRight}
              y2={yFor(v)}
              stroke="currentColor"
              strokeOpacity={idx === 0 ? 0.28 : 0.1}
              strokeDasharray={idx === 0 ? undefined : '3 4'}
            />
            <text
              x={padLeft - 8}
              y={yFor(v) + 3}
              textAnchor="end"
              className="fill-muted-foreground"
              style={{ fontSize: 10 }}
            >
              {format(v)}
            </text>
          </g>
        ))}

        {timeTicks.map((t) => (
          <text
            key={t}
            x={xFor(t)}
            y={baseline + 16}
            textAnchor="middle"
            className="fill-muted-foreground"
            style={{ fontSize: 10 }}
          >
            {formatTimeTick(t, shortRange)}
          </text>
        ))}

        {limit != null && (
          <g>
            <line
              x1={padLeft}
              y1={yFor(limit)}
              x2={width - padRight}
              y2={yFor(limit)}
              className="stroke-rose-500"
              strokeOpacity={0.6}
              strokeDasharray="6 4"
            />
            <text
              x={width - padRight}
              y={yFor(limit) - 4}
              textAnchor="end"
              className="fill-rose-500"
              style={{ fontSize: 10 }}
            >
              Total {format(limit)}
            </text>
          </g>
        )}

        {series.map((s, i) =>
          segmentsOf(s.points).map((seg, j) => (
            <g key={`${i}-${j}`}>
              {onlySeries && <path d={areaPath(seg)} fill={`url(#${gradientId}-${i})`} />}
              <path
                d={linePath(seg)}
                fill="none"
                stroke={s.color.stroke}
                strokeWidth={1.75}
                strokeLinejoin="round"
                strokeLinecap="round"
              />
            </g>
          )),
        )}

        {hoverT !== null && (
          <g pointerEvents="none">
            <line
              x1={hoverX}
              y1={padTop}
              x2={hoverX}
              y2={baseline}
              stroke="currentColor"
              strokeOpacity={0.22}
              strokeDasharray="3 3"
            />
            {hoverRows.map(
              ({ s, p }) =>
                p && (
                  <circle
                    key={s.label}
                    cx={hoverX}
                    cy={yFor(p[1])}
                    r={4}
                    fill={s.color.stroke}
                    className="stroke-background"
                    strokeWidth={2}
                  />
                ),
            )}
          </g>
        )}
      </svg>

      {hoverT !== null && (
        <div
          className="pointer-events-none absolute z-10 top-4"
          style={{
            left: `${tooltipLeftPct}%`,
            transform: tooltipLeftPct > 70 ? 'translateX(calc(-100% - 12px))' : 'translateX(12px)',
          }}
        >
          <div className="rounded-md border border-border bg-popover shadow-lg px-3 py-2 text-xs min-w-[160px]">
            <div className="font-medium text-foreground mb-1.5">
              {new Date(hoverT * 1000).toLocaleString(undefined, {
                month: 'short',
                day: 'numeric',
                hour: '2-digit',
                minute: '2-digit',
              })}
            </div>
            {hoverRows.map(({ s, p }) => (
              <div key={s.label} className="flex items-center justify-between gap-4 tabular-nums mt-1">
                <span className="flex items-center gap-1.5 text-muted-foreground">
                  <span className={`inline-block size-2 rounded-full ${s.color.dot}`} />
                  {s.label}
                </span>
                <span className="font-medium text-foreground">{p ? format(p[1]) : '—'}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/* ───────────────────────── Scales & formatting ───────────────────────── */

/** Picks ~4 round ticks in the value's own unit (e.g. 0/16/32/48/64 GiB, not 0/20G/40G…). */
function niceScale(max: number, base: number): { top: number; ticks: number[] } {
  if (max <= 0) return { top: 1, ticks: [0, 1] };
  const unit = Math.pow(base, Math.max(0, Math.floor(Math.log(max) / Math.log(base))));
  const scaled = max / unit;
  const rawStep = scaled / 4;
  const mag = Math.pow(10, Math.floor(Math.log10(rawStep)));
  const norm = rawStep / mag;
  const stepScaled = (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 2.5 ? 2.5 : norm <= 5 ? 5 : 10) * mag;
  const steps = Math.ceil(scaled / stepScaled - 1e-9);
  const ticks = Array.from({ length: steps + 1 }, (_, i) => i * stepScaled * unit);
  return { top: ticks[ticks.length - 1], ticks };
}

/** Aligns ticks to local wall-clock boundaries (on the hour, midnight, …). */
function buildTimeTicks(start: number, end: number): number[] {
  const intervals = [300, 900, 1800, 3600, 3 * 3600, 6 * 3600, 12 * 3600, 86_400, 2 * 86_400, 7 * 86_400];
  const interval = intervals.find((i) => (end - start) / i <= 7) ?? intervals[intervals.length - 1];
  const tzOffset = -new Date(start * 1000).getTimezoneOffset() * 60;
  const ticks: number[] = [];
  for (let t = Math.ceil((start + tzOffset) / interval) * interval - tzOffset; t <= end; t += interval) {
    ticks.push(t);
  }
  return ticks;
}

function formatTimeTick(t: number, shortRange: boolean): string {
  const d = new Date(t * 1000);
  return shortRange
    ? d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
    : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function formatBytes(v: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

function formatBitRate(v: number): string {
  const units = ['bps', 'Kbps', 'Mbps', 'Gbps'];
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

function toBits(points: MetricPoint[] | undefined): MetricPoint[] {
  return (points ?? []).map(([t, v]) => [t, v * 8]);
}

function lastValue(points: MetricPoint[] | undefined): number | null {
  return points?.length ? points[points.length - 1][1] : null;
}
