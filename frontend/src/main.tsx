import React from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from 'react-router';
import { router } from './app/routes';
import { setAuthRedirect } from './api/client';
import { ToastProvider } from './ui';
import './index.css';

const queryClient = new QueryClient({ defaultOptions: { queries: { refetchOnWindowFocus: false } } });
setAuthRedirect((path) => void router.navigate(path));

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ToastProvider><RouterProvider router={router} /></ToastProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
