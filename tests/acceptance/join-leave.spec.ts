import { test, expect } from './fixtures';

test.describe('Join/Leave Room', () => {
  test('shows a join button when not in the room', async ({ page }) => {
    await page.goto('/');
    const joinButton = page.locator('[data-testid="join-room-button"]');
    await expect(joinButton).toBeVisible();
    await expect(joinButton).toHaveText('Join Room');
  });

  test('shows a leave button after joining the room', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    const leaveButton = page.locator('[data-testid="leave-room-button"]');
    await expect(leaveButton).toBeVisible();
    await expect(leaveButton).toHaveText('Leave Room');
  });

  test('user appears in the room after joining', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    const roomSection = page.locator('[data-testid="room"]');
    await expect(roomSection).toContainText('You');
  });

  test('user is removed from room after leaving', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="join-room-button"]').click();
    await page.locator('[data-testid="leave-room-button"]').click();
    const roomSection = page.locator('[data-testid="room"]');
    await expect(roomSection).toContainText('No one is in the room');
    await expect(page.locator('[data-testid="join-room-button"]')).toBeVisible();
  });
});
