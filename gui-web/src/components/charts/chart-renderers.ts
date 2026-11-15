import type { ComponentType } from "react";
import { GeometryRenderer } from "./geometry-renderer";
import type { GeometryRendererProps } from "./geometry-renderer";

type ChartRenderer = ComponentType<GeometryRendererProps>;

/** Freezes the display registrations for open format keys from the engine. */
export function createChartRenderers(
  registrations: readonly (readonly [
    format: string,
    renderer: ChartRenderer,
  ])[],
) {
  const renderers = new Map<string, ChartRenderer>();
  for (const [format, renderer] of registrations) {
    if (format.trim() === "" || renderers.has(format)) {
      throw new Error(`Invalid chart renderer registration: ${format}`);
    }
    renderers.set(format, renderer);
  }

  /** Resolves a display component without changing the model or its format. */
  return (format: string): ChartRenderer =>
    renderers.get(format) ?? GeometryRenderer;
}
