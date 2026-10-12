import React, { useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ChevronDown, CircleAlert, Info, Loader2, RotateCcw, Save, SlidersHorizontal, Trash2, X } from 'lucide-react';
import {
  compilePrompt, emptyPrompt, INSTRUMENT_GROUPS, KINETIC_PRESETS, loadUserPresets, METERS, promptFrom, saveUserPresets, TONICS,
  type CompiledPrompt, type InstrumentGroup, type KineticPreset, type KineticPrompt, type Route,
} from '../services/kineticPrompt';
import { composeScore } from '../services/transcription';
import { chordBed, failed, instrumentalScore, markScore } from '../services/scoreApi';

/** What the builder puts in the Create form; the form's fields stay editable afterwards. */
export interface KineticForm {
  style: string;
  lyrics: string;
  cot: 'full';
  durationSeconds: number | null;
  /** The score to render from; empty lets the engine plan its own. */
  abc: string;
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-zinc-50 px-2.5 py-1.5 text-xs text-zinc-900 outline-hidden transition-colors focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/25 dark:text-white';
const LABEL = 'mb-1 block text-[10px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400';
const BUTTON =
  'inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-2.5 py-1.5 text-xs font-semibold text-zinc-700 transition hover:border-pink-300 disabled:opacity-50 dark:border-white/10 dark:text-zinc-200';
const PRIMARY =
  'inline-flex items-center gap-1.5 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-3 py-1.5 text-xs font-bold text-white transition hover:brightness-110 disabled:opacity-50';

const GROUP_LABELS: Record<InstrumentGroup, string> = {
  bass: 'Bass',
  synths: 'Synths',
  percussion: 'Percussion',
  textures: 'Textures',
  melodic: 'Melodic elements',
  support: 'Supporting instruments',
};

const ROUTE_LABELS: Record<Route, { text: string; hint: string; className: string }> = {
  prompt: { text: 'style', hint: 'Written into the style; the model treats it as guidance.', className: 'bg-sky-500/15 text-sky-700 dark:text-sky-300' },
  score: { text: 'score', hint: 'Fixed in the score the model renders.', className: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300' },
  request: { text: 'request', hint: 'Sent as an engine setting.', className: 'bg-violet-500/15 text-violet-700 dark:text-violet-300' },
  omitted: { text: 'not sent', hint: 'Left out, for the reason shown.', className: 'bg-zinc-500/15 text-zinc-600 dark:text-zinc-300' },
};

const split = (text: string) => text.split(',').map(part => part.trim()).filter(Boolean);

/** A comma-separated list that keeps what is being typed (a trailing comma) while the list itself updates. */
const ListInput: React.FC<{ value: string[]; onChange: (value: string[]) => void; placeholder?: string; label: string }> = ({ value, onChange, placeholder, label }) => {
  const [text, setText] = useState(value.join(', '));
  useEffect(() => {
    if (split(text).join('\u0000') !== value.join('\u0000')) setText(value.join(', '));
    // only an outside change (a preset) rewrites the text
  }, [value]);
  return (
    <label className="block">
      <span className={LABEL}>{label}</span>
      <input className={CONTROL} value={text} placeholder={placeholder} onChange={event => { setText(event.target.value); onChange(split(event.target.value)); }} />
    </label>
  );
};

const TextInput: React.FC<{ value: string; onChange: (value: string) => void; placeholder?: string; label: string }> = ({ value, onChange, placeholder, label }) => (
  <label className="block">
    <span className={LABEL}>{label}</span>
    <input className={CONTROL} value={value} placeholder={placeholder} onChange={event => onChange(event.target.value)} />
  </label>
);

const numberOrNull = (text: string) => (text.trim() === '' || !Number.isFinite(Number(text)) ? null : Number(text));

const Issues: React.FC<{ compiled: CompiledPrompt }> = ({ compiled }) => {
  if (!compiled.issues.length) return null;
  return (
    <ul className="mt-2 space-y-1">
      {compiled.issues.map((issue, index) => (
        <li
          key={index}
          className={`flex gap-1.5 text-[11px] leading-4 ${issue.level === 'error' ? 'text-red-600 dark:text-red-400' : issue.level === 'warning' ? 'text-amber-700 dark:text-amber-300' : 'text-zinc-500'}`}
        >
          {issue.level === 'error' ? <CircleAlert size={12} className="mt-0.5 shrink-0" /> : issue.level === 'warning' ? <AlertTriangle size={12} className="mt-0.5 shrink-0" /> : <Info size={12} className="mt-0.5 shrink-0" />}
          {issue.message}
        </li>
      ))}
    </ul>
  );
};

/**
 * Kinetic Studio's structured prompt: pick a preset or fill the fields, read
 * exactly what YuE2 will be sent and how each setting reaches it, then put it
 * in the Create form, where every field can still be edited by hand.
 */
export const KineticPromptPanel: React.FC<{
  /** The engine can plan a score now. */
  engineReady: boolean;
  onApply: (form: KineticForm) => void;
  /** Puts back what the form held before the last apply, when there is one. */
  onUndo?: () => void;
}> = ({ engineReady, onApply, onUndo }) => {
  const [open, setOpen] = useState(() => {
    try { return window.localStorage.getItem('kinetic.builderOpen') !== '0'; } catch { return true; }
  });
  const toggle = () => setOpen(value => {
    try { window.localStorage.setItem('kinetic.builderOpen', value ? '0' : '1'); } catch { /* remembered until a reload */ }
    return !value;
  });
  const [prompt, setPrompt] = useState<KineticPrompt>(() => promptFrom(KINETIC_PRESETS[0]));
  const [presetId, setPresetId] = useState(KINETIC_PRESETS[0].id);
  const [userPresets, setUserPresets] = useState<KineticPreset[]>([]);
  const [presetError, setPresetError] = useState<string | null>(null);
  const [saveName, setSaveName] = useState('');
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState<null | 'bed' | 'compose'>(null);
  const [seconds, setSeconds] = useState(0);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = useRef<AbortController | null>(null);

  useEffect(() => {
    loadUserPresets().then(setUserPresets).catch(reason => setPresetError(`Your saved presets could not be read: ${reason instanceof Error ? reason.message : String(reason)}`));
  }, []);

  useEffect(() => {
    if (!busy) return;
    setSeconds(0);
    const started = Date.now();
    const timer = window.setInterval(() => setSeconds(Math.round((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, [busy]);

  useEffect(() => () => run.current?.abort(), []);

  const compiled = useMemo(() => compilePrompt(prompt), [prompt]);
  const blocked = compiled.issues.some(issue => issue.level === 'error');
  const presets = [...KINETIC_PRESETS, ...userPresets];
  const selected = presets.find(entry => entry.id === presetId) ?? null;
  const instrumental = prompt.mode === 'instrumental';

  const set = <K extends keyof KineticPrompt>(key: K, value: KineticPrompt[K]) => setPrompt(current => ({ ...current, [key]: value }));
  const setGroup = (group: InstrumentGroup, value: string[]) => setPrompt(current => ({ ...current, instruments: { ...current.instruments, [group]: value } }));

  const choosePreset = (id: string) => {
    const entry = presets.find(candidate => candidate.id === id);
    setPresetId(id);
    setConfirmDelete(false);
    setPrompt(entry ? promptFrom(entry) : emptyPrompt(prompt.mode));
  };

  const persist = async (next: KineticPreset[], done: string) => {
    setPresetError(null);
    try {
      await saveUserPresets(next);
      setUserPresets(next);
      setMessage(done);
      return true;
    } catch (reason) {
      setPresetError(`Not saved: ${reason instanceof Error ? reason.message : String(reason)}`);
      return false;
    }
  };

  const saveAsNew = async () => {
    const name = saveName.trim();
    if (!name) return;
    const entry: KineticPreset = { id: `user-${Date.now().toString(36)}`, name, builtIn: false, prompt: JSON.parse(JSON.stringify(prompt)) as KineticPrompt };
    if (await persist([...userPresets, entry], `Saved "${name}".`)) {
      setPresetId(entry.id);
      setSaveName('');
    }
  };

  const saveOver = async () => {
    if (!selected || selected.builtIn) return;
    await persist(userPresets.map(entry => (entry.id === selected.id ? { ...entry, prompt: JSON.parse(JSON.stringify(prompt)) as KineticPrompt } : entry)), `Updated "${selected.name}".`);
  };

  const remove = async () => {
    if (!selected || selected.builtIn) return;
    if (!confirmDelete) { setConfirmDelete(true); return; }
    setConfirmDelete(false);
    if (await persist(userPresets.filter(entry => entry.id !== selected.id), `Deleted "${selected.name}".`)) setPresetId(KINETIC_PRESETS[0].id);
  };

  const apply = (abc: string) => {
    onApply({ style: compiled.style, lyrics: compiled.lyrics, cot: compiled.cot, durationSeconds: compiled.durationSeconds, abc });
  };

  const useInForm = () => {
    setError(null);
    apply('');
    setMessage('The style and lyrics are in the form below; edit them there before you create.');
  };

  const writeLockedScore = async () => {
    if (!compiled.chordBed) return;
    setError(null); setMessage(null); setBusy('bed');
    try {
      const answer = await chordBed(compiled.chordBed);
      if (failed(answer)) { setError(answer.error); return; }
      apply(answer.abc.trimEnd());
      setMessage(`Wrote a ${answer.bars}-bar chord bed at ${compiled.chordBed.bpm} BPM in ${compiled.chordBed.key}, ${compiled.chordBed.meter}. The model renders this score instead of planning its own.`);
    } finally {
      setBusy(null);
    }
  };

  /** m-a-p's instrumental workflow: plan from the style and section tags, then move every sung note to the instrument voice. */
  const composeInstrumental = async () => {
    setError(null); setMessage(null); setBusy('compose');
    const controller = new AbortController();
    run.current = controller;
    try {
      const planned = await composeScore({ style: compiled.style, lyrics: compiled.lyrics, cot: compiled.cot }, controller.signal);
      const moved = await instrumentalScore(planned);
      if (failed(moved)) { setError(moved.error); return; }
      const marked = await markScore(moved.abc, compiled.style, compiled.lyrics, compiled.cot, false);
      if (failed(marked)) { setError(marked.error); return; }
      if (controller.signal.aborted) return;
      apply(marked.abc.trimEnd());
      setMessage(`Planned a score and moved ${moved.moved} sung note${moved.moved === 1 ? '' : 's'} to the instrument voice. Listen to the result: "no vocals" is guidance, not a guarantee.`);
    } catch (reason) {
      if (!(reason instanceof DOMException && reason.name === 'AbortError')) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      run.current = null;
      setBusy(null);
    }
  };

  const keyValue = prompt.key ? prompt.key.tonic : '';

  return (
    <div className="overflow-hidden rounded-xl border border-pink-300/40 bg-white dark:border-pink-500/20 dark:bg-suno-card">
      <button
        type="button"
        onClick={toggle}
        aria-expanded={open}
        className="flex w-full items-center justify-between gap-2 border-b border-zinc-100 bg-zinc-50 px-3 py-2 text-left dark:border-white/5 dark:bg-white/5"
      >
        <span className="flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
          <SlidersHorizontal size={13} />Kinetic prompt builder
        </span>
        <ChevronDown size={14} className={`text-zinc-400 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && (
        <div className="space-y-3 p-3">
          <div className="flex flex-wrap items-end gap-2">
            <label className="min-w-0 flex-1">
              <span className={LABEL}>Preset</span>
              <select className={CONTROL} value={presetId} onChange={event => choosePreset(event.target.value)}>
                <optgroup label="Kinetic presets">
                  {KINETIC_PRESETS.map(entry => <option key={entry.id} value={entry.id}>{entry.name}</option>)}
                </optgroup>
                {userPresets.length > 0 && (
                  <optgroup label="Your presets">
                    {userPresets.map(entry => <option key={entry.id} value={entry.id}>{entry.name}</option>)}
                  </optgroup>
                )}
              </select>
            </label>
            <button type="button" className={BUTTON} onClick={() => choosePreset(presetId)} title="Discard edits and reload the preset"><RotateCcw size={12} />Reload</button>
            {selected && !selected.builtIn && (
              <>
                <button type="button" className={BUTTON} onClick={() => void saveOver()}><Save size={12} />Update</button>
                <button type="button" className={BUTTON} onClick={() => void remove()}><Trash2 size={12} />{confirmDelete ? 'Delete for good?' : 'Delete'}</button>
              </>
            )}
          </div>
          <div className="flex gap-2">
            <input className={CONTROL} value={saveName} placeholder="Name for a new preset" onChange={event => setSaveName(event.target.value)} />
            <button type="button" className={BUTTON} disabled={!saveName.trim()} onClick={() => void saveAsNew()}><Save size={12} /><span className="whitespace-nowrap">Save as new</span></button>
          </div>
          {presetError && <p className="text-[11px] text-red-600 dark:text-red-400">{presetError}</p>}

          <div className="grid grid-cols-2 gap-1 rounded-lg bg-zinc-100 p-1 dark:bg-black/30" role="tablist">
            {(['instrumental', 'vocal'] as const).map(value => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-selected={prompt.mode === value}
                onClick={() => set('mode', value)}
                className={`rounded-md py-1 text-xs font-semibold ${prompt.mode === value ? 'bg-white text-black shadow-xs dark:bg-zinc-800 dark:text-white' : 'text-zinc-500'}`}
              >
                {value === 'instrumental' ? 'Instrumental' : 'With vocals'}
              </button>
            ))}
          </div>

          <section className="grid grid-cols-2 gap-2">
            <TextInput label="Genre" value={prompt.genre} onChange={value => set('genre', value)} placeholder="experimental trip-hop" />
            <ListInput label="Influences (described, not artists)" value={prompt.influences} onChange={value => set('influences', value)} placeholder="1990s Bristol sound" />
            <ListInput label="Mood" value={prompt.mood} onChange={value => set('mood', value)} placeholder="hypnotic, smoky" />
            <TextInput label="Creative direction" value={prompt.direction} onChange={value => set('direction', value)} placeholder="late night city drive" />
          </section>

          <section className="grid grid-cols-2 gap-2">
            {INSTRUMENT_GROUPS.map(group => (
              <ListInput key={group} label={GROUP_LABELS[group]} value={prompt.instruments[group]} onChange={value => setGroup(group, value)} />
            ))}
          </section>

          <section className="grid grid-cols-2 gap-2">
            <ListInput label="Sections in order" value={prompt.sections} onChange={value => set('sections', value)} placeholder="Intro, Verse, Build, Drop, Outro" />
            <ListInput label="Arrangement" value={prompt.arrangement} onChange={value => set('arrangement', value)} placeholder="slow swells" />
            <ListInput label="Production" value={prompt.production} onChange={value => set('production', value)} placeholder="wide stereo field" />
            <ListInput label="Exclusions (best-effort)" value={prompt.exclusions} onChange={value => set('exclusions', value)} placeholder="drums, guitar" />
          </section>

          <section className="grid grid-cols-4 gap-2">
            <label className="block">
              <span className={LABEL}>Tempo (BPM)</span>
              <input className={CONTROL} inputMode="numeric" value={prompt.tempo ?? ''} onChange={event => set('tempo', numberOrNull(event.target.value))} />
            </label>
            <label className="block">
              <span className={LABEL}>Key</span>
              <select
                className={CONTROL}
                value={keyValue}
                onChange={event => set('key', event.target.value ? { tonic: event.target.value, minor: prompt.key?.minor ?? true } : null)}
              >
                <option value="">model decides</option>
                {TONICS.map(tonic => <option key={tonic} value={tonic}>{tonic}</option>)}
              </select>
            </label>
            <label className="block">
              <span className={LABEL}>Scale</span>
              <select className={CONTROL} disabled={!prompt.key} value={prompt.key?.minor ? 'minor' : 'major'} onChange={event => prompt.key && set('key', { ...prompt.key, minor: event.target.value === 'minor' })}>
                <option value="minor">minor</option>
                <option value="major">major</option>
              </select>
            </label>
            <label className="block">
              <span className={LABEL}>Meter</span>
              <select className={CONTROL} value={prompt.meter} onChange={event => set('meter', event.target.value)}>
                {METERS.map(meter => <option key={meter} value={meter}>{meter}</option>)}
              </select>
            </label>
            <div className="col-span-2"><TextInput label="Harmony" value={prompt.harmony} onChange={value => set('harmony', value)} placeholder="slowly shifting minor chords" /></div>
            <div className="col-span-1"><TextInput label="Energy" value={prompt.energy} onChange={value => set('energy', value)} placeholder="low, rising" /></div>
            <label className="block">
              <span className={LABEL}>Length (s)</span>
              <input className={CONTROL} inputMode="numeric" value={prompt.durationSeconds ?? ''} placeholder="form's value" onChange={event => set('durationSeconds', numberOrNull(event.target.value))} />
            </label>
          </section>
          <label className="flex items-start gap-2 text-[11px] leading-4 text-zinc-600 dark:text-zinc-300">
            <input type="checkbox" className="mt-0.5 accent-pink-500" checked={prompt.lockToScore} onChange={event => set('lockToScore', event.target.checked)} />
            <span>
              Lock tempo, key and meter to the score. The studio writes a chord bed at these settings and the model renders it instead of planning its
              own melody. Without the lock, tempo and key are only words in the style, and the model may not follow them.
            </span>
          </label>

          {!instrumental && (
            <section className="grid grid-cols-2 gap-2">
              <TextInput label="Language" value={prompt.language} onChange={value => set('language', value)} />
              <TextInput label="Voice" value={prompt.voice} onChange={value => set('voice', value)} placeholder="breathy female voice" />
              <label className="col-span-2 block">
                <span className={LABEL}>Lyrics</span>
                <textarea className={`${CONTROL} min-h-24 font-mono`} value={prompt.lyrics} onChange={event => set('lyrics', event.target.value)} placeholder={'[Verse]\n...'} />
              </label>
            </section>
          )}

          <section className="rounded-lg border border-zinc-200 bg-zinc-50 p-2.5 dark:border-white/10 dark:bg-black/25">
            <div className="flex items-baseline justify-between">
              <span className={LABEL}>What YuE2 is sent</span>
              <span className={`text-[10px] tabular-nums ${compiled.words > 50 ? 'text-amber-600' : 'text-zinc-400'}`}>{compiled.words} words</span>
            </div>
            <p className="font-mono text-[11px] leading-4 text-zinc-800 dark:text-zinc-100">[Tags] {compiled.style || '(empty)'}</p>
            <pre className="mt-1.5 max-h-28 overflow-y-auto whitespace-pre-wrap font-mono text-[11px] leading-4 text-zinc-600 dark:text-zinc-300">[Lyrics]{'\n'}{compiled.lyrics || '(empty)'}</pre>
            <Issues compiled={compiled} />
            <table className="mt-2 w-full text-left text-[11px] leading-4">
              <tbody>
                {compiled.controls.map((control, index) => (
                  <tr key={index} className="border-t border-zinc-200/70 dark:border-white/5">
                    <td className="py-0.5 pr-2 align-top text-zinc-500">{control.setting}</td>
                    <td className="py-0.5 pr-2 align-top">
                      <span className={`rounded px-1 text-[10px] font-semibold ${ROUTE_LABELS[control.route].className}`} title={ROUTE_LABELS[control.route].hint}>{ROUTE_LABELS[control.route].text}</span>
                    </td>
                    <td className="py-0.5 text-zinc-700 dark:text-zinc-300">{control.detail}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>

          <div className="flex flex-wrap items-center gap-2">
            {prompt.lockToScore ? (
              <button type="button" className={PRIMARY} disabled={blocked || busy !== null || !compiled.chordBed} onClick={() => void writeLockedScore()}>
                {busy === 'bed' && <Loader2 size={13} className="animate-spin" />}Use in form with a locked score
              </button>
            ) : (
              <button type="button" className={PRIMARY} disabled={blocked || busy !== null} onClick={useInForm}>Use in form</button>
            )}
            {instrumental && !prompt.lockToScore && (
              <button
                type="button"
                className={BUTTON}
                disabled={blocked || busy !== null || !engineReady}
                onClick={() => void composeInstrumental()}
                title={engineReady ? 'Plan a score from the style and sections, then move any sung notes to the instrument voice' : 'Install and start the music engine first'}
              >
                {busy === 'compose' && <Loader2 size={13} className="animate-spin" />}Compose an instrumental score
              </button>
            )}
            {busy === 'compose' && (
              <span className="inline-flex items-center gap-1.5 text-[11px] text-zinc-500">
                <span className="tabular-nums">{seconds} s</span>
                <button type="button" onClick={() => run.current?.abort()} className="rounded p-0.5 hover:bg-zinc-200 dark:hover:bg-white/10" title="Cancel"><X size={13} /></button>
              </span>
            )}
            {onUndo && (
              <button type="button" className={BUTTON} disabled={busy !== null} onClick={() => { onUndo(); setMessage('The form is back as it was.'); }}>
                <RotateCcw size={12} />Undo
              </button>
            )}
          </div>
          {blocked && <p className="text-[11px] text-red-600 dark:text-red-400">Fix the errors above first.</p>}
          {error && <p className="text-[11px] text-red-600 dark:text-red-400">{error}</p>}
          {message && <p className="text-[11px] leading-4 text-zinc-500">{message}</p>}
        </div>
      )}
    </div>
  );
};
