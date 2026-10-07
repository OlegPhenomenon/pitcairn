import { test, expect } from '@playwright/test';
import { loginSeed } from '../helpers/ui.js';

test.use({ viewport: { width: 360, height: 740 }, isMobile: true, hasTouch: true });

test('dashboard, project, and catalog fit a narrow viewport; drawer opens with keyboard', async ({ page }) => {
  await loginSeed(page);
  await expect(page.getByRole('heading', { name: /Welcome/ })).toBeVisible();
  const fits = async () => expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(360);
  await fits();
  const menu = page.getByRole('button', { name: 'Open navigation menu' });
  await menu.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Navigation menu' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'Navigation menu' })).toBeHidden();
  await page.getByRole('link', { name: 'Coral health around Pitcairn' }).first().click();
  await expect(page.getByRole('heading', { name: 'Coral health around Pitcairn' })).toBeVisible();
  await fits();
  await page.goto('/catalog');
  await expect(page.getByRole('heading', { name: 'Open catalog' })).toBeVisible();
  await fits();
});
