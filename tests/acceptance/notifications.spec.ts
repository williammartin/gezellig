import { test, expect } from '@playwright/test';

test.describe('Notifications', () => {
  test('shows a notification area for events', async ({ page }) => {
    await page.goto('/');
    const notificationArea = page.locator('[data-testid="notification-area"]');
    await expect(notificationArea).toBeVisible();
  });

  test('shows notification when joining the room', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    const notification = page.locator('[data-testid="notification-area"]');
    await expect(notification).toContainText('joined the room');
  });

  test('shows notification when leaving the room', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    await page.locator('[data-testid="leave-room-button"]').click();
    const notification = page.locator('[data-testid="notification-area"]');
    await expect(notification).toContainText('left the room');
  });

  test('shows notification when becoming DJ', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    await page.locator('[data-testid="become-dj-button"]').click();
    const notification = page.locator('[data-testid="notification-area"]');
    await expect(notification).toContainText('is now the DJ');
  });
});
