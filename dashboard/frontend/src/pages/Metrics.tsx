import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  getMetricsSummary,
  getTasksOverTime,
  getTopUsers,
  getTopRepos,
} from '@/lib/api';
import type { MetricsSummary, TasksOverTimeRow } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Users,
  Activity,
  DollarSign,
  Zap,
  TrendingUp,
  TrendingDown,
  Minus,
  FolderGit2,
} from 'lucide-react';

const PERIOD_OPTIONS = [
  { value: '7', label: 'Last 7 days' },
  { value: '30', label: 'Last 30 days' },
  { value: '90', label: 'Last 90 days' },
] as const;

export default function Metrics() {
  const [days, setDays] = useState<number>(30);

  const summaryQ = useQuery({
    queryKey: ['metrics-summary', days],
    queryFn: () => getMetricsSummary(days),
  });
  const overTimeQ = useQuery({
    queryKey: ['metrics-tasks-over-time', days],
    queryFn: () => getTasksOverTime(days),
  });
  const topUsersQ = useQuery({
    queryKey: ['metrics-top-users', days],
    queryFn: () => getTopUsers(days),
  });
  const topReposQ = useQuery({
    queryKey: ['metrics-top-repos', days],
    queryFn: () => getTopRepos(days),
  });

  return (
    <div className="space-y-6 max-w-7xl">
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Metrics</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Usage, cost, and activity trends across your workspace.
          </p>
        </div>
        <Select value={String(days)} onValueChange={(v) => setDays(Number(v))}>
          <SelectTrigger className="w-44">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PERIOD_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <StatGrid summary={summaryQ.data} loading={summaryQ.isLoading} />

      <ActivityCard
        data={overTimeQ.data}
        loading={overTimeQ.isLoading}
        days={days}
      />

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <TopList
          title="Top Users"
          icon={<Users className="size-4" />}
          rows={(topUsersQ.data ?? []).map((u) => ({
            label: u.login,
            count: u.task_count,
            cost: u.cost_usd,
          }))}
          loading={topUsersQ.isLoading}
        />
        <TopList
          title="Top Repos"
          icon={<FolderGit2 className="size-4" />}
          rows={(topReposQ.data ?? []).map((r) => ({
            label: r.repo,
            count: r.task_count,
            cost: r.cost_usd,
          }))}
          loading={topReposQ.isLoading}
        />
      </div>
    </div>
  );
}

/* ───────────────────────── Stat cards ───────────────────────── */

function StatGrid({
  summary,
  loading,
}: {
  summary: MetricsSummary | undefined;
  loading: boolean;
}) {
  const s = summary;
  const tokens = s ? s.total_input_tokens + s.total_output_tokens : 0;

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
      <StatCard
        accent="indigo"
        icon={<Users className="size-4" />}
        label="Active Users"
        value={loading ? '—' : String(s?.active_users ?? 0)}
        sub={s ? `of ${s.total_users} total registered` : undefined}
        delta={s ? deltaPct(s.active_users, s.prev_active_users) : null}
      />
      <StatCard
        accent="emerald"
        icon={<Activity className="size-4" />}
        label="Tasks"
        value={loading ? '—' : String(s?.total_tasks ?? 0)}
        sub={
          s
            ? `${s.completed_tasks} completed · ${s.failed_tasks} failed`
            : undefined
        }
        delta={s ? deltaPct(s.total_tasks, s.prev_total_tasks) : null}
      />
      <StatCard
        accent="amber"
        icon={<DollarSign className="size-4" />}
        label="Total Cost"
        value={loading ? '—' : fmtCost(s?.total_cost_usd ?? 0)}
        sub={s ? `${fmtCost(s.avg_cost_per_task)} avg / task` : undefined}
        delta={s ? deltaPct(s.total_cost_usd, s.prev_total_cost_usd) : null}
        deltaInverse /* cost going UP is not "good" */
      />
      <StatCard
        accent="rose"
        icon={<Zap className="size-4" />}
        label="Tokens"
        value={loading ? '—' : formatTokens(tokens)}
        sub={
          s
            ? `${formatTokens(s.total_input_tokens)} in · ${formatTokens(s.total_output_tokens)} out`
            : undefined
        }
      />
    </div>
  );
}

type AccentKey = 'indigo' | 'emerald' | 'amber' | 'rose';
const ACCENT_CLASSES: Record<AccentKey, { bg: string; fg: string }> = {
  indigo: { bg: 'bg-indigo-500/10', fg: 'text-indigo-500 dark:text-indigo-400' },
  emerald: { bg: 'bg-emerald-500/10', fg: 'text-emerald-600 dark:text-emerald-400' },
  amber: { bg: 'bg-amber-500/15', fg: 'text-amber-600 dark:text-amber-400' },
  rose: { bg: 'bg-rose-500/10', fg: 'text-rose-500 dark:text-rose-400' },
};

