import { test, expect } from './fixtures';

test.describe('Notifications', () => {
  test('shows a notification area for events', async ({ page }) => {
    await page.goto('/');
    const notificationArea = page.locator('[data-testid="notification-area"]');
    await expect(notificationArea).toBeVisible();
  });

  test('shows notification when settings are saved', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    await page.locator('[data-testid="settings-save"]').click();
    const notification = page.locator('[data-testid="notification-area"]');
    await expect(notification).toContainText('Settings saved');
  });
});
