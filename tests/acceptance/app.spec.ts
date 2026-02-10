import { test, expect } from '@playwright/test';

test('app loads and shows the main page', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle(/gezellig/i);
});
