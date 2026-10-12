/**
 * Kinetic Studio's structured prompt and its compiler.
 *
 * YuE2 reads two texts: a style under [Tags] and lyrics under [Lyrics]. It has
 * no tempo, key, meter or negative-prompt field (yue2 protocol.py:113-114), so
 * most structured settings can only become descriptors in the style, which the
 * model follows as guidance. The compiler says, setting by setting, how each
 * one reaches the model:
 *
 * - `prompt`  - written into the style as a descriptor; best-effort guidance.
 * - `score`   - fixed in the score the model sings from (a chord bed written by
 *               the studio at the given tempo, key and meter); the model renders
 *               that score rather than planning its own.
 * - `request` - an engine request field.
 * - `omitted` - not sent, with the reason.
 *
 * Nothing the person wrote is dropped silently: every omission, merge and
 * conflict is reported.
 *
 * Instrumental mode follows m-a-p's own instrumental workflow
 * (skills/yue2-music/instrumental): plan a score from the style and section
 * tags only, move every Vocal note to the Ins voice, then render with the
 * section tags as the only lyrics. The model still decides the sound; the
 * style's "no vocals" is guidance, and only listening confirms a track has no
 * voice.
 */

export type PromptMode = 'instrumental' | 'vocal';

export const INSTRUMENT_GROUPS = ['bass', 'synths', 'percussion', 'textures', 'melodic', 'support'] as const;
export type InstrumentGroup = (typeof INSTRUMENT_GROUPS)[number];

export interface KineticPrompt {
  mode: PromptMode;
  /** Musical style */
  genre: string;
  /** Described sound of influences ("1990s Bristol trip-hop"), not artist names: YuE2 does not know artists. */
  influences: string[];
  mood: string[];
  direction: string;
  /** Instrumentation */
  instruments: Record<InstrumentGroup, string[]>;
  /** Arrangement: sections in order, and how the piece develops. */
  sections: string[];
  arrangement: string[];
  /** Production */
  production: string[];
  /** Musical parameters */
  tempo: number | null;
  key: { tonic: string; minor: boolean } | null;
  meter: string;
  harmony: string;
  energy: string;
  /** Fix tempo, key and meter in a chord-bed score instead of leaving them to the model. */
  lockToScore: boolean;
  /** Engine target in seconds; the model may end earlier. */
  durationSeconds: number | null;
  /** Exclusions, best-effort. */
  exclusions: string[];
  /** Vocal mode only. */
  language: string;
  voice: string;
  lyrics: string;
}

export type Route = 'prompt' | 'score' | 'request' | 'omitted';

export interface ControlReport {
  setting: string;
  route: Route;
  detail: string;
}

export interface Issue {
  level: 'error' | 'warning' | 'note';
  message: string;
}

export interface CompiledPrompt {
  /** What YuE2 reads under [Tags]. */
  style: string;
  /** What YuE2 reads under [Lyrics]: section tags only for an instrumental. */
  lyrics: string;
  /** The request's chain-of-thought mode. */
  cot: 'full';
  /** A chord bed to write before rendering, when the parameters are locked to the score. */
  chordBed: { bpm: number; key: string; meter: string; lyrics: string } | null;
  durationSeconds: number | null;
  words: number;
  controls: ControlReport[];
  issues: Issue[];
}

/** YuE2's own section tags (the studio's lyric rules) and the score reader's extra labels. */
export const SECTION_TAGS = ['Intro', 'Verse', 'Pre-Chorus', 'Chorus', 'Bridge', 'Interlude', 'Instrumental', 'Outro'] as const;

