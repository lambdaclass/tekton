import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Navigate } from 'react-router-dom';
import { getMe, getKpis } from '@/lib/api';
import type { KpiTrendPoint } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Users, GitPullRequest, Gauge, Clock, BarChart3 } from 'lucide-react';

const PERIOD_OPTIONS = [
  { value: '7', label: 'Last 7 days' },
  { value: '30', label: 'Last 30 days' },
  { value: '90', label: 'Last 90 days' },
] as const;

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const mins = Math.floor(seconds / 60);
  const secs = Math.round(seconds % 60);
  if (mins < 60) return `${mins}m ${secs}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}

export default function KPIDashboard() {
  const { data: me } = useQuery({ queryKey: ['me'], queryFn: getMe });
  const [days, setDays] = useState<number>(30);

  if (me && me.role !== 'admin') {
    return <Navigate to="/" replace />;
  }

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">Product Health</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Adoption and impact KPIs. Merge-status tracking is a follow-up; "PR"
            currently means a reviewable PR has been opened from the task.
          </p>
        </div>
        <Select value={String(days)} onValueChange={(v) => setDays(Number(v))}>
          <SelectTrigger className="w-40">
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

      <KpiCards days={days} />
      <TrendChart days={days} />
    </div>
  );
}

function KpiCards({ days }: { days: number }) {
  const { data, isLoading } = useQuery({
    queryKey: ['kpis', days],
    queryFn: () => getKpis(days),
  });

  const conversionPct =
    data != null ? `${(data.session_to_pr_conversion_rate * 100).toFixed(1)}%` : '-';
  const median =
    data?.median_time_to_first_pr_seconds != null
      ? formatDuration(data.median_time_to_first_pr_seconds)
      : data
      ? 'n/a'
      : '-';

  const cards = [
    {
      title: 'Weekly Active Prompting Users',
      value: data ? String(data.weekly_active_prompting_users) : '-',
      hint: 'Distinct users who created a task in the last 7 days',
      icon: Users,
    },
    {
      title: 'Session → PR Conversion',
      value: conversionPct,
      hint: data
        ? `${data.tasks_with_pr} of ${data.sessions} tasks opened a PR`
        : `Share of tasks that opened a PR in the last ${days} days`,
      icon: GitPullRequest,
    },
    {
      title: 'Median Time to First PR',
      value: median,
      hint: 'Task creation → task.pr_created audit event',
      icon: Clock,
    },
    {
      title: 'Sessions',
      value: data ? String(data.sessions) : '-',
      hint: `Tasks created in the last ${days} days`,
      icon: Gauge,
    },
  ];

  return (
    <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {cards.map((c) => (
        <Card key={c.title}>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">
              {c.title}
            </CardTitle>
            <c.icon className="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            {isLoading ? (
              <p className="text-muted-foreground text-sm">Loading...</p>
            ) : (
              <>
                <p className="text-2xl font-bold tabular-nums">{c.value}</p>
                <p className="text-xs text-muted-foreground mt-1">{c.hint}</p>
              </>
            )}
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

function TrendChart({ days }: { days: number }) {
  const { data, isLoading } = useQuery({
    queryKey: ['kpis', days],
    queryFn: () => getKpis(days),
  });

  const trends: KpiTrendPoint[] = data?.trends ?? [];

  const rawMax = trends.length ? Math.max(...trends.map((t) => t.sessions), 1) : 1;
  function niceMax(v: number): number {
    if (v <= 0) return 1;
    const mag = Math.pow(10, Math.floor(Math.log10(v)));
    const norm = v / mag;
    if (norm <= 1) return mag;
    if (norm <= 2) return 2 * mag;
    if (norm <= 5) return 5 * mag;
    return 10 * mag;
  }
  const maxY = niceMax(rawMax);

  const width = 800;
  const height = 200;
  const padLeft = 48;
  const padRight = 20;
  const padTop = 10;
  const padBottom = 30;
  const chartW = width - padLeft - padRight;
  const chartH = height - padTop - padBottom;

  const xFor = (i: number) =>
    padLeft + (trends.length === 1 ? chartW / 2 : (i / (trends.length - 1)) * chartW);
  const yFor = (v: number) => padTop + chartH - (v / maxY) * chartH;

  const sessionPts = trends.map((t, i) => ({ x: xFor(i), y: yFor(t.sessions), t }));
  const prPts = trends.map((t, i) => ({ x: xFor(i), y: yFor(t.tasks_with_pr), t }));

  const toPath = (pts: { x: number; y: number }[]) =>
    pts.map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x},${p.y}`).join(' ');

  const yTicks = [0, maxY / 2, maxY];

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <BarChart3 className="size-5" />
          Sessions & PRs per Day
        </CardTitle>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-muted-foreground text-sm">Loading chart...</p>
        ) : !trends.length ? (
          <p className="text-muted-foreground text-sm">No data for this period.</p>
        ) : (
          <>
            <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-48">
              {yTicks.map((v) => {
                const y = yFor(v);
                return (
                  <g key={v}>
                    <line
                      x1={padLeft}
                      y1={y}
                      x2={width - padRight}
                      y2={y}
                      stroke="currentColor"
                      strokeOpacity={0.15}
                    />
                    <text
                      x={padLeft - 8}
                      y={y + 4}
                      textAnchor="end"
                      className="fill-muted-foreground"
                      style={{ fontSize: 11 }}
                    >
                      {Math.round(v)}
                    </text>
                  </g>
                );
              })}

              <path
                d={toPath(sessionPts)}
                fill="none"
                className="stroke-primary"
                strokeWidth={2}
                strokeLinejoin="round"
              />
              <path
                d={toPath(prPts)}
                fill="none"
                className="stroke-emerald-500"
                strokeWidth={2}
                strokeLinejoin="round"
                strokeDasharray="4 3"
              />

              {sessionPts.map((p) => {
                const date = new Date(p.t.day);
                const label = `${date.getMonth() + 1}/${date.getDate()}`;
                return (
                  <g key={`s-${p.t.day}`}>
                    <circle cx={p.x} cy={p.y} r={3} className="fill-primary" />
                    <title>{`${label}: ${p.t.sessions} sessions, ${p.t.tasks_with_pr} PRs`}</title>
                    {trends.length <= 31 && (
                      <text
                        x={p.x}
                        y={padTop + chartH + 16}
                        textAnchor="middle"
                        className="fill-muted-foreground"
                        style={{ fontSize: 9 }}
                      >
                        {label}
                      </text>
                    )}
                  </g>
                );
              })}
              {prPts.map((p) => (
                <circle
                  key={`p-${p.t.day}`}
                  cx={p.x}
                  cy={p.y}
                  r={3}
                  className="fill-emerald-500"
                />
              ))}
            </svg>
            <div className="flex gap-4 mt-3 text-xs text-muted-foreground">
              <span className="inline-flex items-center gap-2">
                <span className="inline-block h-0.5 w-4 bg-primary" />
                Sessions
              </span>
              <span className="inline-flex items-center gap-2">
                <span className="inline-block h-0.5 w-4 bg-emerald-500" />
                Tasks with PR
              </span>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
