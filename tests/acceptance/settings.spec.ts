import { test, expect } from './fixtures';

test.describe('Settings', () => {
  test('shows settings button', async ({ page }) => {
    await page.goto('/');
    const settingsButton = page.locator('[data-testid="settings-button"]');
    await expect(settingsButton).toBeVisible();
  });

  test('opens settings panel when clicked', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    const settingsPanel = page.locator('[data-testid="settings-panel"]');
    await expect(settingsPanel).toBeVisible();
    await expect(settingsPanel).toContainText('Settings');
  });

  test('can close settings panel', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    await page.locator('[data-testid="settings-close"]').click();
    await expect(page.locator('[data-testid="settings-panel"]')).not.toBeVisible();
  });

  test('shows display name field', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    const nameInput = page.locator('[data-testid="display-name-input"]');
    await expect(nameInput).toBeVisible();
  });

  test('shows LiveKit server URL field', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    const urlInput = page.locator('[data-testid="livekit-url-input"]');
    await expect(urlInput).toBeVisible();
  });

  test('display name is used in online users list', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    const nameInput = page.locator('[data-testid="display-name-input"]');
    await nameInput.fill('Alice');
    await page.locator('[data-testid="settings-close"]').click();
    await expect(page.locator('[data-testid="online-users"]')).toContainText('Alice');
  });

  test('display name is used when joining the room', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    await page.locator('[data-testid="display-name-input"]').fill('Bob');
    await page.locator('[data-testid="settings-close"]').click();
    await page.locator('[data-testid="join-room-button"]').click();
    await expect(page.locator('[data-testid="room"]')).toContainText('Bob');
  });

  test('has a save button that confirms settings are saved', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="settings-button"]').click();
    const saveButton = page.locator('[data-testid="settings-save"]');
    await expect(saveButton).toBeVisible();
    await expect(saveButton).toHaveText('Save');
  });
});
