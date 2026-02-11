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

  test('shows now playing panel and skip button', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="now-playing"]')).toBeVisible();
    await expect(page.locator('[data-testid="skip-track-button"]')).toBeVisible();
  });

  test('can add URL to queue', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="queue-url-input"]').fill('https://youtube.com/watch?v=test');
    await page.locator('[data-testid="add-to-queue-button"]').click();
    await expect(page.locator('[data-testid="dj-queue"]')).toBeVisible();
  });

  test('shows clear queue button', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="clear-queue-button"]')).toBeVisible();
  });

  test('can clear queue locally', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="queue-url-input"]').fill('https://youtube.com/watch?v=test');
    await page.locator('[data-testid="add-to-queue-button"]').click();
    await page.locator('[data-testid="clear-queue-button"]').click();
    await expect(page.locator('[data-testid="dj-queue"]')).not.toBeVisible();
  });
});
