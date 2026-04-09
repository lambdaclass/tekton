import type { AutoresearchExperiment } from '@/lib/api';

interface MetricChartProps {
  experiments: AutoresearchExperiment[];
  baseline: number | null;
  best: number | null;
  direction: string;
}

export default function MetricChart({ experiments, baseline, best, direction }: MetricChartProps) {
  const dataPoints = experiments.filter((e) => e.metric_value != null);
  if (dataPoints.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        No experiment data yet
      </div>
    );
  }

  const values = dataPoints.map((e) => e.metric_value!);
  const allValues = [...values];
  if (baseline != null) allValues.push(baseline);
  if (best != null) allValues.push(best);

  const minVal = Math.min(...allValues);
  const maxVal = Math.max(...allValues);
  const range = maxVal - minVal || 1;
  const padding = range * 0.1;
  const yMin = minVal - padding;
  const yMax = maxVal + padding;

  const width = 600;
  const height = 200;
  const marginLeft = 60;
  const marginRight = 20;
  const marginTop = 10;
  const marginBottom = 30;
  const plotW = width - marginLeft - marginRight;
  const plotH = height - marginTop - marginBottom;

  const maxExpNum = Math.max(...dataPoints.map((e) => e.experiment_number));
  const minExpNum = Math.min(...dataPoints.map((e) => e.experiment_number));
  const xRange = maxExpNum - minExpNum || 1;

  const toX = (expNum: number) => marginLeft + ((expNum - minExpNum) / xRange) * plotW;
  const toY = (val: number) => marginTop + (1 - (val - yMin) / (yMax - yMin)) * plotH;

  return (
    <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-full" preserveAspectRatio="xMidYMid meet">
      {/* Grid lines */}
      {[0, 0.25, 0.5, 0.75, 1].map((frac) => {
        const y = marginTop + (1 - frac) * plotH;
        const val = yMin + frac * (yMax - yMin);
        return (
          <g key={frac}>
            <line x1={marginLeft} y1={y} x2={width - marginRight} y2={y} stroke="currentColor" strokeOpacity={0.1} />
            <text x={marginLeft - 8} y={y + 3} textAnchor="end" className="fill-muted-foreground" fontSize={9}>
              {val.toFixed(val < 10 ? 2 : 0)}
            </text>
          </g>
        );
      })}

      {/* Baseline line */}
      {baseline != null && (
        <>
          <line
            x1={marginLeft} y1={toY(baseline)}
            x2={width - marginRight} y2={toY(baseline)}
            stroke="currentColor" strokeOpacity={0.3} strokeDasharray="4 4"
          />
          <text x={width - marginRight + 4} y={toY(baseline) + 3} fontSize={8} className="fill-muted-foreground">
            baseline
          </text>
        </>
      )}

      {/* Best line */}
      {best != null && best !== baseline && (
        <>
          <line
            x1={marginLeft} y1={toY(best)}
            x2={width - marginRight} y2={toY(best)}
            stroke="rgb(16 185 129)" strokeOpacity={0.5} strokeDasharray="4 4"
          />
          <text x={width - marginRight + 4} y={toY(best) + 3} fontSize={8} fill="rgb(16 185 129)">
            best
          </text>
        </>
      )}

      {/* Data points */}
      {dataPoints.map((exp) => {
        const x = toX(exp.experiment_number);
        const y = toY(exp.metric_value!);
        const accepted = exp.accepted === true;
        return (
          <circle
            key={exp.id}
            cx={x}
            cy={y}
            r={4}
            fill={accepted ? 'rgb(16 185 129)' : 'rgb(239 68 68)'}
            fillOpacity={0.8}
          >
            <title>
              Exp #{exp.experiment_number}: {exp.metric_value} ({accepted ? 'accepted' : 'rejected'})
            </title>
          </circle>
        );
      })}

      {/* X axis labels */}
      {dataPoints.filter((_, i) => i % Math.max(1, Math.floor(dataPoints.length / 8)) === 0 || i === dataPoints.length - 1).map((exp) => (
        <text
          key={exp.id}
          x={toX(exp.experiment_number)}
          y={height - 5}
          textAnchor="middle"
          fontSize={9}
          className="fill-muted-foreground"
        >
          #{exp.experiment_number}
        </text>
      ))}
    </svg>
  );
}
