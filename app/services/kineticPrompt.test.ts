import { describe, expect, it } from 'vitest';
import { compilePrompt, emptyPrompt, KINETIC_PRESETS, promptFrom, readPreset, sectionTag, type KineticPrompt } from './kineticPrompt';

const prompt = (fields: Partial<KineticPrompt>): KineticPrompt => ({ ...emptyPrompt('instrumental'), ...fields });

describe('the Kinetic prompt as YuE2 reads it', () => {
  it('writes the style in the contract order with the tempo last', () => {
    const compiled = compilePrompt(
      prompt({
        genre: 'trip-hop',
        mood: ['smoky'],
        instruments: { bass: ['sub bass'], synths: [], percussion: ['breakbeats'], textures: [], melodic: [], support: [] },
        production: ['lo-fi mix'],
        tempo: 86,
        key: { tonic: 'F', minor: true },
      }),
    );
    expect(compiled.style).toBe('Instrumental, trip-hop, sub bass, breakbeats, smoky, lo-fi mix, F minor, no vocals, no singing, no choir, no spoken words, 86 BPM');
    expect(compiled.cot).toBe('full');
    expect(compiled.issues.filter(issue => issue.level === 'error')).toEqual([]);
  });

  it('adds the instrumental exclusions once and marks every exclusion best-effort', () => {
    const compiled = compilePrompt(prompt({ genre: 'ambient', exclusions: ['No vocals', 'guitar'] }));
    expect(compiled.style).toBe('Instrumental, ambient, No vocals, no guitar, no singing, no choir, no spoken words');
    const exclusions = compiled.controls.find(control => control.setting === 'exclusions');
    expect(exclusions?.detail).toContain('best-effort');
  });

  it('refuses an exclusion that contradicts an included instrument', () => {
    const compiled = compilePrompt(
      prompt({ genre: 'downtempo', instruments: { bass: [], synths: [], percussion: ['soft drums'], textures: [], melodic: [], support: [] }, exclusions: ['drums'] }),
    );
    expect(compiled.issues).toContainEqual({ level: 'error', message: '"no drums" contradicts "soft drums". Remove one of them.' });
  });

  it('writes a repeated descriptor once and says so', () => {
    const compiled = compilePrompt(prompt({ genre: 'ambient', mood: ['Dark', 'dark'], production: ['dark'] }));
    expect(compiled.style.match(/dark/gi)).toHaveLength(1);
    expect(compiled.issues.filter(issue => issue.level === 'note' && issue.message.includes('written once'))).toHaveLength(2);
  });

  it('leaves out a voice and lyrics in instrumental mode and says why', () => {
    const compiled = compilePrompt(prompt({ genre: 'ambient', voice: 'airy female voice', lyrics: 'hello' }));
    expect(compiled.style).not.toContain('airy');
    expect(compiled.lyrics).not.toContain('hello');
    expect(compiled.controls.filter(control => control.route === 'omitted').map(control => control.setting)).toEqual(['voice', 'lyrics']);
    expect(compiled.issues.filter(issue => issue.level === 'warning').length).toBeGreaterThanOrEqual(2);
  });

  it('keeps a sung song\'s voice, lyrics and language and leaves its key out of the style', () => {
    const compiled = compilePrompt({ ...emptyPrompt('vocal'), genre: 'synth-pop', voice: 'breathy male voice', lyrics: '[Verse]\r\nline one', key: { tonic: 'C', minor: false }, tempo: 120 });
    expect(compiled.style).toBe('English, synth-pop, breathy male voice, 120 BPM');
    expect(compiled.lyrics).toBe('[Verse]\nline one');
    expect(compiled.controls).toContainEqual({ setting: 'key', route: 'omitted', detail: "A sung song's style carries no key; lock to the score to fix it." });
  });

  it('turns lock to score into a chord bed request', () => {
    const compiled = compilePrompt(prompt({ genre: 'ambient', tempo: 70, key: { tonic: 'D', minor: true }, meter: '3/4', lockToScore: true }));
    expect(compiled.chordBed).toEqual({ bpm: 70, key: 'Dm', meter: '3/4', lyrics: compiled.lyrics });
    const routes = Object.fromEntries(compiled.controls.filter(control => ['tempo', 'meter'].includes(control.setting)).map(control => [control.setting, control.route]));
    expect(routes).toEqual({ tempo: 'score', meter: 'score' });
    expect(compiled.controls).toContainEqual({ setting: 'key', route: 'score', detail: 'D minor' });
  });

  it('refuses a lock without tempo and key, and parameters the chord bed cannot write', () => {
    expect(compilePrompt(prompt({ genre: 'x', lockToScore: true })).issues.some(issue => issue.level === 'error' && issue.message.startsWith('Lock to score'))).toBe(true);
    const errors = compilePrompt(prompt({ genre: 'x', tempo: 300, key: { tonic: 'H', minor: false }, meter: '7/8' })).issues.filter(issue => issue.level === 'error');
    expect(errors.map(issue => issue.message)).toEqual([
      '"H" is not a key the chord bed can write.',
      'Meter 7/8 is not one the chord bed writes (4/4, 3/4, 6/8, 2/4).',
      'Tempo 300 is outside 40-200 BPM.',
    ]);
  });

  it('says that an odd meter needs the score when it is not locked', () => {
    const compiled = compilePrompt(prompt({ genre: 'x', meter: '6/8' }));
    expect(compiled.controls).toContainEqual({ setting: 'meter', route: 'omitted', detail: '6/8 needs the score: turn on lock to score.' });
  });

  it('maps producer section names to YuE2 tags and numbers repeats', () => {
    expect(sectionTag('Drop')).toBe('Chorus');
    expect(sectionTag('breakdown')).toBe('Bridge');
    expect(sectionTag('Build-up')).toBe('Pre-Chorus');
    expect(sectionTag('Verse 2')).toBe('Verse');
    expect(sectionTag('wobble')).toBeNull();
    const compiled = compilePrompt(prompt({ genre: 'x', sections: ['Intro', 'Groove', 'Drop', 'Breakdown', 'Drop', 'wobble', 'Outro'] }));
    expect(compiled.lyrics).toBe('[Intro]\n\n[Verse 1]\n\n[Chorus 1]\n\n[Bridge]\n\n[Chorus 2]\n\n[Verse 2]\n\n[Outro]');
    expect(compiled.issues).toContainEqual({ level: 'warning', message: 'Section "wobble" has no YuE2 tag; it is sent as [Verse].' });
    expect(compiled.issues).toContainEqual({ level: 'note', message: 'Section "Drop" is sent as [Chorus].' });
  });

  it('warns about a long style without cutting anything', () => {
    const many = Array.from({ length: 30 }, (_, index) => `texture number${index}`);
    const compiled = compilePrompt(prompt({ genre: 'ambient', production: many }));
    expect(compiled.words).toBeGreaterThan(50);
    for (const value of many) expect(compiled.style).toContain(value);
    expect(compiled.issues.some(issue => issue.level === 'warning' && issue.message.includes('Nothing was cut'))).toBe(true);
  });

  it('routes the duration to the request', () => {
    const compiled = compilePrompt(prompt({ genre: 'x', durationSeconds: 150.4 }));
    expect(compiled.durationSeconds).toBe(150);
    expect(compiled.controls).toContainEqual({ setting: 'duration', route: 'request', detail: '150 s target; the model may end earlier' });
    expect(compilePrompt(prompt({ genre: 'x', durationSeconds: -1 })).issues.some(issue => issue.level === 'error')).toBe(true);
  });
});

