import { test, expect } from './fixtures';

test('app loads and shows the main page', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle(/gezellig/i);
});
