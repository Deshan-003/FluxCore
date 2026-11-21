import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

// @wdio/tauri-service 1.5.1 detects msedgedriver's version with
//   versionOutput.match(/MSEdgeDriver ([\d.]+)/)
// but current drivers print "Microsoft Edge WebDriver 157.0.4341.46 (...)".
// The service therefore reports "Driver: unknown" and refuses to start even
// when the correct driver is on PATH. Patch the pinned installed copy to accept
// both formats. Remove this once upstream fixes the detection.
const entry = path.join(
  process.cwd(),
  'node_modules',
  '@wdio',
  'tauri-service',
  'dist',
  'esm',
  'index.js',
);

const source = readFileSync(entry, 'utf8');
const from = '/MSEdgeDriver ([\\d.]+)/';
const to = '/(?:Microsoft Edge WebDriver|MSEdgeDriver) ([\\d.]+)/';

if (source.includes(to)) {
  console.log('msedgedriver version detection already patched');
} else if (source.includes(from)) {
  writeFileSync(entry, source.replace(from, to));
  console.log('patched @wdio/tauri-service msedgedriver version detection');
} else {
  console.error(`Expected regex not found in ${entry}; upstream may have changed.`);
  process.exit(1);
}
