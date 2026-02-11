import { test, expect } from './fixtures';

test.describe('Notifications', () => {
  test('shows a notification area for events', async ({ page }) => {
    await page.goto('/');
    const notificationArea = page.locator('[data-testid="notification-area"]');
    await expect(notificationArea).toBeVisible();
  });

  test('shows notification when becoming DJ', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="become-dj-button"]').click();
    const notification = page.locator('[data-testid="notification-area"]');
    await expect(notification).toContainText('You are now the DJ');
  });
});
