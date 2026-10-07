import { expect, type Page } from '@playwright/test';

const names: Record<string, string> = { anna: 'Dr Anna Hart', liam: 'Liam Chen', lukas: 'Dr Lukas Weber', maria: 'Maria Ellis', james: 'Dr James Okafor', helen: 'Helen Brooks', sam: 'Sam Torres', ruth: 'Ruth Palmer' };

export async function switchPersona(page: Page, key: keyof typeof names) {
  await page.getByRole('button', { name: 'Switch persona' }).click();
  await page.getByRole('menuitem', { name: new RegExp(names[key]) }).click();
  await expect(page.getByRole('button', { name: new RegExp(names[key]) })).toBeVisible();
}

export async function uploadFile(page: Page, filename: string, body: string, mimeType = 'text/plain') {
  const scope = await page.getByRole('dialog').count() ? page.getByRole('dialog').last() : page;
  await scope.getByLabel('Choose file to upload').setInputFiles({ name: filename, mimeType, buffer: Buffer.from(body) });
  await scope.getByLabel('This is fictional test data').check();
  await scope.getByRole('button', { name: 'Upload', exact: true }).click();
}

export const measurementCsv = 'site,date,variable,value,unit\nBounty Bay,2026-11-12,temperature,22.4,C\n';

export async function loginSeed(page: Page) {
  await page.goto('/login');
  await page.getByLabel('Email').fill('anna@demo.pitcairn.invalid');
  await page.getByLabel('Password').fill('demo-pass-2026');
  await page.getByRole('button', { name: 'Log in' }).click();
  await expect(page).toHaveURL(/\/app$/);
}
