import { Link } from 'react-router';
import { Anchor, FlaskConical, Landmark, MapPin, Users } from 'lucide-react';
import { MapContainer, Marker, Popup, TileLayer } from 'react-leaflet';

import { Button } from '../../ui';
import { useMe } from '../auth/api';
import { usePersonas } from '../demo/api';

const STEPS = [
  {
    title: 'Create your project',
    body: 'Register, start an application and answer the structured form (Annex 2 for base use).',
  },
  {
    title: 'Submit and reply',
    body: 'Pitcairn staff screen your application and may request missing items — you reply in the same workspace.',
  },
  {
    title: 'Expert review & decision',
    body: 'An independent expert reviews the science; a decision maker issues the permit or a refusal with reasons.',
  },
  {
    title: 'Plan the trip',
    body: 'Book rooms, lab space, equipment and the boat; receive an invoice and pay it.',
  },
  {
    title: 'Deliver results',
    body: 'Agree deliverables before fieldwork, submit results after — accepted work is published to the open catalog.',
  },
] as const;

const AUDIENCES = [
  {
    icon: <FlaskConical className="size-6 text-teal-700" aria-hidden />,
    title: 'Researchers',
    body: 'Foreign scientists applying to work at the Marine Science Base — one workspace for the application, replies, bookings and results.',
  },
  {
    icon: <Landmark className="size-6 text-teal-700" aria-hidden />,
    title: 'Pitcairn staff',
    body: 'Coordinators, decision makers, base and finance staff — screening, permits, resources and payments with a full audit trail.',
  },
  {
    icon: <Users className="size-6 text-teal-700" aria-hidden />,
    title: 'Reviewers & the public',
    body: 'Experts review assigned applications; the public catalog shares published results openly.',
  },
] as const;

// Pitcairn Island, South Pacific (25.0667° S, 130.1° W).
const PITCAIRN: [number, number] = [-25.0667, -130.1];

