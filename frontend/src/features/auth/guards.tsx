import type { ReactNode } from 'react';
import { Navigate, Outlet, useLocation } from 'react-router';

import { PageLoading } from '../../ui/Skeleton';
import { Card, CardBody, EmptyState } from '../../ui';
import { useMe } from './api';

import { needsMfa } from './guardUtils';

/** Session required, MFA not necessarily verified yet (used by /mfa pages). */
export function RequireSession({ children }: { children?: ReactNode }) {
  const me = useMe();
  const location = useLocation();
  if (me.isPending) return <PageLoading />;
  if (!me.data) {
    const next = encodeURIComponent(location.pathname + location.search);
    return <Navigate to={`/login?next=${next}`} replace />;
  }
  return <>{children ?? <Outlet />}</>;
}

/** Full guard: session + (for staff/expert) verified MFA. */
export function RequireAuth({ children }: { children?: ReactNode }) {
  const me = useMe();
  const location = useLocation();
  if (me.isPending) return <PageLoading />;
  if (!me.data) {
    const next = encodeURIComponent(location.pathname + location.search);
    return <Navigate to={`/login?next=${next}`} replace />;
  }
  if (needsMfa(me.data)) {
    const next = encodeURIComponent(location.pathname + location.search);
    return <Navigate to={`/mfa?next=${next}`} replace />;
  }
  return <>{children ?? <Outlet />}</>;
}

export function RequireRole({ roles, children }: { roles: string[]; children?: ReactNode }) {
  const me = useMe();
  if (me.isPending) return <PageLoading />;
  const has = me.data && roles.some((r) => me.data.user.roles.includes(r));
  if (!has) {
    return (
      <Card>
        <CardBody>
          <EmptyState
            title="You don't have access to this page"
            body={`This page requires the ${roles.join(' or ')} role.`}
          />
        </CardBody>
      </Card>
    );
  }
  return <>{children ?? <Outlet />}</>;
}
