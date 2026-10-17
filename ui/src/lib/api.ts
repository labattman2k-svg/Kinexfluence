/**
 * Client for the local Kinexfluence core API.
 *
 * The core binds the first free port from its configured starting point, so the
 * UI cannot assume 47821. It probes the range once and remembers the answer for
 * every later call.
 *
 * The UI is served from the Tauri webview, which is a different origin from the
 * core server, so every call here is cross-origin. Nothing in this module knows
 * that beyond the URLs it builds.
 */

/** Ports worth trying, in order. Mirrors the range the core searches. */
const CANDIDATE_PORTS = [
  47821, 47822, 47823, 47824, 47825, 47826, 47827, 47828, 47829, 47830,
];

/** The port that answered last time, if any. */
let cachedBaseUrl: string | null = null;

/**
 * A probe already in flight.
 *
 * Two views mounting at once would otherwise run two full probe loops: the
 * second would start before the first had cached anything.
 */
let probing: Promise<string> | null = null;

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
 * `invalid_input`, `conflict`, `internal`) or `network` when no server answered.
 * `message` is safe to show a person.
 */
export class ApiError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

async function probe(): Promise<string> {
  for (const port of CANDIDATE_PORTS) {
    const url = `http://127.0.0.1:${port}`;
    try {
      const response = await fetch(`${url}/v1/health`, { method: "GET" });
      // A port held by something that is not Kinexfluence could still answer,
      // so the health payload is what identifies the core -- not the status
      // code alone.
      if (response.ok) {
        const body = (await response.json()) as { status?: string };
        if (body.status === "ok") {
          cachedBaseUrl = url;
          return url;
        }
      }
    } catch {
      // Nothing listening on this port; try the next.
    }
  }

  const first = CANDIDATE_PORTS[0];
  const last = CANDIDATE_PORTS[CANDIDATE_PORTS.length - 1];
  throw new ApiError(
    "network",
    `Cannot reach Kinexfluence core on any expected port (${first}–${last}). Is it running?`,
  );
}

async function baseUrl(): Promise<string> {
  if (cachedBaseUrl) return cachedBaseUrl;
  if (!probing) {
    probing = probe().finally(() => {
      probing = null;
    });
  }
  return probing;
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
  const base = await baseUrl();

  let response: Response;
  try {
    response = await fetch(`${base}${path}`, {
      headers: { "content-type": "application/json" },
      ...init,
    });
  } catch {
    // A server that answered the probe but not this call has gone away. Drop
    // the cache, so the next call probes again instead of pointing at a port
    // nothing is listening on.
    cachedBaseUrl = null;
    throw new ApiError(
      "network",
      "The Kinexfluence core stopped responding. Is it still running?",
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
