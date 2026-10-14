/**
 * The two ways Kinetic Studio renders a new song.
 *
 * Studio Quality is the reference render the model's authors use: 32 midpoint
 * flow-matching steps, no length ceiling (the model ends the song, up to 6:00),
 * the official instrumental workflow and the native stereo master. Nothing else
 * runs after it: stems, MIDI and Ableton are made only when asked for.
 *
 * Fast Draft changes only the acoustic stage, which is where the time goes on
 * Apple Silicon (86% of a measured 5:12 song on an M1 Pro). The composition
 * (score and semantic stream) is written exactly as in Studio Quality, so a
 * draft can be re-rendered in Studio Quality from its own semantic tokens: the
 * same composition and the same noise seed, rendered at the reference steps.
 * That re-render is not a new composition, and it is not bit-identical to what
 * a Studio Quality generation with the same prompt would have drawn.
 *
 * The draft setting is the engine's second-order Adams-Bashforth solver at
 * the reference 32 steps: 33 network passes instead of 64. Its authors measured
 * it against midpoint at 16 steps (mel spectra correlated at 0.9996); it has not
 * yet been listened to against midpoint at 32 steps on Jared's Mac, and
 * scripts/quality/bench_speed.py is the benchmark that settles it.
 */

export type QualityMode = 'studio' | 'draft' | 'custom';

export interface ModeSettings {
  /** Flow-matching steps, '' for the engine default (32). */
  steps: string;
  /** '' for the engine default (midpoint). */
  solver: '' | 'midpoint' | 'ab2';
}

export const REFERENCE_STEPS = 32;

export const MODE_SETTINGS: Record<Exclude<QualityMode, 'custom'>, ModeSettings> = {
  studio: { steps: '', solver: '' },
  draft: { steps: String(REFERENCE_STEPS), solver: 'ab2' },
};

/** Network passes of the acoustic stage: midpoint evaluates twice a step, AB2 once after its first. */
export function acousticPasses(steps: number, solver: 'midpoint' | 'ab2'): number {
  return solver === 'ab2' ? steps + 1 : steps * 2;
}

const stepsOf = (steps: string) => (steps.trim() === '' ? REFERENCE_STEPS : Number(steps));
const solverOf = (solver: string) => (solver === 'ab2' ? 'ab2' : 'midpoint');

/** Which mode the Quality fields amount to; anything set by hand is Custom. */
export function modeOf(steps: string, solver: string): QualityMode {
  const count = stepsOf(steps);
  const method = solverOf(solver);
  if (count === REFERENCE_STEPS && method === 'midpoint') return 'studio';
  const draft = MODE_SETTINGS.draft;
  if (count === stepsOf(draft.steps) && method === solverOf(draft.solver)) return 'draft';
  return 'custom';
}

/** The mode a finished song was rendered in, from the settings it keeps. */
export function renderModeOf(settings: Record<string, unknown> | undefined): QualityMode {
  const steps = typeof settings?.steps === 'number' ? String(settings.steps) : '';
  const solver = typeof settings?.solver === 'string' ? settings.solver : '';
  return modeOf(steps, solver);
}

export const MODE_LABEL: Record<QualityMode, string> = {
  studio: 'Studio Quality',
  draft: 'Fast Draft',
  custom: 'Custom',
};

export const MODE_HINT: Record<QualityMode, string> = {
  studio: 'Reference render: 32 midpoint steps (64 network passes), no length ceiling, native stereo master. Nothing else runs until you ask.',
  draft:
    'Same composition stages; the acoustic stage uses the AB2 solver, 33 network passes instead of 64, so it takes about half as long. ' +
    'Expect small differences in fine detail; not yet listening-tested against Studio Quality on this Mac. Keep a draft? Re-render it in Studio Quality from its tokens.',
  custom: 'Steps or solver set by hand.',
};
