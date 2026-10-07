import { createServer } from 'node:http';
import { test, expect, type Page, type Locator } from '@playwright/test';
import { loginSeed, measurementCsv, switchPersona, uploadFile } from '../helpers/ui.js';

const title = 'Bounty Bay plankton observations';
const condition = 'No sampling within the marked reef buffer';
const date = (offset: number) => new Date(Date.now() + offset * 86_400_000).toISOString().slice(0, 10);

async function selectMatching(select: Locator, name: RegExp) {
  const value = await select.getByRole('option', { name }).getAttribute('value');
  if (!value) throw new Error(`No option matching ${name}`);
  await select.selectOption(value);
}

async function visit(page: Page, id: string, tab = '') {
  await page.goto(`/app/projects/${id}${tab ? `/${tab}` : ''}`);
  await expect(page.getByRole('heading', { name: title })).toBeVisible();
}

async function documentUpload(page: Page, slot: string, content: string, version = false) {
  await page.getByRole('button', { name: `${version ? 'Upload new version' : 'Upload'} — ${slot}` }).click();
  const dialog = page.getByRole('dialog', { name: new RegExp(slot) });
  await uploadFile(page, `${slot.toLowerCase().replaceAll(' ', '-')}.txt`, content);
  await expect(dialog).toBeHidden();
}

async function deliverable(page: Page, name: string) {
  await page.getByRole('button', { name: 'Propose deliverable' }).click();
  const dialog = page.getByRole('dialog', { name: 'Propose deliverable' });
  await dialog.getByLabel('Title').fill(name);
  await dialog.getByLabel('Description').fill(`Fictional ${name.toLowerCase()} from Bounty Bay`);
  await dialog.getByLabel('Kind').selectOption(name === 'Survey dataset' ? 'dataset' : 'report');
  await dialog.getByLabel('Due date').fill(date(90));
  await selectMatching(dialog.getByLabel('Recipient'), /Maria Ellis/);
  await dialog.getByRole('button', { name: 'Save proposal' }).click();
  await expect(dialog).toBeHidden();
}

async function openResult(page: Page, name: string) {
  await page.getByRole('button', { name, exact: true }).click();
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
}

async function submitResult(page: Page, filename: string, content: string, csv = false, fileTitle = csv ? 'Survey measurements' : 'Field report', links: { url: string; description: string }[] = []) {
  await page.getByLabel('File title').fill(fileTitle);
  await uploadFile(page, filename, content, csv ? 'text/csv' : 'text/plain');
  await expect(page.getByText(fileTitle, { exact: false }).last()).toBeVisible();
  await page.getByRole('checkbox', { name: new RegExp(`^${fileTitle} ·`) }).check();
  if (csv) {
    const dictionary = page.getByRole('heading', { name: 'Data dictionary' }).locator('..');
    await dictionary.getByLabel('Column').fill('value');
    await dictionary.getByLabel('Description').fill('Sea surface temperature');
    await dictionary.getByLabel('Unit').fill('C');
    await dictionary.getByLabel('Method').fill('Fictional sensor');
  }
  for (const [i, link] of links.entries()) {
    if (i > 0) await page.getByRole('button', { name: 'Add link' }).click();
    await page.getByLabel('Url', { exact: true }).nth(i).fill(link.url);
    await page.getByLabel('Description', { exact: true }).nth(i).fill(link.description);
    await page.getByLabel('Version Label', { exact: true }).nth(i).fill('v1');
  }
  await page.getByRole('button', { name: 'Submit results' }).click();
  await expect(page.getByText('Results submitted', { exact: true })).toBeVisible();
}