function StatCard({
  icon,
  label,
  value,
  sub,
  delta,
  deltaInverse,
  accent,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  sub?: string;
  delta?: number | null;
  deltaInverse?: boolean;
  accent: AccentKey;
}) {
  const a = ACCENT_CLASSES[accent];
  return (
    <Card className="overflow-hidden relative">
      <div className={`absolute inset-y-0 left-0 w-1 ${a.bg}`} />
      <CardContent className="p-4">
        <div className="flex items-center gap-2 mb-2">
          <div className={`size-7 rounded-md flex items-center justify-center ${a.bg} ${a.fg}`}>
            {icon}
          </div>
          <span className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
            {label}
          </span>
        </div>
        <div className="flex items-baseline gap-2">
          <p className="text-3xl font-semibold tabular-nums leading-none">{value}</p>
          {delta !== null && delta !== undefined && <DeltaBadge pct={delta} inverse={deltaInverse} />}
        </div>
        {sub && (
          <p className="text-xs text-muted-foreground mt-2 tabular-nums truncate">{sub}</p>
        )}
      </CardContent>
    </Card>
  );
}

function DeltaBadge({ pct, inverse }: { pct: number; inverse?: boolean }) {
  if (!Number.isFinite(pct)) {
    return (
      <span className="inline-flex items-center gap-0.5 text-[11px] font-medium text-muted-foreground">
        <TrendingUp className="size-3" /> new
      </span>
    );
  }
  const isZero = Math.abs(pct) < 0.5;
  const up = pct > 0;
  const isGood = isZero ? null : (inverse ? !up : up);
  const cls = isGood === null
    ? 'text-muted-foreground'
    : isGood
      ? 'text-emerald-600 dark:text-emerald-400'
      : 'text-rose-500 dark:text-rose-400';
  const Icon = isZero ? Minus : up ? TrendingUp : TrendingDown;
  const sign = up && !isZero ? '+' : '';
  return (
    <span className={`inline-flex items-center gap-0.5 text-[11px] font-medium tabular-nums ${cls}`}>
      <Icon className="size-3" />
      {sign}
      {Math.round(pct)}%
    </span>
  );
}

function deltaPct(curr: number, prev: number): number {
  if (prev === 0) return curr === 0 ? 0 : Infinity;
  return ((curr - prev) / prev) * 100;
}

/* ───────────────────────── Activity chart ───────────────────────── */

function ActivityCard({
  data,
  loading,
  days,
}: {
  data: TasksOverTimeRow[] | undefined;
  loading: boolean;
  days: number;
}) {
  const hasData = (data ?? []).some((d) => d.total > 0 || d.cost_usd > 0);

  const trendLabel = useMemo(() => {
    if (!data?.length) return null;
    const half = Math.floor(data.length / 2);
    if (half < 2) return null;
    const firstTotal = data.slice(0, half).reduce((s, d) => s + d.total, 0);
    const secondTotal = data.slice(half).reduce((s, d) => s + d.total, 0);
    if (firstTotal === 0 && secondTotal === 0) return null;
    const pct = firstTotal === 0 ? 100 : ((secondTotal - firstTotal) / firstTotal) * 100;
    return {
      pct,
      label:
        pct > 5
          ? 'Activity trending up'
          : pct < -5
            ? 'Activity trending down'
            : 'Activity roughly steady',
    };
  }, [data]);

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between flex-wrap gap-3">
          <div>
            <CardTitle className="text-base">Activity over time</CardTitle>
            <p className="text-xs text-muted-foreground mt-0.5">
              Daily tasks and cost.
            </p>
          </div>
          <div className="flex items-center gap-4 flex-wrap">
            {trendLabel && (
              <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                {trendLabel.pct > 5 ? (
                  <TrendingUp className="size-3.5 text-emerald-500" />
                ) : trendLabel.pct < -5 ? (
                  <TrendingDown className="size-3.5 text-rose-500" />
                ) : (
                  <Minus className="size-3.5" />
                )}
                {trendLabel.label}
                {Number.isFinite(trendLabel.pct) && (
                  <span className="tabular-nums">
                    ({trendLabel.pct > 0 ? '+' : ''}
                    {Math.round(trendLabel.pct)}%)
                  </span>
                )}
              </span>
            )}
            <LegendRow />
          </div>
        </div>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="h-64 flex items-center justify-center text-sm text-muted-foreground">
            Loading chart…
          </div>
        ) : !data?.length || !hasData ? (
          <div className="h-64 flex items-center justify-center text-sm text-muted-foreground">
            No activity in the last {days} days.
          </div>
        ) : (
          <ActivityChart data={data} />
        )}
      </CardContent>
    </Card>
  );
}

