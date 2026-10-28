import { boundary } from "./reader";

export function scrollGeometry(
  total: number,
  offset: number,
  visible: number,
  maximum: number,
  track: number,
) {
  const height = Math.min(
    track,
    Math.max(24, (track * Math.max(0, visible)) / Math.max(1, total)),
  );
  const ratio = maximum > 0 ? Math.max(0, Math.min(1, offset / maximum)) : 0;
  return {
    height: maximum === 0 ? track : height,
    top: (track - height) * ratio,
  };
}
export function scrollOffset(text: string, ratio: number, maximum: number) {
  return boundary(text, Math.round(Math.max(0, Math.min(1, ratio)) * maximum));
}
