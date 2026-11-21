import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));

// Point at the built release binary; tauri.conf.json names the product FluxCore
// but Cargo names the executable fluxcore, so this is the raw target path.
const appBinaryPath = path.resolve(here, '../src-tauri/target/release/fluxcore.exe');

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

  // 'external' drives the real app through tauri-driver. It works with the
  // release binary and needs no WebDriver plugin compiled into the app. The
  // service installs tauri-driver and a matching Edge WebDriver automatically.
  services: [
    [
      '@wdio/tauri-service',
      {
        driverProvider: 'external',
        autoInstallTauriDriver: true,
        autoDownloadEdgeDriver: true,
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
