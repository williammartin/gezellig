import { test, expect } from './fixtures';

test.describe('Presence', () => {
  test('shows the user as online when the app loads', async ({ page }) => {
    await page.goto('/');
    const onlineSection = page.locator('[data-testid="online-users"]');
    await expect(onlineSection).toBeVisible();
    await expect(onlineSection).toContainText('Online');
  });

  test('shows the room with no one in it initially', async ({ page }) => {
    await page.goto('/');
    const roomSection = page.locator('[data-testid="room"]');
    await expect(roomSection).toBeVisible();
    await expect(roomSection).toContainText('Room');
  });
});