test('new team completes the application, permit, booking, payment, and publication story', async ({ browser }) => {
  test.setTimeout(240_000);
  const researcherContext = await browser.newContext();
  const researcher = await researcherContext.newPage();
  const colleagueContext = await browser.newContext();
  const colleague = await colleagueContext.newPage();
  const staffContext = await browser.newContext();
  const staff = await staffContext.newPage();
  const anonymous = await browser.newContext();
  // A stand-in university repository on a real socket: the link checker
  // fetches it over HTTP; deleting a file makes it answer 404.
  const repository = new Map([['/repo/field-photos.zip', 200], ['/repo/closed-archive.zip', 401]]);
  const repositoryServer = createServer((req, res) => {
    res.statusCode = repository.get(req.url ?? '') ?? 404;
    res.end();
  });
  await new Promise<void>(resolve => repositoryServer.listen(18091, '127.0.0.1', resolve));
  try {
    await researcher.goto('/register');
    await researcher.getByLabel('Full name').fill('Dr Mira Solis');
    await researcher.getByLabel('Email').fill('mira@fictional-ocean.invalid');
    await researcher.getByLabel('Organisation').fill('Southern Ocean Field Institute (fictional)');
    await researcher.getByLabel('Password').fill('fictional-pass-2026');
    await researcher.getByRole('button', { name: 'Create account' }).click();
    await expect(researcher).toHaveURL(/\/app$/);
    await researcher.getByRole('link', { name: 'New project' }).click();
    await researcher.getByLabel('Project title').fill(title);
    await researcher.getByRole('button', { name: 'Create project' }).click();
    await expect(researcher).toHaveURL(/\/app\/projects\/[0-9a-f-]+$/);
    const projectId = new URL(researcher.url()).pathname.split('/')[3];
    expect(projectId).toBeTruthy();

    await visit(researcher, projectId, 'team');
    await researcher.getByLabel('Email').fill('tari@fictional-ocean.invalid');
    await researcher.getByRole('button', { name: 'Send invitation' }).click();
    const inviteUrl = await researcher.getByRole('link', { name: /\/invite\// }).getAttribute('href');
    expect(inviteUrl).toBeTruthy();
    await colleague.goto('/register');
    await colleague.getByLabel('Full name').fill('Tari Vale');
    await colleague.getByLabel('Email').fill('tari@fictional-ocean.invalid');
    await colleague.getByLabel('Organisation').fill('Southern Ocean Field Institute (fictional)');
    await colleague.getByLabel('Password').fill('fictional-pass-2026');
    await colleague.getByRole('button', { name: 'Create account' }).click();
    await expect(colleague).toHaveURL(/\/app$/);
    await colleague.goto(inviteUrl!);
    await colleague.getByRole('button', { name: 'Accept invitation' }).click();
    await expect(colleague.getByRole('heading', { name: title })).toBeVisible();

    await visit(researcher, projectId, 'application');
    const answers: Record<string, string> = {
      'Name of applicant (the lead researcher who will take responsibility)': 'Dr Mira Solis',
      'Position': 'Research fellow',
      'Institution': 'Southern Ocean Field Institute (fictional)',
      'Address': '1 Imaginary Quay, Wellington',
      'Funder(s)': 'Fictional Ocean Trust',
      'Named researchers who will be participating': 'Dr Mira Solis, 34, no special needs',
      'Title of proposed research': title,
      'Aims (overarching statement, max 50 words)': 'Observe plankton near Bounty Bay.',
      'Objectives (numbered list, max 200 words)': '1. Record surface temperature. 2. Report observations.',
      'Proposed approaches/methods (max 500 words)': 'Use a handheld sensor and visual observations from shore.',
      'Anticipated outputs and benefit to the Pitcairn Island community (max 500 words)': 'Share a public report and dataset.',
      'Data management and sharing (max 300 words)': 'Publish fictional aggregate data and a report.',
      'Timeline (minimum resolution of a week)': 'Week 1: field observations. Week 2: analysis.',
      'Budget (total project budget plus Pitcairn-specific budget)': 'Total NZ$5,000; Pitcairn NZ$1,000.',
      'Summary of key risks and mitigations': 'Work from shore; stop during rough seas.',
    };
    for (const [label, value] of Object.entries(answers)) await researcher.getByRole('textbox', { name: label, exact: true }).fill(value);
    await researcher.getByLabel('Proposed dates on Pitcairn start').fill(date(30));
    await researcher.getByLabel('Proposed dates on Pitcairn end').fill(date(35));
    await visit(researcher, projectId, 'sites');
    await researcher.getByLabel('Site name').fill('Bounty Bay shore station');
    await researcher.getByLabel('Latitude').fill('-25.067');
    await researcher.getByLabel('Longitude').fill('-130.101');
    await researcher.getByRole('button', { name: 'Add point' }).click();
    await researcher.getByRole('button', { name: 'Add site' }).click();
    await expect(researcher.getByText('Bounty Bay shore station').first()).toBeVisible();
    await visit(researcher, projectId, 'application');
    for (const slot of ['Field safety plan', 'Insurance certificate', 'CVs of team', 'Permits held/needed']) {
      await documentUpload(researcher, slot, `Fictional ${slot} for ${title}.`);
    }
    await expect(researcher.getByText('Scan clean')).toHaveCount(4);
    const privateUrl = await researcher.getByRole('link', { name: 'Download' }).first().getAttribute('href');
    expect(privateUrl).toBeTruthy();
    await researcher.getByLabel('This is fictional test data').check();
    await researcher.getByRole('button', { name: 'Submit application' }).click();
    await expect(researcher.getByText('Application submitted')).toBeVisible();
    await expect(researcher.getByRole('link', { name: 'View revisions' })).toBeVisible();
    const projectReference = (await researcher.getByRole('main').textContent())!.match(/PIT-\d{4}-\d{4}/)![0];

    await loginSeed(staff);
    await switchPersona(staff, 'maria');
    await visit(staff, projectId);
    await staff.getByRole('button', { name: 'Open for screening' }).click();
    await expect(staff.getByText('Application opened for screening')).toBeVisible();
    await visit(staff, projectId, 'messages?anchor=document:safety_plan');
    await expect(staff.getByText('About: document safety_plan')).toBeVisible();
    await staff.getByLabel('Message').fill('Please provide an updated safety plan for shore sampling.');
    await staff.getByLabel('Request information from team').check();
    await staff.getByLabel('What the team needs to do').fill('Upload revised safety plan');
    await staff.getByRole('button', { name: 'Request information' }).click();
    await expect(staff.getByText('Information requested')).toBeVisible();

    await visit(researcher, projectId);
    await expect(researcher.getByRole('heading', { name: 'What needs attention' })).toBeVisible();
    await expect(researcher.getByText('Upload revised safety plan').first()).toBeVisible();
    await researcher.getByRole('link', { name: 'Upload document' }).click();
    await documentUpload(researcher, 'Field safety plan', 'Revised fictional shore safety plan with weather stop rules.', true);
    await expect(researcher.getByText('Scan clean')).toHaveCount(4);
    await researcher.getByLabel('This is fictional test data').check();
    await researcher.getByRole('button', { name: 'Resubmit application' }).click();
    await expect(researcher.getByText('Application resubmitted')).toBeVisible();
    await visit(researcher, projectId, 'revisions');
    await expect(researcher.getByText(/Revision 1/).first()).toBeVisible();
    await expect(researcher.getByText(/Revision 2/).first()).toBeVisible();

    await visit(staff, projectId, 'messages');
    await staff.getByRole('button', { name: 'Resolve' }).click();
    await expect(staff.getByText('Action item resolved')).toBeVisible();
    await visit(staff, projectId, 'review');
    await selectMatching(staff.getByLabel('Expert'), /Dr James Okafor/);
    await staff.getByRole('button', { name: 'Assign expert' }).click();
    await expect(staff.getByText('Expert invited')).toBeVisible();
    await switchPersona(staff, 'james');
    await visit(staff, projectId, 'review');
    await staff.getByRole('button', { name: 'Accept assignment' }).click();
    await staff.getByLabel('Opinion').fill('The revised safety plan is adequate for shore observations.');
    await staff.getByLabel('Recommendation').selectOption('approve_with_conditions');
    await staff.getByRole('button', { name: 'Submit opinion' }).click();
    await expect(staff.getByText('Opinion submitted')).toBeVisible();

    await switchPersona(staff, 'helen');
    await visit(staff, projectId, 'decisions');
    await staff.getByLabel('Application revision').selectOption({ label: 'Revision 2' });
    await staff.getByLabel('Permit name').fill('Shore plankton observation');
    await staff.getByLabel('Basis').fill('Expert advice and updated safety plan.');
    await staff.getByLabel('Valid from').fill(date(29));
    await staff.getByLabel('Valid to').fill(date(40));
    await staff.getByLabel('Permitted activities (one per line)').fill('Shore plankton observation');
    await staff.getByLabel('Conditions (one per line)').fill(condition);
    await staff.getByRole('button', { name: 'Save draft' }).click();
    await expect(staff.getByText('Decision draft saved')).toBeVisible();
    await staff.getByRole('button', { name: 'Issue decision' }).first().click();
    await staff.getByRole('dialog', { name: 'Issue decision' }).getByRole('button', { name: 'Issue decision' }).click();
    await expect(staff.getByRole('heading', { name: 'Permit: Shore plankton observation' })).toBeVisible();
    await visit(researcher, projectId);
    await expect(researcher.getByText(`Conditions: ${condition}`)).toBeVisible();

    await visit(researcher, projectId, 'trips');
    await researcher.getByRole('button', { name: 'Plan a trip' }).click();
    const trip = researcher.getByRole('dialog', { name: 'Plan a trip' });
    await trip.getByLabel('Trip title').fill('Bounty Bay field visit');
    await trip.getByLabel('Arrival date').fill(date(30));
    await trip.getByLabel('Departure date').fill(date(33));
    await trip.getByLabel('Participants').fill('Dr Mira Solis');
    await trip.getByRole('button', { name: 'Save trip' }).click();
    await expect(trip).toBeHidden();
    await researcher.getByRole('button', { name: 'Request booking' }).click();
    const booking = researcher.getByRole('dialog', { name: 'Request a booking' });
    await selectMatching(booking.getByLabel('Resource'), /MSB twin bedroom/);
    await booking.getByRole('button', { name: 'Send request' }).click();
    await expect(booking).toBeHidden();
    await switchPersona(staff, 'sam');
    await staff.goto('/app/calendar');
    await staff.getByLabel('Month').fill(date(30).slice(0, 7));
    await staff.getByRole('heading', { name: 'Pending requests' }).scrollIntoViewIfNeeded();
    await staff.getByRole('button', { name: 'Confirm', exact: true }).last().click();
    await expect(staff.getByText('Booking confirmed')).toBeVisible();

    await switchPersona(staff, 'ruth');
    await staff.goto('/app/finance');
    await selectMatching(staff.getByLabel('Project', { exact: true }), new RegExp(title));
    await staff.getByRole('checkbox', { name: /MSB twin bedroom/ }).check();
    await staff.getByRole('button', { name: 'Create draft invoice' }).click();
    await expect(staff.getByText('Draft invoice created')).toBeVisible();
    await staff.getByRole('row').filter({ hasText: projectReference }).getByRole('button', { name: 'Issue', exact: true }).click();
    await staff.getByRole('dialog', { name: 'Issue invoice' }).getByRole('button', { name: 'Issue invoice' }).click();
    await expect(staff.getByText('Invoice issued')).toBeVisible();
    await visit(researcher, projectId, 'invoices');
    await researcher.getByRole('button', { name: 'Pay with test card' }).click();
    await researcher.getByRole('dialog', { name: 'Pay with test card' }).getByRole('button', { name: 'Record test payment' }).click();
    await expect(researcher.getByText('Test payment recorded')).toBeVisible();
    await expect(researcher.getByText(/Received NZ\$\d/)).toBeVisible();

    await visit(researcher, projectId, 'results');
    await deliverable(researcher, 'Survey dataset');
    await deliverable(researcher, 'Field report');
    await switchPersona(staff, 'maria');
    await visit(staff, projectId, 'results');
    for (const name of ['Survey dataset', 'Field report']) {
      await openResult(staff, name);
      await staff.getByRole('button', { name: 'Agree to these terms' }).click();
      await expect(staff.getByText('Deliverable updated')).toBeVisible();
      await staff.getByRole('button', { name: 'Close details' }).click();
    }

    // Spec §5 item 5 and §6 "Проверка изменений": a second, independent permit;
    // extending one permit leaves the other untouched; a reschedule after the
    // trip, permit and deadlines are agreed shows its consequences and does not
    // silently change any decision; revoking one permit keeps the other.
    await switchPersona(staff, 'helen');
    await visit(staff, projectId, 'decisions');
    await staff.getByLabel('Permit name').fill('Plankton net tows');
    await staff.getByLabel('Application revision').selectOption({ label: 'Revision 2' });
    await staff.getByLabel('Basis').fill('Separate activity: small plankton net tows from the jetty.');
    await staff.getByLabel('Valid from').fill(date(29));
    await staff.getByLabel('Valid to').fill(date(40));
    await staff.getByLabel('Permitted activities (one per line)').fill('Plankton net tows from the jetty');
    await staff.getByRole('button', { name: 'Save draft' }).click();
    await expect(staff.getByText('Decision draft saved')).toBeVisible();
    await staff.getByRole('button', { name: 'Issue decision' }).first().click();
    await staff.getByRole('dialog', { name: 'Issue decision' }).getByRole('button', { name: 'Issue decision' }).click();
    await expect(staff.getByRole('heading', { name: 'Permits in force (2)' })).toBeVisible();
    const permitA = staff.getByRole('region', { name: 'Permit Shore plankton observation' });
    const permitB = staff.getByRole('region', { name: 'Permit Plankton net tows' });
    const permitBTerms = await permitB.textContent();

    await visit(researcher, projectId, 'changes');
    await researcher.getByLabel('Type').selectOption('extend_permit');
    await selectMatching(researcher.getByLabel('Permit to extend'), /Shore plankton observation/);
    await researcher.getByLabel('Requested end date').fill(date(100));
    await researcher.getByLabel('Describe the change').fill('Extend the shore observation permit to cover a second visit.');
    await researcher.getByRole('button', { name: 'Send change request' }).click();
    await expect(researcher.getByText('Change requested')).toBeVisible();
    await switchPersona(staff, 'maria');
    await visit(staff, projectId, 'changes');
    const extension = staff.getByRole('listitem').filter({ hasText: 'extend permit' });
    await expect(extension.getByText(/Permit: Shore plankton observation/)).toBeVisible();
    await expect(extension.getByText(/Permit: Plankton net tows/)).toHaveCount(0);

    await switchPersona(staff, 'helen');
    await visit(staff, projectId, 'decisions');
    await permitA.getByRole('button', { name: 'Extend this permit' }).click();
    await staff.getByLabel('Basis').fill('Second visit approved on the same conditions.');
    await staff.getByLabel('Valid to').fill(date(100));
    await staff.getByRole('button', { name: 'Save draft' }).click();
    await expect(staff.getByText('Decision draft saved')).toBeVisible();
    await staff.getByRole('button', { name: 'Issue decision' }).first().click();
    await staff.getByRole('dialog', { name: 'Issue decision' }).getByRole('button', { name: 'Issue decision' }).click();
    await expect(permitA.getByText('Latest: Extension')).toBeVisible();
    await expect(staff.getByRole('heading', { name: 'Permits in force (2)' })).toBeVisible();
    expect(await permitB.textContent()).toBe(permitBTerms);
    await expect(staff.getByText(/Replaces Research permit “Shore plankton observation”/)).toBeVisible();

    await switchPersona(staff, 'maria');
    await visit(staff, projectId, 'changes');
    await selectMatching(extension.getByLabel('Issued decision implementing this change'), /Extension · Shore plankton observation/);
    await extension.getByRole('button', { name: 'Approve' }).click();
    await expect(staff.getByText('Change approved')).toBeVisible();

    await visit(researcher, projectId, 'changes');
    await researcher.getByLabel('Type').selectOption('reschedule_trip');
    await selectMatching(researcher.getByLabel('Trip'), /Bounty Bay field visit/);
    await researcher.getByLabel('New arrival').fill(date(88));
    await researcher.getByLabel('New departure').fill(date(95));
    await researcher.getByLabel('Describe the change').fill('The supply ship moved; we need to arrive later.');
    await researcher.getByRole('button', { name: 'Send change request' }).click();
    await expect(researcher.getByText('Change requested')).toBeVisible();
    await visit(staff, projectId, 'changes');
    const reschedule = staff.getByRole('listitem').filter({ hasText: 'reschedule trip' });
    await expect(reschedule.getByText(/Booking: MSB twin bedroom · Will be re-requested/)).toBeVisible();
    await expect(reschedule.getByText(/Deliverable: Survey dataset/)).toBeVisible();
    await expect(reschedule.getByText(/Permit: Plankton net tows/)).toBeVisible();
    await expect(reschedule.getByText(/Permit: Shore plankton observation/)).toHaveCount(0);
    await reschedule.getByRole('button', { name: 'Approve' }).click();
    await expect(staff.getByText('Change approved')).toBeVisible();
    await visit(staff, projectId, 'decisions');
    await expect(staff.getByRole('heading', { name: 'Permits in force (2)' })).toBeVisible();
    expect(await permitB.textContent()).toBe(permitBTerms);

    await switchPersona(staff, 'helen');
    await visit(staff, projectId, 'decisions');
    await permitB.getByRole('button', { name: 'Revoke this permit' }).click();
    await staff.getByLabel('Basis').fill('Net tows are no longer needed.');
    await staff.getByRole('button', { name: 'Save draft' }).click();
    await expect(staff.getByText('Decision draft saved')).toBeVisible();
    await staff.getByRole('button', { name: 'Issue decision' }).first().click();
    await staff.getByRole('dialog', { name: 'Issue decision' }).getByRole('button', { name: 'Issue decision' }).click();
    await expect(staff.getByRole('heading', { name: 'Permits in force (1)' })).toBeVisible();
    await expect(permitB.getByText('Revoked', { exact: true })).toBeVisible();
    await expect(permitA.getByText('In force', { exact: true })).toBeVisible();
    await switchPersona(staff, 'maria');
    await visit(researcher, projectId, 'results');
    await openResult(researcher, 'Survey dataset');
    await submitResult(researcher, 'measurements.csv', measurementCsv, true);
    await researcher.getByRole('button', { name: 'Close details' }).click();
    await openResult(researcher, 'Field report');
    await submitResult(researcher, 'field-report.txt', 'Fictional field report: plankton observations at Bounty Bay.', false, undefined, [
      { url: 'http://127.0.0.1:18091/repo/field-photos.zip', description: 'Field photo archive' },
      { url: 'http://127.0.0.1:18091/repo/closed-archive.zip', description: 'Raw sensor archive' },
    ]);
    await researcher.getByRole('button', { name: 'Close details' }).click();
    await visit(staff, projectId, 'results');
    await openResult(staff, 'Survey dataset');
    await staff.getByRole('button', { name: 'Request changes' }).click();
    await staff.getByRole('dialog', { name: 'Request changes' }).getByLabel('Note').fill('Add the instrument calibration note.');
    await staff.getByRole('dialog', { name: 'Request changes' }).getByRole('button', { name: 'Confirm' }).click();
    await expect(staff.getByText('Deliverable updated')).toBeVisible();
    await staff.getByRole('button', { name: 'Close details' }).click();
    await openResult(staff, 'Field report');
    await staff.getByRole('button', { name: 'Accept receipt' }).click();
    await staff.getByRole('dialog', { name: 'Accept receipt' }).getByRole('button', { name: 'Confirm' }).click();

    // Spec §5 item 11: Maria checks the repository links for real. The same
    // URL turns from available to not found once the file is deleted; a
    // login-protected archive is reported as such, not as lost data.
    const photos = staff.locator('div').filter({ has: staff.getByRole('link', { name: 'Field photo archive' }) }).last();
    const closed = staff.locator('div').filter({ has: staff.getByRole('link', { name: 'Raw sensor archive' }) }).last();
    await photos.getByRole('button', { name: 'Check now' }).click();
    await expect(photos.getByText('Available', { exact: true })).toBeVisible();
    await expect(photos.getByText(/last checked/)).toBeVisible();
    await closed.getByRole('button', { name: 'Check now' }).click();
    await expect(closed.getByText('Login required', { exact: true })).toBeVisible();
    await expect(closed.getByText(/may be agreed closed access/)).toBeVisible();
    repository.delete('/repo/field-photos.zip');
    await photos.getByRole('button', { name: 'Check now' }).click();
    await expect(photos.getByText('Not found', { exact: true })).toBeVisible();
    await expect(photos.getByText(/HTTP 404.*last checked/)).toBeVisible();
    await staff.getByRole('button', { name: 'Close details' }).click();
    await visit(researcher, projectId, 'results');
    await openResult(researcher, 'Survey dataset');
    await submitResult(researcher, 'corrected-measurements.csv', measurementCsv + 'Bounty Bay,2026-11-13,temperature,22.5,C\n', true, 'Corrected survey measurements');
    await visit(staff, projectId, 'results');
    await openResult(staff, 'Survey dataset');
    await staff.getByRole('button', { name: 'Accept receipt' }).click();
    await staff.getByRole('dialog', { name: 'Accept receipt' }).getByRole('button', { name: 'Confirm' }).click();
    await staff.getByRole('button', { name: 'Close details' }).click();

    // Spec §5 item 14: another staff member finds the new study by
    // organisation and year, and the accepted (corrected) table drives the chart.
    await switchPersona(staff, 'helen');
    await staff.goto('/app/search');
    await staff.getByLabel('Organisation').fill('Southern Ocean Field Institute');
    await staff.getByLabel('Year').fill(date(30).slice(0, 4));
    await staff.getByRole('button', { name: 'Search', exact: true }).click();
    await expect(staff.getByText(title).first()).toBeVisible();
    await staff.goto('/app/reports');
    await staff.getByLabel('Variable').selectOption('temperature');
    await expect(staff.getByRole('heading', { name: 'temperature (C)' })).toBeVisible();
    const measured = staff.getByRole('row').filter({ hasText: `${projectReference} — Survey dataset` });
    await expect(measured).toHaveCount(2);
    await expect(measured.filter({ hasText: '22.5 C' })).toContainText('(submission #2)');
    await switchPersona(staff, 'maria');
    await visit(staff, projectId, 'results');
    await openResult(staff, 'Field report');
    await staff.getByRole('button', { name: 'Set publication' }).click();
    const publishing = staff.getByRole('dialog', { name: 'Publish results' });
    await publishing.getByLabel('Publication level').selectOption('metadata_and_files');
    await publishing.getByRole('checkbox', { name: 'Field report' }).check();
    await publishing.getByRole('button', { name: 'Publish', exact: true }).click();
    await expect(publishing).toBeHidden();
    const publicPage = await anonymous.newPage();
    await publicPage.goto('/catalog');
    await publicPage.getByLabel('Search published research').fill(title);
    await publicPage.getByRole('button', { name: 'Search' }).click();
    await publicPage.getByRole('link', { name: title }).click();
    const publicUrl = await publicPage.getByRole('link', { name: /Field report/ }).getAttribute('href');
    expect(publicUrl).toBeTruthy();
    expect((await anonymous.request.get(publicUrl!)).status()).toBe(200);
    expect([403, 404]).toContain((await anonymous.request.get(privateUrl!)).status());
    await switchPersona(staff, 'lukas');
    expect([403, 404]).toContain((await staffContext.request.get(privateUrl!)).status());
  } finally {
    await researcherContext.close();
    await colleagueContext.close();
    await staffContext.close();
    await anonymous.close();
    repositoryServer.close();
  }
});
