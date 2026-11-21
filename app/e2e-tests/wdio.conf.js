import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));

// The embedded WebDriver server is registered only in the feature-gated debug
// build, so drive that binary rather than the shipped release one.
const appBinaryPath = path.resolve(here, '../src-tauri/target/debug/fluxcore.exe');

export const config = {
  runner: 'local',
  specs: [path.join(here, 'specs/**/*.spec.js')],
  maxInstances: 1,

  capabilities: [
    {
      browserName: 'tauri',
      'tauri:options': {
        application: appBinaryPath,
      },
    },
  ],

  // 'embedded' connects to the WebDriver server running inside the feature-gated
  // debug build. No msedgedriver or tauri-driver is needed, which avoids the
  // external driver's version-detection fragility on current WebView2 runtimes.
  services: [
    [
      '@wdio/tauri-service',
      {
        driverProvider: 'embedded',
        startTimeout: 60000,
        captureBackendLogs: true,
        captureFrontendLogs: true,
      },
    ],
  ],

  logLevel: 'info',
  waitforTimeout: 15000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: { ui: 'bdd', timeout: 120000 },
};
