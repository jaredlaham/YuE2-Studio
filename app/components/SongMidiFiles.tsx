import React, { useEffect, useState } from 'react';
import { Download, Music2 } from 'lucide-react';
import { saveFile } from '../services/saveFile';

interface ScoreFile { part: string; label: string; notes: number; chords: number; file_name: string }

const counted = (file: ScoreFile) =>
  [file.notes ? `${file.notes} notes` : '', file.chords ? `${file.chords} chords` : ''].filter(Boolean).join(', ');
interface Listing {
  score: { bpm: number; meter: string; key: string; files: ScoreFile[] } | { error: string } | null;
  own: { origin: 'transcribed' | 'edited'; file_name: string } | null;
}

/**
 * A song's MIDI, each file named for where its notes come from: the score the
 * song was generated with, or the track's own MIDI (heard in the audio, or
 * edited). A song made without a score has no score MIDI.
 */
export const SongMidiFiles: React.FC<{ songId: string }> = ({ songId }) => {
  const [listing, setListing] = useState<Listing | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setListing(null); setError(null);
    void fetch(`/v1/kinetic/midi/${encodeURIComponent(songId)}`)
      .then(response => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
      .then(setListing)
      .catch(reason => setError(reason instanceof Error ? reason.message : String(reason)));
  }, [songId]);

  const save = (part: string, name: string) => void saveFile(name, { url: `/v1/kinetic/midi/${encodeURIComponent(songId)}/${part}` });
  const row = (key: string, label: string, detail: string, onSave: () => void) => (
    <li key={key} className="flex items-center gap-2 py-1">
      <span className="min-w-0 flex-1"><span className="font-semibold text-zinc-800 dark:text-zinc-100">{label}</span> <span className="text-zinc-500">{detail}</span></span>
      <button type="button" onClick={onSave} className="inline-flex shrink-0 items-center gap-1 rounded-md border border-zinc-200 px-2 py-1 font-semibold text-zinc-700 hover:border-pink-300 dark:border-white/10 dark:text-zinc-200" aria-label={`Save ${label}`}>
        <Download size={12} />.mid
      </button>
    </li>
  );

  const score = listing?.score && 'files' in listing.score ? listing.score : null;
  return (
    <section className="mt-5 border-t border-zinc-200 pt-4 text-xs dark:border-white/10" data-testid="song-midi">
      <h3 className="flex items-center gap-1.5 text-sm font-bold text-zinc-900 dark:text-white"><Music2 size={14} />MIDI</h3>
      {error && <p className="mt-2 text-rose-600 dark:text-rose-300">The MIDI could not be listed: {error}</p>}
      {listing && (
        <>
          {score ? (
            <>
              <p className="mt-1.5 leading-5 text-zinc-500">
                From the score this song was generated with: {score.bpm} BPM, {score.meter}, {score.key}. These are the notes the model was asked to play; how closely the recording follows them is not measured.
              </p>
              <ul className="mt-1">{score.files.map(file => row(file.part, `${file.label} (from score)`, counted(file), () => save(file.part, file.file_name)))}</ul>
            </>
          ) : (
            <p className="mt-1.5 leading-5 text-zinc-500">
              {listing.score && 'error' in listing.score ? listing.score.error : 'This song was generated without a score, so there is no score MIDI for it.'}
            </p>
          )}
          {listing.own && (
            <ul className="mt-2 border-t border-zinc-100 pt-2 dark:border-white/5">
              {row('own', listing.own.origin === 'edited' ? 'MIDI edited in the MIDI editor' : 'Audio to MIDI (estimated)',
                listing.own.origin === 'edited' ? '' : 'heard in the recording by the transcriber; expect wrong and missing notes',
                () => save('own', listing.own!.file_name))}
            </ul>
          )}
          {!listing.own && <p className="mt-2 leading-5 text-zinc-500">No audio-to-MIDI yet; the tools page can transcribe the recording.</p>}
        </>
      )}
    </section>
  );
};
