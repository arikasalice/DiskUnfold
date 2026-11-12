import { useEffect, useMemo, useRef, useState } from "react";
import { Notice, Progress } from "../../components";
import type { AnalysisClient, VizRenderRequest } from "../../ipc";
import { ControlledVisualization } from "./ControlledVisualization";
import { VisualizationController } from "./controller";
import { GroupController } from "./group-controller";
import type { ModelResponse } from "./model-adapter";
import type { VizText } from "./model-text";
import { useLocaleText } from "../../i18n/presentation";
import { visualizationMessages } from "./messages";
import {
  createVisualizationPort,
  observeVisualizationJob,
} from "./visualization-port";

/** Explicit conditions for one pinned F06 view. Channel and sequence are lifecycle-owned. */
export type VisualizationViewRequest = Omit<
  VizRenderRequest,
  "viewChannel" | "requestSeq" | "resultRevision"
> & { resultRevision: NonNullable<VizRenderRequest["resultRevision"]> };

type Session = {
  client: AnalysisClient;
  resultRevision: string;
  viewChannel: string;
  controller: VisualizationController;
  groups: GroupController;
};

/** Owns one result-bound channel; view changes on that result use increasing sequences. */
export function VisualizationSession({
  client,
  request,
  canDisplay,
  text,
  autoLoadRows,
  resolveRenderer,
  onApplyRevision,
}: {
  client: AnalysisClient;
  request: VisualizationViewRequest;
  canDisplay: (frame: ModelResponse) => boolean;
  text: VizText;
  /** The feature owner must choose whether the initial table page is requested. */
  autoLoadRows: boolean;
  resolveRenderer?: Parameters<
    typeof ControlledVisualization
  >[0]["resolveRenderer"];
  onApplyRevision?: (revision: string) => void;
}) {
  const localeText = useLocaleText();
  const localizedText = useMemo<VizText>(
    () => ({
      ...text,
      message: (key, values) =>
        localeText(
          `ui.visualization.${key}`,
          visualizationMessages.en[key],
          values,
        ),
    }),
    [localeText, text],
  );
  const [session, setSession] = useState<Session | null>(null);
  const [openError, setOpenError] = useState(false);
  const [renderError, setRenderError] = useState(false);
  const canDisplayRef = useRef(canDisplay);
  canDisplayRef.current = canDisplay;
  const requestKey = JSON.stringify(request);

  // A channel belongs to the chosen immutable result, not to each view parameter set.
  useEffect(() => {
    let active = true;
    let owned: Session | null = null;
    let stopJobs: (() => void) | null = null;
    let port: ReturnType<typeof createVisualizationPort> | null = null;
    let openedChannel: string | null = null;
    let closeIssued = false;
    const closeOpenedChannel = () => {
      if (!port || !openedChannel || closeIssued) return;
      closeIssued = true;
      void port.close(openedChannel).catch(() => {});
    };
    setSession(null);
    setOpenError(false);

    async function start() {
      try {
        const currentPort = createVisualizationPort(client, (frame) =>
          canDisplayRef.current(frame),
        );
        port = currentPort;
        const channel = await currentPort.open();
        openedChannel = channel.viewChannel;
        if (!active) {
          closeOpenedChannel();
          return;
        }
        const controller = new VisualizationController(currentPort);
        const groups = new GroupController({ rows: currentPort.groupRows });
        owned = {
          client,
          resultRevision: request.resultRevision,
          viewChannel: channel.viewChannel,
          controller,
          groups,
        };
        stopJobs = observeVisualizationJob(controller, client);
        setSession(owned);
      } catch {
        try {
          stopJobs?.();
        } catch {
          // Continue closing the view channel even if observer cleanup fails.
        }
        owned?.groups.close();
        owned = null;
        closeOpenedChannel();
        if (active) setSession(null);
        if (active) setOpenError(true);
      }
    }

    void start();
    return () => {
      active = false;
      try {
        stopJobs?.();
      } catch {
        // Continue closing the view channel even if observer cleanup fails.
      }
      owned?.groups.close();
      if (owned && port) {
        // Start both cleanup calls immediately so a stalled cancel cannot leak the channel.
        void owned.controller.cancelCurrent().catch(() => {});
      }
      closeOpenedChannel();
    };
  }, [client, request.resultRevision]);

  // Changes to view conditions reuse this pinned channel and advance its sequence.
  useEffect(() => {
    let active = true;
    if (
      !session ||
      session.client !== client ||
      session.resultRevision !== request.resultRevision
    )
      return () => {
        active = false;
      };
    try {
      const renderRequest = JSON.parse(requestKey) as VisualizationViewRequest;
      setRenderError(false);
      void session.controller
        .render({
          ...renderRequest,
          viewChannel: session.viewChannel,
          requestSeq: session.controller.nextRequestSequence(),
        })
        .catch(() => {
          if (active) setRenderError(true);
        });
    } catch {
      if (active) setRenderError(true);
    }
    return () => {
      active = false;
    };
  }, [client, request.resultRevision, requestKey, session]);

  if (
    session?.client === client &&
    session.resultRevision === request.resultRevision
  ) {
    return (
      <>
        {renderError && (
          <Notice tone="error">{localizedText.message("updateFailed")}</Notice>
        )}
        <ControlledVisualization
          controller={session.controller}
          groupController={session.groups}
          text={localizedText}
          autoLoadRows={autoLoadRows}
          {...(resolveRenderer ? { resolveRenderer } : {})}
          {...(onApplyRevision ? { onApplyRevision } : {})}
        />
      </>
    );
  }
  return openError ? (
    <Notice tone="error">{localizedText.message("openFailed")}</Notice>
  ) : (
    <Progress label={localizedText.message("loading")} />
  );
}
