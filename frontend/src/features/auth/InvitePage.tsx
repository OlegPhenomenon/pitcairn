import { useNavigate, useParams } from 'react-router';

import { ApiError } from '../../api/client';
import { Banner, Button, Card, CardBody } from '../../ui';
import { useAcceptInvitation } from './api';
import { AuthShell } from './AuthShell';
import { RequireSession } from './guards';

function InviteInner() {
  const { token } = useParams<{ token: string }>();
  const navigate = useNavigate();
  const accept = useAcceptInvitation();

  const onAccept = async () => {
    if (!token) return;
    const res = await accept.mutateAsync(token);
    navigate(`/app/projects/${res.project_id}`);
  };

  const err = accept.error as ApiError | null;

  return (
    <AuthShell
      title="Project invitation"
      subtitle="You've been invited to join a project team on the Pitcairn Research Hub."
    >
      <Card>
        <CardBody className="flex flex-col gap-4">
          {err && (
            <Banner tone="error">
              {err.code === 'invitation_invalid'
                ? err.message
                : err.message || 'The invitation could not be accepted.'}
            </Banner>
          )}
          <p className="text-sm text-slate-600">
            Accepting adds you to the project team with the role from the invitation. The
            invitation is valid only for the email address of your account.
          </p>
          <Button onClick={onAccept} loading={accept.isPending} className="w-full">
            Accept invitation
          </Button>
        </CardBody>
      </Card>
    </AuthShell>
  );
}

export function InvitePage() {
  return (
    <RequireSession>
      <InviteInner />
    </RequireSession>
  );
}
