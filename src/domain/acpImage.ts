export type AcpImageMediaType = "image/png" | "image/jpeg" | "image/webp";

export type AcpImagePayload = {
  mediaType: AcpImageMediaType;
  dataUrl: string;
  source?: "user" | "agent";
};
