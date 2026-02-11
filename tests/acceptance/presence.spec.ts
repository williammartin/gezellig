import { test, expect } from './fixtures';

test.describe('Presence', () => {
  test('shows the room section when the app loads', async ({ page }) => {
    await page.goto('/');
    const roomSection = page.locator('[data-testid="room"]');
    await expect(roomSection).toBeVisible();
    await expect(roomSection).toContainText('Room');
  });
});
