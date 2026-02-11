import { test, expect } from './fixtures';

test.describe('DJ Controls', () => {
  test('shows become DJ button when in the room', async ({ page }) => {
    await page.goto('/');
    const djButton = page.locator('[data-testid="become-dj-button"]');
    await expect(djButton).toBeVisible();
    await expect(djButton).toContainText('Become DJ');
  });

  test('shows DJ status after becoming DJ', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    const djStatus = page.locator('[data-testid="dj-status"]');
    await expect(djStatus).toBeVisible();
    await expect(djStatus).toContainText('You are the DJ');
  });

  test('can stop being DJ', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    await page.locator('[data-testid="stop-dj-button"]').click();
    await expect(page.locator('[data-testid="become-dj-button"]')).toBeVisible();
    await expect(page.locator('[data-testid="dj-status"]')).not.toBeVisible();
  });

  test('does not show DJ controls when not in room', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="become-dj-button"]')).not.toBeVisible();
  });

  test('shows prompt to add URL when DJ is active', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    const nowPlaying = page.locator('[data-testid="now-playing"]');
    await expect(nowPlaying).toBeVisible();
    await expect(nowPlaying).toContainText('Add a YouTube URL');
  });

  test('shows music volume control when DJ is active', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    const volumeControl = page.locator('[data-testid="music-volume"]');
    await expect(volumeControl).toBeVisible();
  });

  test('shows queue URL input when DJ is active', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    await expect(page.locator('[data-testid="queue-url-input"]')).toBeVisible();
    await expect(page.locator('[data-testid="add-to-queue-button"]')).toBeVisible();
  });

  test('can add URL to queue', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    await page.locator('[data-testid="queue-url-input"]').fill('https://youtube.com/watch?v=test');
    await page.locator('[data-testid="add-to-queue-button"]').click();
    await expect(page.locator('[data-testid="dj-queue"]')).toBeVisible();
  });

  test('DJ controls are hidden after stopping DJ', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    await expect(page.locator('[data-testid="dj-status"]')).toBeVisible();
    await page.locator('[data-testid="stop-dj-button"]').click();
    await expect(page.locator('[data-testid="dj-status"]')).not.toBeVisible();
    await expect(page.locator('[data-testid="now-playing"]')).not.toBeVisible();
    await expect(page.locator('[data-testid="music-volume"]')).not.toBeVisible();
  });
});
