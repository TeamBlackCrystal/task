import type { components, paths as GeneratedPaths } from "./openapi";
import type { ApiPaths, BulkUpdateFields } from "./paths";

type HttpMethod =
  | "get"
  | "put"
  | "post"
  | "delete"
  | "options"
  | "head"
  | "patch"
  | "trace";

type ImplementedMethods<Path> = {
  [Method in HttpMethod]: Method extends keyof Path
    ? NonNullable<Path[Method]> extends never
      ? never
      : Method
    : never;
}[HttpMethod];

type MissingOperations = {
  [Path in keyof ApiPaths]: Path extends keyof GeneratedPaths
    ? Exclude<
        ImplementedMethods<ApiPaths[Path]>,
        ImplementedMethods<GeneratedPaths[Path]>
      > extends never
      ? never
      : Path
    : Path;
}[keyof ApiPaths];

type AssertNoMissingOperations<Missing extends never> = Missing;

// This type fails compilation when a path or HTTP method used by the CLI is
// removed from the canonical OpenAPI contract.
export type CliOpenApiContract = AssertNoMissingOperations<MissingOperations>;

type AssertNoMissingFields<Missing extends never> = Missing;

// This type fails compilation when the canonical OpenAPI schema gains a field
// that the hand-written CLI request type does not declare.
export type BulkUpdateFieldsContract = AssertNoMissingFields<
  Exclude<
    keyof components["schemas"]["BulkUpdateFields"],
    keyof BulkUpdateFields
  >
>;
