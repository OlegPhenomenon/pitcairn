import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router';

import { fieldError } from '../../api/client';
import { Banner, Button, Card, CardBody, FormField, Input, PageHeader, Select } from '../../ui';
import { useCreateProject } from './api';

// Template keys are defined by the backend seed (§4); the picker stays
// small until the templates endpoints arrive in a later slice.
const TEMPLATES = [
  {
    key: 'base_use',
    label: 'Research application — use of the Marine Science Base (Annex 2)',
  },
  { key: 'fieldwork_permit', label: 'Fieldwork permit' },
] as const;

export function NewProjectPage() {
  const navigate = useNavigate();
  const create = useCreateProject();
  const [title, setTitle] = useState('');
  const [templateKey, setTemplateKey] = useState<string>('base_use');

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const project = await create.mutateAsync({ template_key: templateKey, title });
      navigate(`/app/projects/${project.id}`);
    } catch {
      // The mutation error appears beside the form.
    }
  };

  const err = create.error;

  return (
    <>
      <PageHeader
        title="New project"
        subtitle="Start an application. You can fill in the details and invite your team afterwards — nothing is submitted yet."
      />
      <Card className="max-w-xl">
        <CardBody>
          <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
            {err && !err.fields && <Banner tone="error">{err.message}</Banner>}
            <FormField
              label="Application type"
              required
              error={fieldError(err, 'template_key')}
            >
              <Select
                name="template_key"
                value={templateKey}
                onChange={(e) => setTemplateKey(e.target.value)}
              >
                {TEMPLATES.map((t) => (
                  <option key={t.key} value={t.key}>
                    {t.label}
                  </option>
                ))}
              </Select>
            </FormField>
            <FormField
              label="Project title"
              required
              error={fieldError(err, 'title')}
              help="A short, clear title — e.g. “Coral health around Pitcairn”."
            >
              <Input
                name="title"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                required
                maxLength={300}
              />
            </FormField>
            <div className="flex gap-2">
              <Button type="submit" loading={create.isPending}>
                Create project
              </Button>
              <Button type="button" variant="ghost" onClick={() => navigate(-1)}>
                Cancel
              </Button>
            </div>
          </form>
        </CardBody>
      </Card>
    </>
  );
}
