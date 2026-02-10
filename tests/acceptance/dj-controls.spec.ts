import { test, expect } from './fixtures';

test.describe('DJ Controls', () => {
  test('shows become DJ button when in the room', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    const djButton = page.locator('[data-testid="become-dj-button"]');
    await expect(djButton).toBeVisible();
    await expect(djButton).toContainText('Become DJ');
  });

  test('shows DJ status after becoming DJ', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    await page.locator('[data-testid="become-dj-button"]').click();
    const djStatus = page.locator('[data-testid="dj-status"]');
    await expect(djStatus).toBeVisible();
    await expect(djStatus).toContainText('You are the DJ');
  });

  test('can stop being DJ', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    await page.locator('[data-testid="become-dj-button"]').click();
    await page.locator('[data-testid="stop-dj-button"]').click();
    await expect(page.locator('[data-testid="become-dj-button"]')).toBeVisible();
    await expect(page.locator('[data-testid="dj-status"]')).not.toBeVisible();
  });

  test('does not show DJ controls when not in room', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="become-dj-button"]')).not.toBeVisible();
  });

  test('shows now playing info when DJ is active', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    await page.locator('[data-testid="become-dj-button"]').click();
    const nowPlaying = page.locator('[data-testid="now-playing"]');
    await expect(nowPlaying).toBeVisible();
    await expect(nowPlaying).toContainText('Waiting for Spotify');
  });

  test('shows music volume control when DJ is active', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    await page.locator('[data-testid="become-dj-button"]').click();
    const volumeControl = page.locator('[data-testid="music-volume"]');
    await expect(volumeControl).toBeVisible();
  });
});
