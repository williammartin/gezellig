import { test, expect } from './fixtures';

test.describe('Room', () => {
  test('shows the room section on launch', async ({ page }) => {
    await page.goto('/');
    const roomSection = page.locator('[data-testid="room"]');
    await expect(roomSection).toBeVisible();
    await expect(roomSection).toContainText('Room');
  });

  test('shows music volume control on launch', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="music-volume"]')).toBeVisible();
  });

  test('shows become DJ button on launch', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="become-dj-button"]')).toBeVisible();
  });
});