export function LandingPage() {
  const me = useMe();
  const personas = usePersonas();
  const loggedIn = !!me.data;
  const demo = Boolean(me.data?.demo_mode) || personas.isSuccess;

  return (
    <div className="flex min-h-dvh flex-col bg-sand-50">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded focus:bg-white focus:px-3 focus:py-2"
      >
        Skip to content
      </a>
      <header className="bg-navy-900 text-white">
        <div className="mx-auto flex w-full max-w-6xl items-center gap-3 px-4 py-3">
          <Link to="/" className="inline-flex items-center gap-2 font-semibold">
            <Anchor className="size-5 text-teal-300" aria-hidden />
            Pitcairn Research Hub
          </Link>
          <nav className="ml-auto flex items-center gap-2" aria-label="Public">
            <Link
              to="/catalog"
              className="rounded-md px-3 py-2 text-sm font-medium text-sand-100 hover:bg-navy-800"
            >
              Catalog
            </Link>
            {loggedIn ? (
              <Link
                to="/app"
                className="rounded-md bg-teal-700 px-3 py-2 text-sm font-medium text-white hover:bg-teal-600"
              >
                Open the app
              </Link>
            ) : (
              <Link
                to="/login"
                className="rounded-md bg-teal-700 px-3 py-2 text-sm font-medium text-white hover:bg-teal-600"
              >
                Log in
              </Link>
            )}
          </nav>
        </div>
      </header>

      <main id="main">
        <section className="bg-navy-900 pb-16 text-white">
          <div className="mx-auto grid max-w-6xl gap-10 px-4 pt-12 md:grid-cols-2 md:items-center">
            <div>
              <p className="text-sm font-semibold tracking-wide text-teal-300 uppercase">
                Pitcairn Islands · Marine Science Base
              </p>
              <h1 className="mt-3 text-3xl font-bold tracking-tight sm:text-4xl">
                Research permits and data, in one place
              </h1>
              <p className="mt-4 max-w-xl text-lg text-navy-100">
                The Hub manages the whole life-cycle of visiting research on the Pitcairn
                Islands: application, expert review, the permit decision, base logistics,
                invoicing — and the results scientists deliver back to Pitcairn.
              </p>
              <div className="mt-6 flex flex-wrap gap-3">
                {demo && (
                  <Button
                    size="md"
                    onClick={() => {
                      document
                        .getElementById('demo-cta')
                        ?.scrollIntoView({ behavior: 'smooth', block: 'start' });
                    }}
                  >
                    Try the demo
                  </Button>
                )}
                <Link to="/catalog">
                  <Button variant="secondary">Browse the catalog</Button>
                </Link>
                {!loggedIn && (
                  <Link to="/register">
                    <Button variant="ghost" className="text-sand-100 hover:bg-navy-800">
                      Create an account
                    </Button>
                  </Link>
                )}
              </div>
            </div>
            <div className="overflow-hidden rounded-lg shadow-lg">
              <MapContainer
                center={PITCAIRN}
                zoom={8}
                scrollWheelZoom={false}
                className="h-64 w-full"
                attributionControl
              >
                <TileLayer url="https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png" />
                <Marker position={PITCAIRN}>
                  <Popup>Pitcairn Island — Marine Science Base</Popup>
                </Marker>
              </MapContainer>
              <p className="flex items-center gap-1.5 bg-navy-800 px-3 py-2 text-xs text-navy-200">
                <MapPin className="size-3.5" aria-hidden /> Pitcairn Island, South Pacific — ©
                OpenStreetMap contributors
              </p>
            </div>
          </div>
        </section>

        <section aria-labelledby="audiences" className="mx-auto max-w-6xl px-4 py-12">
          <h2 id="audiences" className="text-2xl font-bold text-navy-900">
            Who the hub is for
          </h2>
          <div className="mt-6 grid gap-4 md:grid-cols-3">
            {AUDIENCES.map((a) => (
              <div
                key={a.title}
                className="rounded-lg border border-slate-200 bg-white p-5 shadow-sm"
              >
                {a.icon}
                <h3 className="mt-3 font-semibold text-navy-900">{a.title}</h3>
                <p className="mt-1 text-sm text-slate-600">{a.body}</p>
              </div>
            ))}
          </div>
        </section>

        <section aria-labelledby="how" className="bg-sand-100">
          <div className="mx-auto max-w-6xl px-4 py-12">
            <h2 id="how" className="text-2xl font-bold text-navy-900">
              How it works
            </h2>
            <ol className="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-5">
              {STEPS.map((s, i) => (
                <li
                  key={s.title}
                  className="rounded-lg border border-slate-200 bg-white p-4 shadow-sm"
                >
                  <span
                    aria-hidden
                    className="flex size-8 items-center justify-center rounded-full bg-teal-700 text-sm font-bold text-white"
                  >
                    {i + 1}
                  </span>
                  <h3 className="mt-3 text-sm font-semibold text-navy-900">{s.title}</h3>
                  <p className="mt-1 text-sm text-slate-600">{s.body}</p>
                </li>
              ))}
            </ol>
          </div>
        </section>

        {demo && (
          <section id="demo-cta" aria-labelledby="demo" className="mx-auto max-w-6xl px-4 py-12">
            <div className="rounded-lg border border-teal-200 bg-teal-50 p-6">
              <h2 id="demo" className="text-xl font-bold text-teal-950">
                Try the demo
              </h2>
              <p className="mt-2 max-w-2xl text-sm text-teal-900">
                This install runs in demo mode with fictional people and data. Log in and
                use the <strong>persona switcher</strong> in the header to step into each
                role — a researcher (Anna), the coordinator (Maria), the expert (James), the
                decision maker (Helen), the base manager (Sam), finance (Ruth), a boat
                provider (David) and the site admin. Every action is real; mail, payments
                and antivirus are simulated.
              </p>
              <div className="mt-4">
                <Link to="/app/demo/story" className="mr-3 text-sm font-semibold text-teal-800 underline">Open the story guide</Link>
                <Link to="/login">
                  <Button>Log in and pick a persona</Button>
                </Link>
              </div>
            </div>
          </section>
        )}
      </main>

      <footer className="mt-auto border-t border-slate-200 py-4 text-center text-xs text-slate-500">
        Open-source demo · fictional data
      </footer>
    </div>
  );
}
