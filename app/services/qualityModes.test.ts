import { describe, expect, it } from 'vitest';
import { MODE_SETTINGS, acousticPasses, modeOf, renderModeOf } from './qualityModes';

describe('quality modes', () => {
  it('reads the engine defaults as Studio Quality', () => {
    expect(modeOf('', '')).toBe('studio');
    expect(modeOf('32', 'midpoint')).toBe('studio');
  });

  it('reads its own settings back as each mode', () => {
    expect(modeOf(MODE_SETTINGS.studio.steps, MODE_SETTINGS.studio.solver)).toBe('studio');
    expect(modeOf(MODE_SETTINGS.draft.steps, MODE_SETTINGS.draft.solver)).toBe('draft');
  });

  it('calls anything else Custom', () => {
    expect(modeOf('16', 'midpoint')).toBe('custom');
    expect(modeOf('16', 'ab2')).toBe('custom');
    expect(modeOf('48', '')).toBe('custom');
  });

  it('labels a finished song from the settings it keeps', () => {
    expect(renderModeOf({})).toBe('studio');
    expect(renderModeOf({ steps: 32, solver: 'ab2' })).toBe('draft');
    expect(renderModeOf({ steps: 32, solver: 'midpoint' })).toBe('studio');
    expect(renderModeOf(undefined)).toBe('studio');
  });

  it('counts the network passes each solver makes', () => {
    expect(acousticPasses(32, 'midpoint')).toBe(64);
    expect(acousticPasses(32, 'ab2')).toBe(33);
  });
});
