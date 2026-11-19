import assert from 'node:assert/strict';
import { test } from 'node:test';
import { selectFiles } from './run.mjs';

const selections = [
  [['eval_discussion_breif.md', 'dist-workspace.toml'], [], false],
  [['docs/boxes/engineering/code-map/README.md'], [], false],
  [['assets/video.js'], ['video'], false],
  [['src/drive/slide_png.rs'], ['slide-png', 'video'], false],
  [['core/assets/widgets/DevpageSidebar.tsx'], ['sidebar'], false],
  [['docs/talks/devpage/title.md'], ['page-script', 'pdf'], false],
  [['tests/fixtures/sync-design/alpha.md'], ['page-script', 'pdf'], false],
  [['docs/designs/checkout/fonts/Figtree/Figtree-Variable.ttf'], ['canvas', 'page-script', 'pdf'], false],
  [['docs/boxes/viewer/on-the-page/designs/writing-a-design/README.md'], ['canvas'], false],
  [['tests/fixtures/world-service/transports.mjs'], ['world'], false],
  [['assets/video.js', 'deploy/server/Dockerfile'], ['video'], true],
  [['core/assets/widgets/DevpageSidebar.tsx', 'src/drive/slide_png.rs'], ['sidebar', 'slide-png', 'video'], false],
];

for (const [files, expectedSuites, smoke] of selections) {
  test(files.join(' + '), () => {
    const selection = selectFiles(files);
    assert.equal(selection.full, false, selection.reasons.join('\n'));
    assert.deepEqual(selection.suites, expectedSuites.map(name => 'browser-' + name));
    assert.equal(selection.smoke, smoke);
  });
}

test('unclassified production overrides a recognized input and includes the independent PDF smoke', () => {
  const selection = selectFiles(['assets/video.js', 'src/drive/new-renderer.rs']);
  assert.equal(selection.full, true);
  assert.equal(selection.smoke, true);
});

test('shared Chrome harness and toolchain changes cover all registered browser filters', () => {
  const fallback = selectFiles(['src/drive/new-renderer.rs']);
  for (const file of ['src/adapters/chrome.rs', 'Cargo.lock', 'rust-toolchain.toml']) {
    const selection = selectFiles([file]);
    assert.deepEqual([...selection.suites].sort(), [...fallback.suites].sort(), file);
    assert.equal(selection.smoke, true, file);
  }
});
