import React, { useEffect, useRef, useState } from 'react';
import { Check, CircleDashed, Download, Loader2, Recycle, SkipForward, SlidersHorizontal, X } from 'lucide-react';
import { saveFile } from '../services/saveFile';
import { formatMs } from './PerformanceDetails';

interface Made {
  id: string;
  file_name: string;
  files: string[];
  stems: number;
  midi_parts: number;
  tempo_from_score: boolean;
  bpm: number;
  meter: string;
  tracks: string[];
  left_off: string[];
}

export interface ExportStage {
  id: string;
  label: string;
  state: 'pending' | 'waiting' | 'running' | 'reused' | 'done' | 'skipped' | 'failed';
  progress: number;
  detail: string;
  ms?: number;
}

export interface ExportJob {
  id: string;
  song_id: string;
  stages: ExportStage[];
  done: boolean;
  error?: string;
  project?: Made;
  total_ms?: number;
}

const ICON: Record<ExportStage['state'], React.ReactNode> = {
  pending: <CircleDashed size={12} className="text-zinc-400" />,
  waiting: <CircleDashed size={12} className="text-amber-500" />,
  running: <Loader2 size={12} className="animate-spin text-pink-500" />,
  reused: <Recycle size={12} className="text-emerald-500" />,
  done: <Check size={12} className="text-emerald-500" />,
  skipped: <SkipForward size={12} className="text-zinc-400" />,
  failed: <X size={12} className="text-rose-500" />,
};

/**
 * The song as an Ableton Live 12 project, made on demand and never as part of
 * generating it. The export reuses what the song already has (stems, a
 * transcription), makes only what is missing and asked for, holds back while
 * a song is being generated, and shows each stage. The set has the generated
 * master on an active track and the stems on tracks that are switched off.
 * Playback is untouched while it runs; a new zip is made each time.
 */
export const AbletonProject: React.FC<{ songId: string }> = ({ songId }) => {
  const [job, setJob] = useState<ExportJob | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [stems, setStems] = useState(true);
  const [transcribe, setTranscribe] = useState(false);
  const saved = useRef<string | null>(null);
  const busy = job !== null && !job.done;

  useEffect(() => {
    if (!job || job.done) return;
    const timer = window.setTimeout(async () => {
      try {
        const response = await fetch(`/v1/kinetic/ableton/jobs/${encodeURIComponent(job.id)}`);
        const body = await response.json().catch(() => ({}));
        if (!response.ok) throw new Error(body?.error ?? `HTTP ${response.status}`);
        setJob(body as ExportJob);
      } catch (reason) {
        setError(reason instanceof Error ? reason.message : String(reason));
        setJob(current => (current ? { ...current, done: true } : current));
      }
    }, 700);
    return () => window.clearTimeout(timer);
  }, [job]);

  // the zip is offered once, when the job has written it
  useEffect(() => {
    const project = job?.project;
    if (!project || saved.current === project.id) return;
    saved.current = project.id;
    void saveFile(project.file_name, { url: `/v1/kinetic/ableton/${encodeURIComponent(project.id)}/file` });
  }, [job]);

  const build = async () => {
    setError(null); setJob(null);
    try {
      const response = await fetch('/v1/kinetic/ableton/jobs', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: songId, stems, transcribe }),
      });
      const body = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(body?.error ?? `HTTP ${response.status}`);
      setJob(body as ExportJob);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const made = job?.project;
  return (
    <section className="mt-5 border-t border-zinc-200 pt-4 text-xs dark:border-white/10" data-testid="ableton-project">
      <h3 className="flex items-center gap-1.5 text-sm font-bold text-zinc-900 dark:text-white"><SlidersHorizontal size={14} />Ableton Live project</h3>
      <p className="mt-1.5 leading-5 text-zinc-500">
        A Live 12 set with the generated master on an active track, the separated stems on tracks that are switched off, and each score part as MIDI,
        at the score's tempo. It reuses stems and MIDI the song already has and makes only what is missing. It never runs while you generate, and
        the song keeps playing while it works.
      </p>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-zinc-600 dark:text-zinc-300">
        <label className="inline-flex items-center gap-1.5"><input type="checkbox" checked={stems} disabled={busy} onChange={event => setStems(event.target.checked)} />Separate stems if the song has none</label>
        <label className="inline-flex items-center gap-1.5"><input type="checkbox" checked={transcribe} disabled={busy} onChange={event => setTranscribe(event.target.checked)} />Transcribe the master to MIDI</label>
      </div>
      <button
        type="button"
        onClick={() => void build()}
        disabled={busy}
        className="mt-2 inline-flex items-center gap-1.5 rounded-md border border-zinc-200 px-2.5 py-1.5 font-semibold text-zinc-700 hover:border-pink-300 disabled:opacity-60 dark:border-white/10 dark:text-zinc-200"
      >
        {busy ? <Loader2 size={12} className="animate-spin" /> : <Download size={12} />}
        {busy ? 'Preparing the project…' : 'Export to Ableton (.zip)'}
      </button>
      {job && (
        <ol className="mt-2 space-y-1" data-testid="ableton-stages">
          {job.stages.map(stage => (
            <li key={stage.id} className="flex items-start gap-1.5">
              <span className="mt-0.5">{ICON[stage.state]}</span>
              <span className="min-w-0 flex-1">
                <span className="font-semibold text-zinc-700 dark:text-zinc-200">{stage.label}</span>
                {stage.detail && <span className="text-zinc-500"> · {stage.detail}</span>}
                {stage.state === 'running' && stage.progress > 0 && (
                  <span className="mt-0.5 block h-1 rounded-full bg-zinc-100 dark:bg-white/5"><span className="block h-1 rounded-full bg-pink-500" style={{ width: `${Math.round(stage.progress * 100)}%` }} /></span>
                )}
              </span>
              {stage.ms !== undefined && stage.ms > 0 && <span className="font-mono tabular-nums text-zinc-400">{formatMs(stage.ms)}</span>}
            </li>
          ))}
        </ol>
      )}
      {(error || job?.error) && <p className="mt-2 text-rose-600 dark:text-rose-300">The project could not be made: {error ?? job?.error}</p>}
      {made && (
        <p className="mt-2 leading-5 text-zinc-500" data-testid="ableton-made">
          {made.file_name}: {made.tracks.length} {made.tracks.length === 1 ? 'track' : 'tracks'} ({made.tracks.join(', ')}) at {made.bpm} BPM in {made.meter}
          {made.tempo_from_score ? '.' : '. The song has no score, so the tempo is measured or Live’s default; the stage above says which.'}
          {made.left_off.length > 0 && ` In the folder only: ${made.left_off.map(file => file.split('/').pop()).join(', ')}.`}
          {job?.total_ms !== undefined && ` Ready in ${formatMs(job.total_ms)}.`}
        </p>
      )}
    </section>
  );
};
