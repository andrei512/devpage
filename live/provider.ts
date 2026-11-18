// Where the voice model and Fast's model run: OpenAI, or an Azure resource. The environment says which, with the
// variables devpage's video narrator reads. On Azure a model's name is the name of its deployment.

/** The addresses of one provider and the headers that carry its key. */
export interface Provider {
  /** The WebSocket address of a voice session. */
  live: string;
  /** The address Fast's model answers at. */
  responses: string;
  /** The key, in the header the provider reads it from. */
  headers: Record<string, string>;
}

/** What to set when no provider is: said on the page and by the eval. */
export const NEEDS = "DEVPAGE_OPENAI_API_KEY, or DEVPAGE_AZURE_OPENAI_ENDPOINT and DEVPAGE_AZURE_OPENAI_API_KEY";

/** The host of an Azure endpoint, given as an address (`https://<resource>.openai.azure.com/`, any path) or a bare host name. Null when it is neither. */
function host(endpoint: string) {
  try {
    return new URL(endpoint.includes("://") ? endpoint : `https://${endpoint}`).host || null;
  } catch {
    return null;
  }
}

/** The provider the environment names: the Azure endpoint being set selects Azure, otherwise the OpenAI key selects OpenAI. A blank variable counts as unset. Null when neither is whole. */
export function provider(env: Record<string, string | undefined>): Provider | null {
  const set = (name: string) => env[name]?.trim() || null;
  const endpoint = set("DEVPAGE_AZURE_OPENAI_ENDPOINT");
  if (endpoint) {
    const key = set("DEVPAGE_AZURE_OPENAI_API_KEY");
    const at = host(endpoint);
    if (!key || !at) return null;
    return { live: `wss://${at}/openai/v1/live/sessions`, responses: `https://${at}/openai/v1/responses`, headers: { "api-key": key } };
  }
  const key = set("DEVPAGE_OPENAI_API_KEY");
  if (!key) return null;
  return { live: "wss://api.openai.com/v1/live/sessions", responses: "https://api.openai.com/v1/responses", headers: { authorization: `Bearer ${key}` } };
}
