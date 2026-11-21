import { browser, $, expect } from '@wdio/globals';

// App.tsx renders each sidebar entry as:
//   <button className={`rx-nav-item ${page === id ? 'active' : ''}`}>
// so the `active` class is the authoritative signal that navigation happened.
const navItem = (label) =>
  $(`//button[contains(@class,'rx-nav-item')][.//span[normalize-space()='${label}']]`);

const navigateTo = async (label) => {
  const item = navItem(label);
  await item.waitForExist({ timeout: 20000 });
  await item.click();
  await browser.waitUntil(
    async () => ((await item.getAttribute('class')) || '').split(/\s+/).includes('active'),
    { timeout: 20000, timeoutMsg: `sidebar item "${label}" never became active` },
  );
};

describe('FluxCore native UI', () => {
  it('starts with the live collector online', async () => {
    await browser.waitUntil(
      async () =>
        (await $('[data-testid="collector-state"]').getText()).includes(
          'Monitoring active',
        ),
      { timeout: 60000, timeoutMsg: 'the collector never came online' },
    );
    expect(await $('//h1').isExisting()).toBe(true);
  });

  it('navigates to Resource Intelligence through the sidebar', async () => {
    await navigateTo('Resource Intelligence');
  });

  it('navigates to Settings and flips the monitoring switch both ways', async () => {
    await navigateTo('Settings');
    expect(
      await $('//h2[normalize-space()="Monitoring"]').isExisting(),
    ).toBe(true);

    const toggle = await $(
      '//div[contains(@class,"rx-setting-row")]' +
        '[.//h3[normalize-space()="Enable monitoring"]]' +
        '//button[@role="switch"]',
    );
    expect(await toggle.isExisting()).toBe(true);
    expect(await toggle.getAttribute('aria-checked')).toBe('true');

    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-checked')) === 'false',
      { timeout: 10000, timeoutMsg: 'the monitoring switch did not turn off' },
    );
    await browser.waitUntil(
      async () =>
        (await $('[data-testid="collector-state"]').getText()).includes(
          'Monitoring paused',
        ),
      { timeout: 10000, timeoutMsg: 'the topbar did not reflect the pause' },
    );

    // Restore the original state so the app is left as it was found.
    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-checked')) === 'true',
      { timeout: 10000, timeoutMsg: 'the monitoring switch did not turn back on' },
    );
  });
});
