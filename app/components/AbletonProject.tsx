import React, { useState } from 'react';
import { Download, Loader2, SlidersHorizontal } from 'lucide-react';
import { saveFile } from '../services/saveFile';

interface Made {
  id: string;
  file_name: string;
  files: string[];
  stems: number;
  midi_parts: number;
  tempo_from_score: boolean;
  bpm: number;
  meter: string;
}

/**
 * The song as an Ableton Live 12 project: a zipped project folder with a set
 * that has the mix, each separated stem and each score part on its own track
 * at bar 1. It is built from what the song has now; a new one is made each
 * time, so an earlier export is never replaced.
 */
export const AbletonProject: React.FC<{ songId: string }> = ({ songId }) => {
  const [busy, setBusy] = useState(false);
  const [made, setMade] = useState<Made | null>(null);
  const [error, setError] = useState<string | null>(null);

  const build = async () => {
    setBusy(true); setError(null); setMade(null);
    try {
      const response = await fetch('/v1/kinetic/ableton', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ song_id: songId }),
      });
      const body = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(body?.error ?? `HTTP ${response.status}`);
      const project = body as Made;
      setMade(project);
      await saveFile(project.file_name, { url: `/v1/kinetic/ableton/${encodeURIComponent(project.id)}/file` });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="mt-5 border-t border-zinc-200 pt-4 text-xs dark:border-white/10" data-testid="ableton-project">
      <h3 className="flex items-center gap-1.5 text-sm font-bold text-zinc-900 dark:text-white"><SlidersHorizontal size={14} />Ableton Live project</h3>
      <p className="mt-1.5 leading-5 text-zinc-500">
        A Live 12 set with the mix, each separated stem and each score part on its own track at bar 1, at the score's tempo, with the files beside it.
        It has not yet been checked by opening it in Live; the read-me inside says which file belongs on which track.
      </p>
      <button
        type="button"
        onClick={() => void build()}
        disabled={busy}
        className="mt-2 inline-flex items-center gap-1.5 rounded-md border border-zinc-200 px-2.5 py-1.5 font-semibold text-zinc-700 hover:border-pink-300 disabled:opacity-60 dark:border-white/10 dark:text-zinc-200"
      >
        {busy ? <Loader2 size={12} className="animate-spin" /> : <Download size={12} />}
        {busy ? 'Building the project…' : 'Save as Live project (.zip)'}
      </button>
      {error && <p className="mt-2 text-rose-600 dark:text-rose-300">The project could not be made: {error}</p>}
      {made && (
        <p className="mt-2 leading-5 text-zinc-500" data-testid="ableton-made">
          {made.file_name}: the mix, {made.stems} {made.stems === 1 ? 'stem' : 'stems'} and {made.midi_parts} MIDI {made.midi_parts === 1 ? 'part' : 'parts'} at {made.bpm} BPM in {made.meter}
          {made.tempo_from_score ? '.' : '. The song has no score, so that is Live’s default tempo, not the song’s.'}
          {made.stems === 0 && ' Separate the stems first to get a track for each.'}
        </p>
      )}
    </section>
  );
};
