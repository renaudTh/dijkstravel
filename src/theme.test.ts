import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import { runThemeName } from './run.ts';
import { DEFAULT_THEME, parseTheme } from './theme.ts';

describe('parseTheme', () => {
  test('fills the settings a theme file leaves out with the defaults', () => {
    const theme = parseTheme({ route: { color: '#000000' } });

    assert.deepEqual(theme, {
      ok: true,
      value: { ...DEFAULT_THEME, route: { ...DEFAULT_THEME.route, color: '#000000' } },
    });
  });

  test('rejects a color that is not #rrggbb, citing its path', () => {
    const theme = parseTheme({ hud: { background: 'violet' } });

    assert.deepEqual(theme.ok ? [] : theme.errors.map((issue) => issue.path), ['hud.background']);
  });

  test('rejects a number outside its range, citing its path', () => {
    const theme = parseTheme({ route: { width: 400 } });

    assert.deepEqual(theme.ok ? [] : theme.errors.map((issue) => issue.path), ['route.width']);
  });

  test('rejects a font the editor does not offer', () => {
    const theme = parseTheme({ hud: { font: 'Comic Sans MS' } });

    assert.equal(theme.ok, false);
  });
});

describe('runThemeName', () => {
  test('uses the default theme when the run names none', () => {
    assert.equal(runThemeName({ name: 'x' }), 'default');
  });

  test('rejects a theme name that could leave the themes folder', () => {
    assert.equal(runThemeName({ theme: '../secrets' }), null);
  });
});
