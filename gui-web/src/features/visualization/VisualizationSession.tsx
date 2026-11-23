import { useEffect, useRef, useState } from "react";
import { Notice, Progress } from "../../components";
import type { AnalysisClient } from "../../ipc";
import { ControlledVisualization } from "./ControlledVisualization";
import { VisualizationController } from "./controller";
import { GroupController } from "./group-controller";
import type { ModelResponse } from "../../viz";
import type { VizText } from "../../viz";
import {
  createVisualizationPort,
  observeVisualizationJob,
  useLocalizedVizText,
} from "../../viz";
import type { VisualizationViewRequest } from "../../viz";

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
  backAction,
  formatAction,
  onAvailableFormats,
  statusAction,
  legend,
  narrowing,
  legendSide,
  onOpenList,
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
  backAction?: Parameters<typeof ControlledVisualization>[0]["backAction"];
  formatAction?: Parameters<typeof ControlledVisualization>[0]["formatAction"];
  onAvailableFormats?: (formats: readonly string[]) => void;
  /** A control at the end of the chart's status line, such as the histogram axis. */
  statusAction?: Parameters<typeof ControlledVisualization>[0]["statusAction"];
  legend?: Parameters<typeof ControlledVisualization>[0]["legend"];
  narrowing?: Parameters<typeof ControlledVisualization>[0]["narrowing"];
  legendSide?: Parameters<typeof ControlledVisualization>[0]["legendSide"];
  onOpenList?: Parameters<typeof ControlledVisualization>[0]["onOpenList"];
}) {
  const localizedText = useLocalizedVizText(text);
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
          client={client}
          controller={session.controller}
          groupController={session.groups}
          text={localizedText}
          autoLoadRows={autoLoadRows}
          sizeBasis={request.sizeBasis}
          {...(resolveRenderer ? { resolveRenderer } : {})}
          {...(onApplyRevision ? { onApplyRevision } : {})}
          {...(backAction ? { backAction } : {})}
          {...(formatAction ? { formatAction } : {})}
          {...(onAvailableFormats ? { onAvailableFormats } : {})}
          {...(statusAction ? { statusAction } : {})}
          legend={legend}
          narrowing={narrowing}
          legendSide={legendSide}
          onOpenList={onOpenList}
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
