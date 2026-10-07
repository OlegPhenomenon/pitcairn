import { test, expect } from '@playwright/test';
import { loginSeed, switchPersona } from '../helpers/ui.js';

test('a second team is denied and removing a member revokes a saved document URL', async ({ browser }) => {
  const leadContext = await browser.newContext();
  const lead = await leadContext.newPage();
  const memberContext = await browser.newContext();
  const member = await memberContext.newPage();
  try {
    await loginSeed(lead);
    await lead.getByRole('link', { name: 'Coral health around Pitcairn' }).first().click();
    const id = new URL(lead.url()).pathname.split('/')[3];
    await lead.goto(`/app/projects/${id}/application`);
    const link = lead.getByRole('link', { name: 'Download' }).first();
    await expect(link).toBeVisible();
    const url = await link.getAttribute('href');
    expect(url).toBeTruthy();

    await loginSeed(member);
    await switchPersona(member, 'liam');
    expect((await memberContext.request.get(url!)).status()).toBe(200);
    await switchPersona(member, 'lukas');
    await member.goto(`/app/projects/${id}`);
    await expect(member.getByText(/Project not found|access|permission|forbidden/i).first()).toBeVisible();
    expect([403, 404]).toContain((await memberContext.request.get(url!)).status());

    await switchPersona(member, 'liam');
    await lead.goto(`/app/projects/${id}/team`);
    await lead.getByRole('listitem').filter({ hasText: 'Liam Chen' }).getByRole('button', { name: 'Remove' }).click();
    const dialog = lead.getByRole('dialog', { name: 'Remove team member' });
    await expect(dialog.getByText('Liam Chen')).toBeVisible();
    await dialog.getByRole('button', { name: 'Remove member' }).click();
    await expect(lead.getByText('Member removed')).toBeVisible();
    expect((await memberContext.request.get(url!)).status()).toBe(403);
  } finally {
    await leadContext.close();
    await memberContext.close();
  }
});