describe('Kinetic presets', () => {
  it('ships the five presets from the brief, each compiling without errors inside the style guide', () => {
    expect(KINETIC_PRESETS.map(entry => entry.name)).toEqual([
      'Dark Cinematic Ambient',
      'Experimental Trip-Hop',
      'Analog Electronica',
      'Minimal Synth Soundtrack',
      'Deep Electronic Downtempo',
    ]);
    for (const entry of KINETIC_PRESETS) {
      const compiled = compilePrompt(entry.prompt);
      expect(compiled.issues.filter(issue => issue.level !== 'note'), entry.name).toEqual([]);
      expect(compiled.words, entry.name).toBeLessThanOrEqual(50);
      expect(compiled.style.endsWith(`${entry.prompt.tempo} BPM`), entry.name).toBe(true);
    }
  });

  it('gives a copy, so editing never changes the preset', () => {
    const copy = promptFrom(KINETIC_PRESETS[0]);
    copy.mood.push('changed');
    expect(KINETIC_PRESETS[0].prompt.mood).not.toContain('changed');
  });

  it('reads a stored preset, filling gaps and dropping what is malformed', () => {
    expect(readPreset(null)).toBeNull();
    expect(readPreset({ id: 1, name: 'x', prompt: {} })).toBeNull();
    const read = readPreset({ id: 'mine', name: 'Mine', builtIn: true, prompt: { genre: 'ambient', mood: ['calm', 3], instruments: { bass: ['sub'] }, tempo: '90', mode: 'other' } });
    expect(read?.builtIn).toBe(false);
    expect(read?.prompt.mode).toBe('instrumental');
    expect(read?.prompt.mood).toEqual(['calm']);
    expect(read?.prompt.instruments.bass).toEqual(['sub']);
    expect(read?.prompt.instruments.synths).toEqual([]);
    expect(read?.prompt.tempo).toBeNull();
    expect(read?.prompt.sections).toEqual(['Intro', 'Verse', 'Chorus', 'Outro']);
  });
});
