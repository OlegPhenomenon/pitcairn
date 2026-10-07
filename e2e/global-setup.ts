// @ts-ignore The same JavaScript module is used by the Node webServer launcher.
import { prepare } from './prepare.mjs';

export default function globalSetup() {
  // The webServer process calls the same preparation before it starts serving:
  // Playwright starts webServer plugins before invoking globalSetup.
  prepare();
}