/** Producer words for sections, mapped to the tag the model knows. */
const SECTION_ALIASES: Record<string, string> = {
  intro: 'Intro',
  verse: 'Verse',
  groove: 'Verse',
  theme: 'Verse',
  'pre-chorus': 'Pre-Chorus',
  prechorus: 'Pre-Chorus',
  build: 'Pre-Chorus',
  buildup: 'Pre-Chorus',
  'build-up': 'Pre-Chorus',
  rise: 'Pre-Chorus',
  chorus: 'Chorus',
  hook: 'Chorus',
  drop: 'Chorus',
  climax: 'Chorus',
  peak: 'Chorus',
  bridge: 'Bridge',
  breakdown: 'Bridge',
  development: 'Bridge',
  interlude: 'Interlude',
  instrumental: 'Instrumental',
  'instrumental break': 'Instrumental',
  solo: 'Instrumental',
  outro: 'Outro',
  ending: 'Outro',
  'fade-out': 'Outro',
};

export const METERS = ['4/4', '3/4', '6/8', '2/4'] as const;
export const TONICS = ['C', 'C#', 'Db', 'D', 'D#', 'Eb', 'E', 'F', 'F#', 'Gb', 'G', 'G#', 'Ab', 'A', 'A#', 'Bb', 'B'] as const;
export const TEMPO_RANGE = [40, 200] as const;
/** The studio's style contract: 12-50 words of comma-separated descriptors. */
export const STYLE_WORDS = [12, 50] as const;

/** m-a-p's own instrumental example closes its style with these. */
const INSTRUMENTAL_EXCLUSIONS = ['no vocals', 'no singing', 'no choir', 'no spoken words'];

export function emptyPrompt(mode: PromptMode = 'instrumental'): KineticPrompt {
  return {
    mode,
    genre: '',
    influences: [],
    mood: [],
    direction: '',
    instruments: { bass: [], synths: [], percussion: [], textures: [], melodic: [], support: [] },
    sections: ['Intro', 'Verse', 'Chorus', 'Outro'],
    arrangement: [],
    production: [],
    tempo: null,
    key: null,
    meter: '4/4',
    harmony: '',
    energy: '',
    lockToScore: false,
    durationSeconds: null,
    exclusions: [],
    language: 'English',
    voice: '',
    lyrics: '',
  };
}

const clean = (text: string) => text.replace(/\s+/g, ' ').replace(/^[\s,.;]+|[\s,.;]+$/g, '');
const words = (text: string) => (text.trim() ? text.trim().split(/\s+/).length : 0);