function LegendRow() {
  return (
    <div className="flex items-center gap-4 text-xs">
      <LegendLine className="bg-emerald-500" label="Tasks" />
      <LegendLine className="bg-amber-500" label="Cost" />
    </div>
  );
}

function LegendLine({ className, label }: { className: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <span className={`inline-block w-4 h-[2px] rounded-full ${className}`} />
      <span className="text-muted-foreground">{label}</span>
    </span>
  );
}

function ActivityChart({ data }: { data: TasksOverTimeRow[] }) {
  const width = 1000;
  const height = 260;
  const padLeft = 44;
  const padRight = 48;
  const padTop = 12;
  const padBottom = 32;
  const chartW = width - padLeft - padRight;
  const chartH = height - padTop - padBottom;

  const rawMax = Math.max(...data.map((d) => d.total), 1);
  const maxVal = niceMax(rawMax);
  const rawCostMax = Math.max(...data.map((d) => d.cost_usd), 0);
  const costMax = rawCostMax > 0 ? niceCostMax(rawCostMax) : 1;

  const yTicks = buildYTicks(maxVal);

  const slotW = data.length > 1 ? chartW / (data.length - 1) : chartW;
  const xFor = (i: number) => padLeft + i * slotW;
  const baseline = padTop + chartH;

  const taskPoints = data.map((d, i) => ({
    x: xFor(i),
    y: padTop + chartH - (d.total / maxVal) * chartH,
    total: d.total,
  }));
  const costPoints = data.map((d, i) => ({
    x: xFor(i),
    y: padTop + chartH - (d.cost_usd / costMax) * chartH,
    cost: d.cost_usd,
  }));

  // Straight segments — bezier smoothing creates spurious peaks on sparse data.
  const polyline = (pts: { x: number; y: number }[]) =>
    pts.length ? 'M ' + pts.map((p) => `${p.x} ${p.y}`).join(' L ') : '';
  const area = (pts: { x: number; y: number }[]) =>
    pts.length
      ? `M ${pts[0].x} ${baseline} L ${pts
          .map((p) => `${p.x} ${p.y}`)
          .join(' L ')} L ${pts[pts.length - 1].x} ${baseline} Z`
      : '';

  const taskLinePath = polyline(taskPoints);
  const taskAreaPath = area(taskPoints);
  const costLinePath = polyline(costPoints);

  const xLabelStep = Math.max(1, Math.ceil(data.length / 8));

  return (
    <div className="w-full overflow-hidden">
      <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-64" preserveAspectRatio="none">
        <defs>
          <linearGradient id="taskArea" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="rgb(16, 185, 129)" stopOpacity="0.18" />
            <stop offset="100%" stopColor="rgb(16, 185, 129)" stopOpacity="0" />
          </linearGradient>
        </defs>

        {/* Horizontal grid + left axis labels (tasks) */}
        {yTicks.map((v, idx) => {
          const y = padTop + chartH - (v / maxVal) * chartH;
          return (
            <g key={idx}>
              <line
                x1={padLeft}
                y1={y}
                x2={width - padRight}
                y2={y}
                stroke="currentColor"
                strokeOpacity={idx === 0 ? 0.28 : 0.1}
                strokeDasharray={idx === 0 ? undefined : '3 4'}
              />
              <text
                x={padLeft - 8}
                y={y + 3}
                textAnchor="end"
                className="fill-muted-foreground"
                style={{ fontSize: 10 }}
              >
                {formatAxisNum(v)}
              </text>
            </g>
          );
        })}

        {/* Right axis labels (cost) — only if there's cost data */}
        {rawCostMax > 0 &&
          [0, costMax / 2, costMax].map((v, idx) => {
            const y = padTop + chartH - (v / costMax) * chartH;
            return (
              <text
                key={`c-${idx}`}
                x={width - padRight + 6}
                y={y + 3}
                textAnchor="start"
                className="fill-amber-600 dark:fill-amber-400"
                style={{ fontSize: 10 }}
              >
                {fmtCostAxis(v)}
              </text>
            );
          })}

        {/* X-axis date labels + hover hit targets with tooltips */}
        {data.map((d, i) => {
          const date = new Date(d.day + 'T00:00:00');
          const label = `${date.getMonth() + 1}/${date.getDate()}`;
          const cx = xFor(i);
          const hitW = data.length > 1 ? slotW : chartW;
          return (
            <g key={d.day}>
              <rect
                x={cx - hitW / 2}
                y={padTop}
                width={hitW}
                height={chartH}
                fill="transparent"
              >
                <title>{`${label}: ${d.total} task${d.total === 1 ? '' : 's'} · ${fmtCost(d.cost_usd)}`}</title>
              </rect>
              {i % xLabelStep === 0 && (
                <text
                  x={cx}
                  y={baseline + 16}
                  textAnchor="middle"
                  className="fill-muted-foreground"
                  style={{ fontSize: 10 }}
                >
                  {label}
                </text>
              )}
            </g>
          );
        })}

        {/* Tasks area + line */}
        <path d={taskAreaPath} fill="url(#taskArea)" />
        <path
          d={taskLinePath}
          fill="none"
          stroke="rgb(16, 185, 129)"
          strokeWidth={2}
          strokeLinejoin="round"
          strokeLinecap="round"
        />
        {taskPoints
          .filter((p) => p.total > 0)
          .map((p, i) => (
            <circle
              key={`t-${i}`}
              cx={p.x}
              cy={p.y}
              r={3}
              className="fill-emerald-500 stroke-background"
              strokeWidth={1.5}
            />
          ))}

        {/* Cost line */}
        {rawCostMax > 0 && (
          <>
            <path
              d={costLinePath}
              fill="none"
              stroke="rgb(245, 158, 11)"
              strokeWidth={1.75}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
            {costPoints
              .filter((p) => p.cost > 0)
              .map((p, i) => (
                <circle
                  key={`c-${i}`}
                  cx={p.x}
                  cy={p.y}
                  r={2.5}
                  className="fill-amber-500 stroke-background"
                  strokeWidth={1.5}
                />
              ))}
          </>
        )}
      </svg>
    </div>
  );
}

