import { useEffect, useId, useRef, useState } from "react";
import {
  Action,
  Heading,
  ReviewField,
  SegmentedChoice,
  Text,
} from "../../components";
import type { ScanResourceMode } from "../../ipc";
import { useLocaleText } from "../../i18n/presentation";
import "./resource-controls.css";

export type ResourceLimitError = "invalid" | "rejected";

const modeOptions: readonly {
  value: ScanResourceMode;
  key: string;
  fallback: string;
}[] = [
  { value: "auto", key: "ui.analysis.resource_auto", fallback: "Auto" },
  { value: "quiet", key: "ui.analysis.resource_quiet", fallback: "Quiet" },
  { value: "fast", key: "ui.analysis.resource_fast", fallback: "Fast" },
];

export function resourceLimitErrorText(
  kind: ResourceLimitError | undefined,
  max: number | undefined,
  text: ReturnType<typeof useLocaleText>,
) {
  if (!kind) return undefined;
  if (max !== undefined)
    return text(
      "ui.analysis.resource_limit_range",
      "This storage accepts 1 to {max}",
      { max: String(max) },
    );
  return text(
    "ui.analysis.resource_limit_invalid",
    "Enter a whole number of 1 or more",
  );
}

export function resourcePreviousValueText(
  value: number | undefined,
  text: ReturnType<typeof useLocaleText>,
) {
  return value === undefined
    ? text(
        "ui.analysis.resource_previous_automatic",
        "Using the previous value (automatic)",
      )
    : text(
        "ui.analysis.resource_previous_limit",
        "Using the previous value {limit}",
        { limit: String(value) },
      );
}

export function ResourceModeSelector({
  mode,
  onChange,
  showHelp = true,
}: {
  mode: ScanResourceMode;
  onChange: (mode: ScanResourceMode) => void;
  showHelp?: boolean;
}) {
  const text = useLocaleText();
  const options = modeOptions.map(({ value, key, fallback }) => ({
    value,
    label: text(key, fallback),
  }));
  const help = {
    auto: text(
      "ui.analysis.resource_help_auto",
      "Adjusts automatically to storage conditions and screen responsiveness.",
    ),
    quiet: text(
      "ui.analysis.resource_help_quiet",
      "Favors lower device load and responsive input over throughput.",
    ),
    fast: text(
      "ui.analysis.resource_help_fast",
      "Favors throughput within the limit while meeting the GUI response target.",
    ),
  }[mode];
  return (
    <div className="du-resource-mode">
      <Text inline className="du-resource-mode__label">
        {text("ui.analysis.resource_label", "Resource use")}
      </Text>
      <SegmentedChoice
        label={text("ui.analysis.resource_label", "Resource use")}
        value={mode}
        options={options}
        onChange={(value) => onChange(value as ScanResourceMode)}
      />
      {showHelp && <Text kind="muted">{help}</Text>}
    </div>
  );
}