/** The tag YuE2 knows for a section name, or null when it has none. */
export function sectionTag(name: string): string | null {
  const key = clean(name).toLowerCase().replace(/[\s\d:#.]+$/, '').trim();
  return SECTION_ALIASES[key] ?? null;
}

/** The key as the chord bed reads it: `Dm`, `F#`, `Bb`. */
export function bedKey(key: { tonic: string; minor: boolean }): string {
  return `${key.tonic}${key.minor ? 'm' : ''}`;
}

export function keyName(key: { tonic: string; minor: boolean }): string {
  return `${key.tonic} ${key.minor ? 'minor' : 'major'}`;
}

/** Lowercased words of a phrase, for spotting an excluded instrument among the included ones. */
function terms(text: string): string[] {
  return text.toLowerCase().replace(/^no\s+/, '').split(/[^a-z0-9#]+/).filter(term => term.length > 2);
}

/** Section tags as YuE2 reads them: one per line, a blank line between, numbered when repeated. */
function tagLyrics(tags: string[]): string {
  const counts = new Map<string, number>();
  for (const tag of tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  const seen = new Map<string, number>();
  return tags
    .map(tag => {
      const index = (seen.get(tag) ?? 0) + 1;
      seen.set(tag, index);
      const numbered = (counts.get(tag) ?? 0) > 1 && (tag === 'Verse' || tag === 'Chorus') ? `${tag} ${index}` : tag;
      return `[${numbered}]`;
    })
    .join('\n\n');
}

export function compilePrompt(prompt: KineticPrompt): CompiledPrompt {
  const controls: ControlReport[] = [];
  const issues: Issue[] = [];
  const parts: string[] = [];
  const seen = new Map<string, string>();
  const instrumental = prompt.mode === 'instrumental';

  /** Adds descriptors once, case-insensitively, saying where a repeat was merged. */
  const add = (setting: string, values: string[]) => {
    const kept: string[] = [];
    for (const raw of values) {
      const value = clean(raw);
      if (!value) continue;
      const id = value.toLowerCase();
      const earlier = seen.get(id);
      if (earlier) {
        issues.push({ level: 'note', message: `"${value}" in ${setting} repeats ${earlier}; written once.` });
        continue;
      }
      seen.set(id, setting);
      kept.push(value);
    }
    if (kept.length) {
      parts.push(...kept);
      controls.push({ setting, route: 'prompt', detail: kept.join(', ') });
    }
    return kept;
  };

  // Order of the studio's style contract: language or "Instrumental", genre, voice,
  // instruments, mood, production, tempo last.
  if (instrumental) {
    add('mode', ['Instrumental']);
  } else {
    add('language', [prompt.language || 'English']);
  }
  add('genre', [prompt.genre]);
  add('influences', prompt.influences);
  if (!prompt.genre.trim()) issues.push({ level: 'warning', message: 'No genre: the model has nothing to anchor the style to.' });

  if (instrumental) {
    if (prompt.voice.trim()) {
      controls.push({ setting: 'voice', route: 'omitted', detail: `"${clean(prompt.voice)}" is not used in instrumental mode.` });
      issues.push({ level: 'warning', message: 'A voice is described but the mode is instrumental; the voice is left out.' });
    }
    if (prompt.lyrics.trim()) {
      controls.push({ setting: 'lyrics', route: 'omitted', detail: 'Instrumental mode sends section tags only, no sung words.' });
      issues.push({ level: 'warning', message: 'Lyrics are written but the mode is instrumental; they are not sent.' });
    }
  } else if (prompt.voice.trim()) {
    add('voice', [prompt.voice]);
  } else {
    issues.push({ level: 'note', message: 'No voice described; the model chooses the singer.' });
  }

  const included: string[] = [];
  for (const group of INSTRUMENT_GROUPS) included.push(...add(`instruments.${group}`, prompt.instruments[group]));
  if (!included.length) issues.push({ level: 'warning', message: 'No instruments named: the model picks the instrumentation.' });

  add('mood', prompt.mood);
  add('direction', [prompt.direction]);
  add('harmony', [prompt.harmony]);
  add('energy', [prompt.energy]);
  add('arrangement', prompt.arrangement);
  add('production', prompt.production);

  // Key: m-a-p's own instrumental example names it in the style ("G major");
  // the studio's song contract leaves it out of a sung song's style.
  const keyValid = prompt.key && TONICS.includes(prompt.key.tonic as (typeof TONICS)[number]);
  if (prompt.key && !keyValid) issues.push({ level: 'error', message: `"${prompt.key.tonic}" is not a key the chord bed can write.` });
  if (prompt.key && keyValid) {
    if (instrumental) add('key', [keyName(prompt.key)]);
    else if (!prompt.lockToScore) controls.push({ setting: 'key', route: 'omitted', detail: "A sung song's style carries no key; lock to the score to fix it." });
  }

  // Meter: no style descriptor does it; only the score can.
  const meterValid = (METERS as readonly string[]).includes(prompt.meter);
  if (!meterValid) issues.push({ level: 'error', message: `Meter ${prompt.meter} is not one the chord bed writes (${METERS.join(', ')}).` });
  else if (!prompt.lockToScore && prompt.meter !== '4/4') controls.push({ setting: 'meter', route: 'omitted', detail: `${prompt.meter} needs the score: turn on lock to score.` });

  // Exclusions, best-effort, written as "no ..." descriptors.
  const exclusionValues = prompt.exclusions.map(clean).filter(Boolean).map(value => (/^no\s/i.test(value) ? value : `no ${value}`));
  for (const exclusion of exclusionValues) {
    const excluded = terms(exclusion);
    const clash = included.find(value => terms(value).some(term => excluded.includes(term)));
    if (clash) issues.push({ level: 'error', message: `"${exclusion}" contradicts "${clash}". Remove one of them.` });
  }
  if (instrumental) {
    const missing = INSTRUMENTAL_EXCLUSIONS.filter(value => !exclusionValues.some(existing => existing.toLowerCase() === value));
    exclusionValues.push(...missing);
  }
  const keptExclusions = add('exclusions', exclusionValues);
  if (keptExclusions.length) {
    const last = controls[controls.length - 1];
    last.detail += ' (best-effort: YuE2 has no negative prompt)';
  }

  // Tempo last, as "<n> BPM".
  let tempo: number | null = null;
  if (prompt.tempo !== null) {
    if (!Number.isInteger(prompt.tempo) || prompt.tempo < TEMPO_RANGE[0] || prompt.tempo > TEMPO_RANGE[1]) {
      issues.push({ level: 'error', message: `Tempo ${prompt.tempo} is outside ${TEMPO_RANGE[0]}-${TEMPO_RANGE[1]} BPM.` });
    } else {
      tempo = prompt.tempo;
      parts.push(`${tempo} BPM`);
      controls.push({ setting: 'tempo', route: prompt.lockToScore ? 'score' : 'prompt', detail: `${tempo} BPM` });
    }
  }

  // Sections: the tags YuE2 knows, in order.
  const tags: string[] = [];
  for (const name of prompt.sections) {
    if (!clean(name)) continue;
    const tag = sectionTag(name);
    if (!tag) {
      issues.push({ level: 'warning', message: `Section "${clean(name)}" has no YuE2 tag; it is sent as [Verse].` });
      tags.push('Verse');
    } else {
      if (tag.toLowerCase() !== clean(name).toLowerCase().replace(/[\s\d]+$/, '')) issues.push({ level: 'note', message: `Section "${clean(name)}" is sent as [${tag}].` });
      tags.push(tag);
    }
  }
  if (!tags.length) tags.push('Intro', 'Verse', 'Chorus', 'Outro');
  if (instrumental && tags[0] !== 'Intro') issues.push({ level: 'note', message: 'The piece does not start with an Intro section.' });

  let lyrics: string;
  if (instrumental) {
    lyrics = tagLyrics(tags);
    controls.push({ setting: 'sections', route: 'prompt', detail: tags.join(' → ') });
  } else {
    lyrics = prompt.lyrics.replace(/\r\n?/g, '\n').trim();
    if (!lyrics) issues.push({ level: 'error', message: 'A sung song needs lyrics.' });
    else controls.push({ setting: 'lyrics', route: 'prompt', detail: `${words(lyrics)} words` });
  }

  // Locking: a chord bed fixes tempo, key and meter in the score.
  let chordBed: CompiledPrompt['chordBed'] = null;
  if (prompt.lockToScore) {
    if (tempo === null || !prompt.key || !keyValid || !meterValid) {
      issues.push({ level: 'error', message: 'Lock to score needs a tempo, a key and a meter.' });
    } else {
      chordBed = { bpm: tempo, key: bedKey(prompt.key), meter: prompt.meter, lyrics };
      controls.push({ setting: 'key', route: 'score', detail: keyName(prompt.key) });
      controls.push({ setting: 'meter', route: 'score', detail: prompt.meter });
      issues.push({ level: 'note', message: 'Locked: the studio writes chords over rests at this tempo, key and meter and the model renders that score without planning its own melody.' });
    }
  }

  let durationSeconds: number | null = null;
  if (prompt.durationSeconds !== null) {
    if (prompt.durationSeconds > 0 && Number.isFinite(prompt.durationSeconds)) {
      durationSeconds = Math.round(prompt.durationSeconds);
      controls.push({ setting: 'duration', route: 'request', detail: `${durationSeconds} s target; the model may end earlier` });
    } else {
      issues.push({ level: 'error', message: 'Duration must be a positive number of seconds.' });
    }
  }

  const style = parts.join(', ');
  const count = words(style);
  if (count > STYLE_WORDS[1]) issues.push({ level: 'warning', message: `The style is ${count} words; the studio's guide keeps it to ${STYLE_WORDS[1]}. Nothing was cut: shorten it if the model drifts.` });
  if (count < STYLE_WORDS[0]) issues.push({ level: 'note', message: `The style is ${count} words; ${STYLE_WORDS[0]}-${STYLE_WORDS[1]} gives the model more to go on.` });

  return { style, lyrics, cot: 'full', chordBed, durationSeconds, words: count, controls, issues };
}

export interface KineticPreset {
  id: string;
  name: string;
  /** Built in presets can be edited and saved as one's own, not overwritten. */
  builtIn: boolean;
  prompt: KineticPrompt;
}

const preset = (id: string, name: string, fields: Partial<Omit<KineticPrompt, 'instruments'>> & { instruments: Partial<Record<InstrumentGroup, string[]>> }): KineticPreset => {
  const base = emptyPrompt('instrumental');
  return { id, name, builtIn: true, prompt: { ...base, ...fields, instruments: { ...base.instruments, ...fields.instruments } } };
};

/**
 * Starting points for instrumental electronic music. They describe a sound to
 * aim for; the model is not promised to reproduce any particular synthesizer.
 */
export const KINETIC_PRESETS: KineticPreset[] = [
  preset('dark-cinematic-ambient', 'Dark Cinematic Ambient', {
    genre: 'dark cinematic ambient',
    mood: ['brooding', 'vast', 'slowly unfolding'],
    instruments: {
      bass: ['deep analog drone bass'],
      synths: ['evolving analog pads'],
      textures: ['granular atmospheric textures'],
      melodic: ['restrained sparse synth melody'],
    },
    arrangement: ['slow swells'],
    production: ['wide stereo field', 'long dark reverb', 'soft tape saturation'],
    sections: ['Intro', 'Verse', 'Bridge', 'Chorus', 'Outro'],
    tempo: 70,
    key: { tonic: 'D', minor: true },
    exclusions: ['drums'],
  }),
  preset('experimental-trip-hop', 'Experimental Trip-Hop', {
    genre: 'experimental trip-hop',
    influences: ['1990s Bristol sound'],
    mood: ['hypnotic', 'smoky', 'uneasy'],
    instruments: {
      bass: ['heavy analog sub bass'],
      percussion: ['dusty sampled breakbeats', 'vinyl crackle'],
      textures: ['detuned tape textures'],
      melodic: ['eerie Rhodes chords'],
    },
    arrangement: ['looping hypnotic groove'],
    production: ['lo-fi saturated mix', 'spring reverb', 'mono-heavy low end'],
    sections: ['Intro', 'Verse', 'Chorus', 'Bridge', 'Chorus', 'Outro'],
    tempo: 86,
    key: { tonic: 'F', minor: true },
  }),
  preset('analog-electronica', 'Analog Electronica', {
    genre: 'analog electronica',
    mood: ['driving', 'bright', 'euphoric'],
    instruments: {
      bass: ['sequenced analog bassline'],
      synths: ['evolving arpeggiators', 'detuned polysynth chords'],
      percussion: ['crisp drum machine'],
      melodic: ['expressive modulated lead synth'],
    },
    arrangement: ['steady build with filter sweeps'],
    production: ['warm analog saturation', 'tempo-synced delay', 'punchy compression'],
    sections: ['Intro', 'Verse', 'Build', 'Drop', 'Breakdown', 'Drop', 'Outro'],
    tempo: 118,
    key: { tonic: 'A', minor: true },
  }),
  preset('minimal-synth-soundtrack', 'Minimal Synth Soundtrack', {
    genre: 'minimal synth film score',
    mood: ['tense', 'cold', 'cinematic'],
    instruments: {
      bass: ['pulsing analog bass'],
      synths: ['sparse synth ostinato'],
      textures: ['low string-like synth pads'],
    },
    harmony: 'restrained slowly shifting harmony',
    arrangement: ['sparse arrangement', 'gradual tension'],
    production: ['dry close mix', 'subtle hall reverb'],
    sections: ['Intro', 'Verse', 'Bridge', 'Verse', 'Outro'],
    tempo: 96,
    key: { tonic: 'E', minor: true },
  }),
  preset('deep-electronic-downtempo', 'Deep Electronic Downtempo', {
    genre: 'deep electronic downtempo',
    mood: ['warm', 'nocturnal', 'meditative'],
    instruments: {
      bass: ['saturated round bass'],
      synths: ['atmospheric analog pads'],
      percussion: ['detailed shuffled percussion', 'soft kick'],
      melodic: ['muted plucked synth motif'],
    },
    arrangement: ['slow groove with subtle variations'],
    production: ['deep low end', 'wide stereo pads', 'gentle tape saturation'],
    sections: ['Intro', 'Verse', 'Chorus', 'Interlude', 'Chorus', 'Outro'],
    tempo: 100,
    key: { tonic: 'G', minor: true },
  }),
];

/** A copy of a preset's prompt, so editing it never changes the preset. */
export function promptFrom(source: KineticPreset): KineticPrompt {
  return JSON.parse(JSON.stringify(source.prompt)) as KineticPrompt;
}

/** A user preset read from storage, with anything missing filled in and anything malformed refused. */
export function readPreset(value: unknown): KineticPreset | null {
  if (!value || typeof value !== 'object') return null;
  const raw = value as Record<string, unknown>;
  if (typeof raw.id !== 'string' || typeof raw.name !== 'string' || !raw.prompt || typeof raw.prompt !== 'object') return null;
  const base = emptyPrompt();
  const stored = raw.prompt as Partial<KineticPrompt>;
  const strings = (list: unknown) => (Array.isArray(list) ? list.filter((item): item is string => typeof item === 'string') : []);
  const instruments = { ...base.instruments };
  for (const group of INSTRUMENT_GROUPS) instruments[group] = strings((stored.instruments as Record<string, unknown> | undefined)?.[group]);
  return {
    id: raw.id,
    name: raw.name,
    builtIn: false,
    prompt: {
      ...base,
      mode: stored.mode === 'vocal' ? 'vocal' : 'instrumental',
      genre: typeof stored.genre === 'string' ? stored.genre : '',
      influences: strings(stored.influences),
      mood: strings(stored.mood),
      direction: typeof stored.direction === 'string' ? stored.direction : '',
      instruments,
      sections: strings(stored.sections).length ? strings(stored.sections) : base.sections,
      arrangement: strings(stored.arrangement),
      production: strings(stored.production),
      tempo: typeof stored.tempo === 'number' ? stored.tempo : null,
      key: stored.key && typeof stored.key.tonic === 'string' ? { tonic: stored.key.tonic, minor: Boolean(stored.key.minor) } : null,
      meter: typeof stored.meter === 'string' ? stored.meter : '4/4',
      harmony: typeof stored.harmony === 'string' ? stored.harmony : '',
      energy: typeof stored.energy === 'string' ? stored.energy : '',
      lockToScore: Boolean(stored.lockToScore),
      durationSeconds: typeof stored.durationSeconds === 'number' ? stored.durationSeconds : null,
      exclusions: strings(stored.exclusions),
      language: typeof stored.language === 'string' ? stored.language : 'English',
      voice: typeof stored.voice === 'string' ? stored.voice : '',
      lyrics: typeof stored.lyrics === 'string' ? stored.lyrics : '',
    },
  };
}

export async function loadUserPresets(): Promise<KineticPreset[]> {
  const response = await fetch('/v1/kinetic/presets');
  if (!response.ok) throw new Error(`presets: HTTP ${response.status}`);
  const body = (await response.json()) as { presets?: unknown[] };
  return (body.presets ?? []).map(readPreset).filter((entry): entry is KineticPreset => entry !== null);
}

export async function saveUserPresets(presets: KineticPreset[]): Promise<void> {
  const response = await fetch('/v1/kinetic/presets', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ presets: presets.filter(entry => !entry.builtIn).map(({ id, name, prompt }) => ({ id, name, prompt })) }),
  });
  if (!response.ok) throw new Error(`save presets: HTTP ${response.status}`);
}
