import React from 'react';
import { ChevronDown, Gauge } from 'lucide-react';
import type { Song, SongPerformance } from '../types';
import { MODE_LABEL, renderModeOf } from '../services/qualityModes';

/** Seconds as m:ss, or s.s under a minute. */
export function formatMs(ms: number | null | undefined): string | undefined {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) return undefined;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 2 : 1)} s`;
  const whole = Math.round(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`;
}

export interface StageRow {
  label: string;
  ms: number;
  detail?: string;
}

/** The stages of a song's making in order, each with its share of the wait. */
export function stageRows(performance: SongPerformance): StageRow[] {
  const engine = performance.engine;
  const rows: StageRow[] = [];
  if (performance.queued_ms) rows.push({ label: 'Waiting in the queue', ms: performance.queued_ms });
  if (engine?.load_ms) rows.push({ label: 'Loading models', ms: engine.load_ms });
  if (engine?.score_ms !== undefined) rows.push({ label: 'Score (composition plan)', ms: engine.score_ms, detail: engine.score_tokens ? `${engine.score_tokens} tokens` : undefined });
  if (engine?.semantic_ms !== undefined) {
    rows.push({ label: 'Semantic tokens', ms: engine.semantic_ms, detail: engine.semantic_frames ? `${engine.semantic_frames} frames${engine.truncated ? ', cut at the length limit' : ''}` : undefined });
  }
  if (engine?.acoustic_ms !== undefined) {
    const swaps = engine.acoustic_wall_ms !== undefined ? engine.acoustic_wall_ms - engine.acoustic_ms : 0;
    rows.push({
      label: 'Acoustic synthesis (NAR)',
      ms: engine.acoustic_ms,
      detail: [engine.acoustic_steps && `${engine.acoustic_steps} steps`, engine.solver, engine.evaluations && `${engine.evaluations} passes`].filter(Boolean).join(' · ') || undefined,
    });
    if (swaps > 1) rows.push({ label: 'Prefill and model swaps', ms: swaps });
  }
  if (engine?.decode_ms !== undefined) rows.push({ label: 'VAE decode', ms: engine.decode_ms });
  rows.push({ label: 'Encode and add to library', ms: performance.import_ms });
  return rows;
}

const Row: React.FC<{ label: string; value?: string }> = ({ label, value }) =>
  value === undefined ? null : (
    <>
      <span className="whitespace-nowrap text-right text-zinc-500">{label}</span>
      <span className="min-w-0 font-mono text-zinc-800 wrap-break-word dark:text-zinc-200">{value}</span>
    </>
  );

/**
 * How long a song took to make, stage by stage, from the engine's own timings
 * and the studio's clock. Collapsed by default: it is for checking speed, not
 * for making music.
 */
export const PerformanceDetails: React.FC<{ song: Song }> = ({ song }) => {
  const performance = song.performance;
  const mode = renderModeOf(song.generationParams as Record<string, unknown> | undefined);
  if (!performance) {
    return null;
  }
  const engine = performance.engine;
  const rows = stageRows(performance);
  const total = performance.time_to_playable_ms;
  const resources = performance.resources;
  return (
    <details className="group" data-testid="performance-details">
      <summary className="flex cursor-pointer items-center justify-between rounded-xl bg-zinc-100 px-3 py-2 transition-colors hover:bg-zinc-200 dark:bg-white/5 dark:hover:bg-white/10">
        <span className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5 text-[11px]">
          <Gauge size={13} className="text-zinc-500" />
          <span className="font-medium text-zinc-700 dark:text-zinc-300">Performance</span>
          <span className="rounded-sm bg-zinc-200 px-2 py-0.5 text-zinc-600 dark:bg-white/10 dark:text-zinc-400">{formatMs(total)} to playable</span>
          {mode !== 'studio' && <span className="rounded-sm bg-amber-500/15 px-2 py-0.5 font-medium text-amber-700 dark:text-amber-300">{MODE_LABEL[mode]}</span>}
        </span>
        <ChevronDown size={14} className="ml-2 shrink-0 text-zinc-400 transition-transform group-open:rotate-180" />
      </summary>
      <div className="mt-2 space-y-3 px-1 text-[11px]">
        <div>
          {rows.map(row => (
            <div key={row.label} className="py-0.5">
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-zinc-600 dark:text-zinc-300">{row.label}</span>
                <span className="font-mono tabular-nums text-zinc-800 dark:text-zinc-200">
                  {formatMs(row.ms)}
                  {total > 0 && <span className="ml-1.5 text-zinc-400">{Math.round((row.ms / total) * 100)}%</span>}
                </span>
              </div>
              <div className="mt-0.5 h-1 rounded-full bg-zinc-100 dark:bg-white/5">
                <div className="h-1 rounded-full bg-pink-500/70" style={{ width: `${total > 0 ? Math.min(100, (row.ms / total) * 100) : 0}%` }} />
              </div>
              {row.detail && <div className="mt-0.5 text-[10px] text-zinc-400">{row.detail}</div>}
            </div>
          ))}
        </div>
        <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
          <Row label="Generate to playable" value={formatMs(total)} />
          <Row label="Real-time factor" value={performance.realtime_factor ? `${performance.realtime_factor.toFixed(3)}× (song seconds per second of waiting)` : undefined} />
          <Row label="Song length" value={performance.audio_seconds ? formatMs(performance.audio_seconds * 1000) : undefined} />
          <Row label="Model set" value={performance.model ?? undefined} />
          <Row label="Compute" value={engine?.backend ? (engine.backend.startsWith('MTL') ? `${engine.backend} (Metal GPU)` : engine.backend) : undefined} />
          <Row label="Render" value={`${MODE_LABEL[mode]} · ${performance.solver} · ${performance.steps} steps`} />
          <Row label="Peak memory in use" value={resources && !resources.remote_engine && resources.samples > 0 ? `${(resources.system_used_peak_mb / 1024).toFixed(1)} of ${(resources.system_total_mb / 1024).toFixed(0)} GB (whole system, unified memory)` : undefined} />
          <Row label="Engine process" value={resources?.engine_rss_peak_mb ? `${(resources.engine_rss_peak_mb / 1024).toFixed(1)} GB resident at peak` : undefined} />
          <Row label="Swap" value={resources && !resources.remote_engine && resources.samples > 0 ? (resources.swap_peak_mb > resources.swap_start_mb ? `grew by ${resources.swap_peak_mb - resources.swap_start_mb} MB (memory pressure)` : 'no growth') : undefined} />
          <Row label="Engine CPU" value={resources?.engine_cpu_percent_avg !== undefined ? `${Math.round(resources.engine_cpu_percent_avg)}% average (100% = one core)` : undefined} />
          <Row label="GPU utilization" value={resources?.remote_engine ? 'not measured: the engine runs on another computer' : 'not measurable: macOS does not report it to apps without admin rights'} />
        </div>
        {mode === 'draft' && (
          <p className="leading-4 text-zinc-500">
            A Fast Draft render. To keep it, use Re-render from the song's menu and choose Studio Quality: the same composition and noise seed, rendered at the reference steps.
          </p>
        )}
      </div>
    </details>
  );
};
