import { Link, useRouteError } from 'react-router';
import { Card, CardBody } from '../ui';

export function ErrorPage() {
  const error = useRouteError();
  const message = error instanceof Error ? error.message : 'Something went wrong.';
  return <main className="mx-auto max-w-2xl px-4 py-16"><Card><CardBody>
    <h1 className="text-xl font-semibold text-navy-900">We could not show this page</h1>
    <p className="mt-2 text-slate-600">{message}</p>
    <Link className="mt-4 inline-block text-teal-700 underline" to="/">Back to the home page</Link>
  </CardBody></Card></main>;
}

export function NotFoundPage() {
  return <main className="mx-auto max-w-2xl px-4 py-16"><Card><CardBody>
    <h1 className="text-xl font-semibold text-navy-900">Page not found</h1>
    <p className="mt-2 text-slate-600">The address may be incorrect or this page may have moved.</p>
    <Link className="mt-4 inline-block text-teal-700 underline" to="/">Back to the home page</Link>
  </CardBody></Card></main>;
}
