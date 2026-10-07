import { test, expect } from '@playwright/test';
import { loginSeed, switchPersona } from '../helpers/ui.js';

test('the bank simulator deduplicates a notification and settlement counts one verified payment', async ({ browser }) => {
  const context = await browser.newContext();
  const page = await context.newPage();
  try {
    await loginSeed(page);
    await switchPersona(page, 'ruth');
    await page.goto('/app/demo/bank');
    await expect(page.getByRole('heading', { name: 'Bank simulator' })).toBeVisible();
    await page.getByLabel('Invoice').selectOption({ index: 1 });
    const invoiceId = await page.getByLabel('Invoice').inputValue();
    const invoiceNumber = await page.getByLabel('Invoice').locator('option:checked').textContent();
    const number = invoiceNumber!.split(' · ')[0];
    const finance = await context.newPage();
    await finance.goto('/app/finance');
    const invoiceLink = finance.getByRole('link', { name: new RegExp(number) }).first();
    await expect(invoiceLink).toBeVisible();
    const projectId = (await invoiceLink.getAttribute('href'))!.split('/')[3];
    const invoice = async () => {
      const response = await context.request.get(`/api/v1/projects/${projectId}/invoices`);
      expect(response.ok()).toBeTruthy();
      return (await response.json()).items.find((i: { id: string }) => i.id === invoiceId);
    };
    const before = await invoice();
    const reference = `E2E-${Date.now()}`;
    await page.getByLabel('Amount (NZ$)').fill('1.00');
    await page.getByLabel('External reference').fill(reference);
    await page.getByRole('button', { name: 'Send bank notification' }).click();
    await expect(page.getByText('Result: duplicate: false', { exact: false })).toBeVisible();
    await page.getByRole('button', { name: 'Send the same notification again' }).click();
    await expect(page.getByText('Result: duplicate: true', { exact: false })).toBeVisible();
    const pending = await invoice();
    expect(pending.payments.filter((p: { external_ref: string }) => p.external_ref === reference)).toHaveLength(1);
    await finance.reload();
    const paymentRow = finance.getByRole('row').filter({ hasText: reference });
    await paymentRow.getByRole('button', { name: 'Verify' }).click();
    await expect(finance.getByText('Payment verified')).toBeVisible();
    const after = await invoice();
    expect(Number(after.net_verified_cents) - Number(before.net_verified_cents)).toBe(100);
    expect(after.payments.filter((p: { external_ref: string }) => p.external_ref === reference)).toHaveLength(1);
  } finally {
    await context.close();
  }
});
