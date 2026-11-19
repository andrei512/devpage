import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execution } from './execute-ci.mjs';

const brief = {files: ['eval_discussion_breif.md'], full: false, suites: [], smoke: false, filters: []};
const video = {files: ['assets/video.js'], full: false, suites: ['browser-video'], smoke: false, filters: ['browser_tests::video::']};
const unknown = {files: ['new/engine.rs'], full: true, suites: ['browser-video'], smoke: true, filters: []};

test('affected mode keeps zero, focused and fallback execution distinct', () => {
  assert.equal(execution(brief, 'affected', 'pull_request').rust, null);
  assert.equal(execution(brief, 'affected', 'push').smoke, false);
  assert.ok(execution(video, 'affected', 'pull_request', 'win32').rust.includes('--skip'));
  assert.ok(execution(video, 'affected', 'pull_request').rust.includes('browser_tests::video::'));
  assert.equal(execution(video, 'affected', 'pull_request').smoke, false);
  assert.equal(execution(unknown, 'affected', 'pull_request').smoke, true);
});

test('shadow preserves existing checks and exposes configuration errors', () => {
  assert.ok(execution(brief, '', 'pull_request').rust.includes('--ignored'));
  assert.equal(execution(brief, 'shadow', 'pull_request').smoke, false);
  assert.equal(execution(video, 'shadow', 'pull_request').smoke, true);
  assert.equal(execution(brief, 'shadow', 'push').smoke, true);
  assert.throws(() => execution(brief, 'affcted', 'pull_request'));
});
