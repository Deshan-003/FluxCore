import { browser, $, expect } from '@wdio/globals';

const heading = () => $('//h1');
const navItem = (label) =>
  $(`//button[contains(@class,'nav-item')][.//span[normalize-space()='${label}']]`);

const waitForHeading = (text) =>
  browser.waitUntil(async () => (await heading().getText()) === text, {
    timeout: 20000,
    timeoutMsg: `page heading never became "${text}"`,
  });

describe('FluxCore native UI', () => {
  it('starts with the live collector online and shows the Overview page', async () => {
    await browser.waitUntil(
      async () =>
        (await $('//span[contains(@class,"collector-state")]').getText()).includes(
          'COLLECTOR ONLINE',
        ),
      { timeout: 60000, timeoutMsg: 'the collector never came online' },
    );

    expect(await heading().getText()).toBe('Overview');
  });

  it('navigates to Resource Intelligence through the sidebar', async () => {
    await navItem('Resource Intelligence').click();
    await waitForHeading('Resource Intelligence');
    expect(await heading().getText()).toBe('Resource Intelligence');
  });

  it('navigates to Settings and flips the monitoring switch both ways', async () => {
    await navItem('Settings').click();
    await waitForHeading('Settings');
    expect(
      await $('//strong[normalize-space()="Monitoring preferences"]').isExisting(),
    ).toBe(true);

    const toggle = await $(
      '//div[contains(@class,"setting-row")]' +
        '[.//strong[normalize-space()="Enable monitoring"]]' +
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
        (await $('//div[contains(@class,"system-pill")]').getText()).includes(
          'Monitoring paused',
        ),
      { timeout: 10000, timeoutMsg: 'the sidebar did not reflect the pause' },
    );

    // Restore the original state so the app is left as it was found.
    await toggle.click();
    await browser.waitUntil(
      async () => (await toggle.getAttribute('aria-checked')) === 'true',
      { timeout: 10000, timeoutMsg: 'the monitoring switch did not turn back on' },
    );
  });
});
