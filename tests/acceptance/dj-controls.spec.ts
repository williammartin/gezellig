import { test, expect } from './fixtures';

test.describe('Queue', () => {
  test('shows shared queue panel on launch', async ({ page }) => {
    await page.goto('/');
    const queuePanel = page.locator('[data-testid="queue-panel"]');
    await expect(queuePanel).toBeVisible();
    await expect(queuePanel).toContainText('Shared Queue');
  });

  test('shows queue input on launch', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="queue-url-input"]')).toBeVisible();
    await expect(page.locator('[data-testid="add-to-queue-button"]')).toBeVisible();
  });

  test('can add URL to queue', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="queue-url-input"]').fill('https://youtube.com/watch?v=test');
    await page.locator('[data-testid="add-to-queue-button"]').click();
    await expect(page.locator('[data-testid="dj-queue"]')).toBeVisible();
  });
});
