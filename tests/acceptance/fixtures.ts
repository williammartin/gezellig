import { test as base } from '@playwright/test';

/**
 * Extends the base test with a pre-configured setup so that tests
 * bypass the first-launch screen and land directly on the main app.
 */
export const test = base.extend({
  page: async ({ page }, use) => {
    // Navigate first to set the origin for localStorage
    await page.goto('/');
    await page.evaluate(() => {
      localStorage.setItem('gezellig-setup', JSON.stringify({
        displayName: 'You',
        livekitUrl: 'wss://test.livekit.cloud',
        livekitToken: 'test-token',
      }));
    });
    // Reload so the app picks up the saved setup
    await page.reload();
    await use(page);
  },
});

export { expect } from '@playwright/test';
