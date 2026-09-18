import type {
  ChoiceId,
  MutationResponse,
  NodeId,
  StateResponse,
} from "./types";

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status?: number,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(input: RequestInfo | URL, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(input, init);
  } catch {
    throw new ApiError("The lesson server could not be reached.");
  }

  if (!response.ok) {
    let message = `The lesson server returned ${response.status}.`;
    try {
      const body = (await response.json()) as { message?: unknown };
      if (typeof body.message === "string") message = body.message;
    } catch {
      // An error body is optional; the status remains useful on its own.
    }
    throw new ApiError(message, response.status);
  }

  return (await response.json()) as T;
}

export function loadState(signal?: AbortSignal): Promise<StateResponse> {
  return request<StateResponse>("/api/v1/state", { signal });
}

export function submitChoice(
  nodeId: NodeId,
  choiceId: ChoiceId,
): Promise<MutationResponse> {
  return request<MutationResponse>(`/api/v1/questions/${nodeId}/submit`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ choice_id: choiceId }),
  });
}

export function revealAnswer(nodeId: NodeId): Promise<MutationResponse> {
  return request<MutationResponse>(`/api/v1/questions/${nodeId}/reveal`, {
    method: "POST",
    headers: { "content-type": "application/json" },
  });
}