export function ResourceLimitDisclosure({
  value,
  draft,
  max,
  error,
  previousValue,
  disabled = false,
  onChange,
  onReset,
  defaultExpanded = false,
}: {
  value: number | undefined;
  draft: string;
  max?: number | undefined;
  error?: ResourceLimitError | undefined;
  previousValue: number | undefined;
  disabled?: boolean;
  defaultExpanded?: boolean;
  onChange: (draft: string) => void;
  onReset?: (() => void) | undefined;
}) {
  const text = useLocaleText();
  const id = useId();
  const field = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(defaultExpanded);
  useEffect(() => {
    if (error) setExpanded(true);
  }, [error]);
  const errorText = resourceLimitErrorText(error, max, text);
  const description = error
    ? resourcePreviousValueText(previousValue, text)
    : text(
        "ui.analysis.resource_limit_description",
        "Leave blank to use the automatic limit.",
      );
  const currentValue =
    value === undefined
      ? text("ui.analysis.resource_limit_automatic", "automatic")
      : String(value);

  useEffect(() => {
    const input = field.current?.querySelector<HTMLInputElement>("input");
    if (!input) return;
    input.type = "number";
    input.min = "1";
    input.step = "1";
    input.inputMode = "numeric";
    if (max === undefined) input.removeAttribute("max");
    else input.max = String(max);
  }, [expanded, max]);

  return (
    <section className="du-resource-limit">
      <div className="du-resource-limit__heading">
        <Action
          variant="quiet"
          aria-expanded={expanded}
          {...(expanded ? { "aria-controls": id } : {})}
          onPress={() => setExpanded((current) => !current)}
          isDisabled={disabled}
        >
          {expanded ? "▾ " : "▸ "}
          {text("ui.analysis.resource_advanced", "Advanced")} ·{" "}
          {text(
            "ui.analysis.resource_limit_label",
            "Concurrent collection limit",
          )}{" "}
          ({currentValue})
        </Action>
        {onReset && (
          <Action variant="quiet" isDisabled={disabled} onPress={onReset}>
            {text("ui.analysis.resource_reset", "Reset to defaults")}
          </Action>
        )}
      </div>
      {expanded && (
        <div id={id} ref={field} className="du-resource-limit__field">
          <ReviewField
            label={text(
              "ui.analysis.resource_limit_label",
              "Concurrent collection limit",
            )}
            value={draft}
            onChange={onChange}
            description={description}
            error={errorText}
            isDisabled={disabled}
          />
        </div>
      )}
    </section>
  );
}

export function ScopeCollectionConditions({
  mode,
  userLimit,
  draft,
  error,
  previousValue,
  disabled,
  max,
  defaultExpanded = false,
  advancedExpanded = false,
  onModeChange,
  onLimitChange,
  onReset,
}: {
  mode: ScanResourceMode;
  userLimit: number | undefined;
  draft: string;
  error?: ResourceLimitError | undefined;
  previousValue: number | undefined;
  disabled: boolean;
  max?: number | undefined;
  defaultExpanded?: boolean;
  advancedExpanded?: boolean;
  onModeChange: (mode: ScanResourceMode) => void;
  onLimitChange: (draft: string) => void;
  onReset: () => void;
}) {
  const text = useLocaleText();
  const id = useId();
  const [expanded, setExpanded] = useState(defaultExpanded);
  useEffect(() => {
    if (error) setExpanded(true);
  }, [error]);
  const modeLabel = text(
    modeOptions.find((option) => option.value === mode)!.key,
    modeOptions.find((option) => option.value === mode)!.fallback,
  );
  const summary =
    userLimit === undefined
      ? text("ui.analysis.resource_summary", "Resource use: {mode}", {
          mode: modeLabel,
        })
      : text(
          "ui.analysis.resource_summary_limit",
          "Resource use: {mode} · Up to {limit} at once",
          { mode: modeLabel, limit: String(userLimit) },
        );
  const isDefault = mode === "auto" && userLimit === undefined;
  return (
    <section className="du-collection-conditions">
      <div className="du-collection-conditions__header">
        <div>
          <Heading level={3}>
            {text("ui.analysis.collection_conditions", "Collection conditions")}
          </Heading>
          <Text className="du-collection-conditions__summary">
            {summary}
            {isDefault && (
              <Text inline className="du-collection-conditions__default">
                {" "}
                {text("ui.analysis.resource_default", "(default)")}
              </Text>
            )}
          </Text>
        </div>
        <Action
          variant="quiet"
          aria-expanded={expanded}
          {...(expanded ? { "aria-controls": id } : {})}
          isDisabled={disabled}
          onPress={() => setExpanded((current) => !current)}
        >
          {expanded
            ? text(
                "ui.analysis.collection_conditions_collapse",
                "Hide collection conditions",
              )
            : text(
                "ui.analysis.collection_conditions_expand",
                "Change collection conditions",
              )}
        </Action>
      </div>
      {expanded && (
        <div id={id} className="du-collection-conditions__body">
          <ResourceModeSelector mode={mode} onChange={onModeChange} />
          <ResourceLimitDisclosure
            value={userLimit}
            draft={draft}
            max={max}
            error={error}
            previousValue={previousValue}
            disabled={disabled}
            defaultExpanded={advancedExpanded}
            onChange={onLimitChange}
            onReset={onReset}
          />
        </div>
      )}
    </section>
  );
}