function buildYTicks(maxVal: number): number[] {
  // For small integer ranges, step by 1; otherwise aim for ~5 nicely spaced ticks.
  if (maxVal <= 5 && Number.isInteger(maxVal)) {
    return Array.from({ length: maxVal + 1 }, (_, i) => i);
  }
  const targetSteps = 4;
  const rawStep = maxVal / targetSteps;
  const mag = Math.pow(10, Math.floor(Math.log10(rawStep)));
  const norm = rawStep / mag;
  const step = (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10) * mag;
  const ticks: number[] = [];
  for (let v = 0; v <= maxVal + step / 2; v += step) ticks.push(v);
  return ticks;
}

function niceMax(v: number): number {
  if (v <= 0) return 1;
  const mag = Math.pow(10, Math.floor(Math.log10(v)));
  const norm = v / mag;
  if (norm <= 1) return mag;
  if (norm <= 2) return 2 * mag;
  if (norm <= 5) return 5 * mag;
  return 10 * mag;
}

function niceCostMax(v: number): number {
  if (v <= 0) return 1;
  const mag = Math.pow(10, Math.floor(Math.log10(v)));
  const norm = v / mag;
  if (norm <= 1) return mag;
  if (norm <= 2) return 2 * mag;
  if (norm <= 5) return 5 * mag;
  return 10 * mag;
}

function formatAxisNum(v: number): string {
  if (v >= 1000) return `${(v / 1000).toFixed(1)}k`;
  return String(Math.round(v));
}

function fmtCostAxis(v: number): string {
  if (v >= 100) return `$${Math.round(v)}`;
  if (v >= 10) return `$${v.toFixed(1)}`;
  return `$${v.toFixed(2)}`;
}

function fmtCost(v: number): string {
  if (v >= 100) return `$${v.toFixed(2)}`;
  if (v >= 1) return `$${v.toFixed(2)}`;
  return `$${v.toFixed(4)}`;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/* ───────────────────────── Top lists ───────────────────────── */

type TopRow = { label: string; count: number; cost: number };

function TopList({
  title,
  icon,
  rows,
  loading,
}: {
  title: string;
  icon: React.ReactNode;
  rows: TopRow[];
  loading: boolean;
}) {
  const max = Math.max(...rows.map((r) => r.count), 1);

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-sm font-medium text-muted-foreground flex items-center gap-2">
          {icon}
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent>
        {loading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : rows.length === 0 ? (
          <p className="text-sm text-muted-foreground">No data for this period.</p>
        ) : (
          <div className="space-y-2.5">
            {rows.map((r) => {
              const pct = (r.count / max) * 100;
              return (
                <div key={r.label} className="group">
                  <div className="flex items-baseline justify-between gap-2 mb-1">
                    <span className="font-mono text-sm truncate">{r.label}</span>
                    <span className="flex items-baseline gap-2 text-xs tabular-nums text-muted-foreground shrink-0">
                      <span className="font-medium text-foreground">{r.count}</span>
                      <span>·</span>
                      <span>{fmtCost(r.cost)}</span>
                    </span>
                  </div>
                  <div className="h-1.5 w-full rounded-full bg-secondary overflow-hidden">
                    <div
                      className="h-full rounded-full bg-foreground/70 group-hover:bg-foreground transition-colors"
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

