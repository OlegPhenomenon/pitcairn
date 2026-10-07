import {
  apiGet,
  apiPost,
  apiPut,
  useApiMutation,
  useApiQuery,
} from "../../api/client";
import type { ListResponse } from "../../api/types";
import type { InvoiceDto } from "../../api/generated/InvoiceDto";
import type { CreateInvoiceRequest } from "../../api/generated/CreateInvoiceRequest";
import type { ReplaceInvoiceLinesRequest } from "../../api/generated/ReplaceInvoiceLinesRequest";
import type { IssueInvoiceRequest } from "../../api/generated/IssueInvoiceRequest";
import type { CancelInvoiceRequest } from "../../api/generated/CancelInvoiceRequest";
import type { CreatePaymentRequest } from "../../api/generated/CreatePaymentRequest";
import type { CreateRefundRequest } from "../../api/generated/CreateRefundRequest";
import type { RejectPaymentRequest } from "../../api/generated/RejectPaymentRequest";
import type { PaymentDto } from "../../api/generated/PaymentDto";
import type { PayTestCardResponse } from "../../api/generated/PayTestCardResponse";
export const invoicesKey = (id: string) =>
  ["projects", id, "invoices"] as const;
export const useInvoices = (id: string) =>
  useApiQuery<ListResponse<InvoiceDto>>(
    invoicesKey(id),
    `/projects/${id}/invoices`,
    { enabled: !!id },
  );
export const getInvoices = (id: string) =>
  apiGet<ListResponse<InvoiceDto>>(`/projects/${id}/invoices`);
export const useCreateInvoice = (id: string) =>
  useApiMutation<InvoiceDto, CreateInvoiceRequest>(
    (body) => apiPost(`/projects/${id}/invoices`, body),
    { invalidate: [invoicesKey(id), ["finance", "overview"]] },
  );
export const useReplaceLines = (id: string) =>
  useApiMutation<
    InvoiceDto,
    { invoiceId: string; body: ReplaceInvoiceLinesRequest }
  >(({ invoiceId, body }) => apiPut(`/invoices/${invoiceId}/lines`, body), {
    invalidate: [invoicesKey(id), ["finance", "overview"]],
  });
export const useIssueInvoice = (id: string) =>
  useApiMutation<
    InvoiceDto,
    { invoiceId: string; body: IssueInvoiceRequest; key: string }
  >(
    ({ invoiceId, body, key }) =>
      apiPost(`/invoices/${invoiceId}/issue`, body, {
        headers: { "Idempotency-Key": key },
      }),
    { invalidate: [invoicesKey(id), ["finance", "overview"]] },
  );
export const useCancelInvoice = (id: string) =>
  useApiMutation<InvoiceDto, { invoiceId: string; body: CancelInvoiceRequest }>(
    ({ invoiceId, body }) => apiPost(`/invoices/${invoiceId}/cancel`, body),
    { invalidate: [invoicesKey(id), ["finance", "overview"]] },
  );
export const useRecordPayment = (id: string) =>
  useApiMutation<PaymentDto, { invoiceId: string; body: CreatePaymentRequest }>(
    ({ invoiceId, body }) =>
      apiPost(`/invoices/${invoiceId}/payments`, body, {
        headers: { "Idempotency-Key": crypto.randomUUID() },
      }),
    { invalidate: [invoicesKey(id), ["finance", "overview"]] },
  );
export const useRefund = (id: string) =>
  useApiMutation<PaymentDto, { invoiceId: string; body: CreateRefundRequest }>(
    ({ invoiceId, body }) => apiPost(`/invoices/${invoiceId}/refunds`, body),
    { invalidate: [invoicesKey(id), ["finance", "overview"]] },
  );
export const useVerifyPayment = (id: string) =>
  useApiMutation<PaymentDto, string>(
    (paymentId) => apiPost(`/payments/${paymentId}/verify`),
    { invalidate: [invoicesKey(id), ["finance", "overview"]] },
  );
export const useRejectPayment = (id: string) =>
  useApiMutation<PaymentDto, { paymentId: string; body: RejectPaymentRequest }>(
    ({ paymentId, body }) => apiPost(`/payments/${paymentId}/reject`, body),
    { invalidate: [invoicesKey(id), ["finance", "overview"]] },
  );
export const usePayTestCard = (id: string) =>
  useApiMutation<PayTestCardResponse, string>(
    (invoiceId) =>
      apiPost(`/invoices/${invoiceId}/pay-test-card`, undefined, {
        headers: { "Idempotency-Key": crypto.randomUUID() },
      }),
    { invalidate: [invoicesKey(id), ["projects", id]] },
  );
