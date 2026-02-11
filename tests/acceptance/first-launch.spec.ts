import { test, expect } from '@playwright/test';

test.describe('First Launch Setup', () => {
  test('shows setup screen when no token is configured', async ({ page }) => {
    await page.goto('/');
    const setupScreen = page.locator('[data-testid="setup-screen"]');
    await expect(setupScreen).toBeVisible();
    await expect(setupScreen).toContainText('Welcome to Gezellig');
  });

  test('setup screen has fields for server URL and token', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('[data-testid="setup-livekit-url"]')).toBeVisible();
    await expect(page.locator('[data-testid="setup-token"]')).toBeVisible();
  });

  test('setup screen has a connect button', async ({ page }) => {
    await page.goto('/');
    const connectButton = page.locator('[data-testid="setup-connect"]');
    await expect(connectButton).toBeVisible();
    await expect(connectButton).toHaveText('Connect');
  });

  test('completing setup shows the main app', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="setup-livekit-url"]').fill('wss://test.livekit.cloud');
    const header = btoa(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
    const payload = btoa(JSON.stringify({ sub: 'Alice', name: 'Alice' }));
    await page.locator('[data-testid="setup-token"]').fill(`${header}.${payload}.sig`);
    await page.locator('[data-testid="setup-connect"]').click();
    await expect(page.locator('[data-testid="room"]')).toBeVisible();
    await expect(page.locator('[data-testid="setup-screen"]')).not.toBeVisible();
  });

  test('identity from JWT token is used in the app', async ({ page }) => {
    await page.goto('/');
    await page.locator('[data-testid="setup-livekit-url"]').fill('wss://test.livekit.cloud');
    const header = btoa(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
    const payload = btoa(JSON.stringify({ sub: 'Alice', name: 'Alice' }));
    await page.locator('[data-testid="setup-token"]').fill(`${header}.${payload}.sig`);
    await page.locator('[data-testid="setup-connect"]').click();
    await expect(page.locator('[data-testid="room"]')).toBeVisible();
  });

  test('connect button is disabled when fields are empty', async ({ page }) => {
    await page.goto('/');
    const connectButton = page.locator('[data-testid="setup-connect"]');
    await expect(connectButton).toBeDisabled();
  });
});
