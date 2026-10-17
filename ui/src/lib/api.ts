/**
 * Client for the local Kinexfluence core API.
 *
 * The UI is served from the Tauri webview, which is a different origin from
 * the core server, so every call here is cross-origin. Nothing in this module
 * knows about that beyond the base URL.
 */

/** Where the core server listens. Port 47821 unless it was taken at startup. */
export const API_BASE = "http://127.0.0.1:47821";

/** A project as the API renders it. */
export interface Project {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  brief: string | null;
  status: string;
  weights_json: string | null;
  created_at: string;
  updated_at: string;
}

/** The body `POST /v1/projects` accepts. */
export interface NewProject {
  name: string;
  slug?: string;
  description?: string;
  brief?: string;
}

/**
 * A failure the API reported, or a failure to reach it at all.
 *
 * `code` is the machine-readable half of the API's error body (`not_found`,
 * `invalid_input`, `conflict`, `internal`) or `network` when the request never
 * arrived. `message` is safe to show a person.
 */
export class ApiError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

async function toApiError(response: Response): Promise<ApiError> {
  try {
    const body = (await response.json()) as { error?: string; message?: string };
    return new ApiError(body.error ?? "unknown", body.message ?? response.statusText);
  } catch {
    // A non-JSON failure (a proxy error page, say) still has to surface as
    // something readable rather than a parse exception.
    return new ApiError("unknown", `Request failed with status ${response.status}`);
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${API_BASE}${path}`, {
      headers: { "content-type": "application/json" },
      ...init,
    });
  } catch {
    // `fetch` rejects only for transport failures. "Failed to fetch" tells a
    // reader nothing actionable, so name the likely cause.
    throw new ApiError(
      "network",
      `Cannot reach the Kinexfluence server at ${API_BASE}. Is it running?`,
    );
  }

  if (!response.ok) {
    throw await toApiError(response);
  }

  return (await response.json()) as T;
}

/** Every project, oldest first. */
export function listProjects(): Promise<Project[]> {
  return request<Project[]>("/v1/projects");
}

/** One project, by id or slug — the API accepts either. */
export function getProject(idOrSlug: string): Promise<Project> {
  return request<Project>(`/v1/projects/${encodeURIComponent(idOrSlug)}`);
}

/** Create a project. The server generates the id, slug and status. */
export function createProject(data: NewProject): Promise<Project> {
  return request<Project>("/v1/projects", {
    method: "POST",
    body: JSON.stringify(data),
  });
}
