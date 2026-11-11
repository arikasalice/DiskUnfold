import { Text } from "../../components";
import type { RecentScopeOpenTarget } from "../../ipc";

export function targetStatus(target: RecentScopeOpenTarget): string {
  switch (target.state) {
    case "matched":
      return "Volume matched";
    case "moved":
      return "Location moved";
    case "disconnected":
      return "Volume disconnected";
    case "unverifiable":
      return "Cannot verify that this is the same volume";
  }
}

export function formatDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

export function PathValue({ value, lossy }: { value: string; lossy: boolean }) {
  return (
    <div className="du-recent-scopes__path">
      <Text kind="path">{value || "(empty path)"}</Text>
      {lossy && <Text kind="muted">Some characters cannot be displayed.</Text>}
    </div>
  );
}
