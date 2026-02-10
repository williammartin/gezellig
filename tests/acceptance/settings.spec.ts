import { test, expect } from '@playwright/test';

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
});
