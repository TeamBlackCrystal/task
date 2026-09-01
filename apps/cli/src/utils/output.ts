export type OutputOptions = {
  json: boolean;
};

export function print(data: unknown, opts: OutputOptions): void {
  if (opts.json) {
    console.log(JSON.stringify(data, null, 2));
    return;
  }
  printHuman(data);
}

function printHuman(data: unknown): void {
  if (data == null) {
    return;
  }
  if (Array.isArray(data)) {
    for (const item of data) {
      printHuman(item);
    }
    return;
  }
  if (typeof data === "object") {
    const record = data as Record<string, unknown>;
    if ("tasks" in record && Array.isArray(record.tasks)) {
      for (const task of record.tasks) {
        printHuman(task);
      }
      return;
    }
    if ("seq_key" in record && "title" in record) {
      const key = String(record.seq_key ?? record.id ?? "");
      const title = String(record.title ?? "");
      console.log(`${key}\t${title}`);
      return;
    }
    if ("key" in record && "name" in record) {
      console.log(`${String(record.key)}\t${String(record.name)}`);
      return;
    }
    if ("username" in record || "email" in record) {
      console.log(JSON.stringify(record, null, 2));
      return;
    }
    console.log(JSON.stringify(record, null, 2));
    return;
  }
  console.log(String(data));
}
