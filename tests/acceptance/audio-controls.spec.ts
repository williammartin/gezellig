import { test, expect } from './fixtures';

test.describe('Audio Controls', () => {
  test('shows music volume control on launch', async ({ page }) => {
    await page.goto('/');
    const volumeControl = page.locator('[data-testid="music-volume"]');
    await expect(volumeControl).toBeVisible();
  });

  test('can adjust music volume slider', async ({ page }) => {
    await page.goto('/');
    const volumeControl = page.locator('[data-testid="music-volume"]');
    await volumeControl.evaluate((el) => {
      (el as HTMLInputElement).value = '25';
      el.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await expect(volumeControl).toHaveValue('25');
  });
});
