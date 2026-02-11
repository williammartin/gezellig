import { test, expect } from './fixtures';

test.describe('Audio Controls', () => {
  test('shows mute button when in the room', async ({ page }) => {
    await page.goto('/');
    const muteButton = page.locator('[data-testid="mute-button"]');
    await expect(muteButton).toBeVisible();
    await expect(muteButton).toContainText('Mute');
  });

  test('toggles to unmute when muted', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="mute-button"]').click();
    const unmuteButton = page.locator('[data-testid="mute-button"]');
    await expect(unmuteButton).toContainText('Unmute');
  });

  test('does not show mute button when not in room', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="mute-button"]')).not.toBeVisible();
  });
});
