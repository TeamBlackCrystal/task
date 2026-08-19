export class CliError extends Error {
  constructor(
    message: string,
    readonly exitCode: number = 1,
  ) {
    super(message);
    this.name = "CliError";
  }
}

export function exitWithConfigError(message: string): never {
  console.error(message);
  process.exit(2);
}

export function handleApiError(err: unknown): never {
  const e = err as { status?: number; message?: string };
  if (e.status === 401) {
    console.error("Authentication failed: invalid or expired token");
    process.exit(3);
  }
  if (e.status === 403) {
    console.error("Permission denied: insufficient access for this resource");
    process.exit(4);
  }
  if (e.status === 404) {
    console.error("Resource not found");
    process.exit(5);
  }
  console.error(
    JSON.stringify({
      error: "api_error",
      message: e.message ?? String(err),
      status: e.status,
    }),
  );
  process.exit(1);
}

export function unwrapApiResult<T>(
  result: { data?: T; error?: unknown; response: Response },
): T {
  if (result.error) {
    const status = result.response.status;
    handleApiError({ status, message: formatApiError(result.error) });
  }
  if (result.data === undefined) {
    throw new CliError("API returned empty response");
  }
  return result.data;
}

function formatApiError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return JSON.stringify(error);
}
