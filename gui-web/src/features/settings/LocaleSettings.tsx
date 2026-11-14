import { useRef, useState } from "react";
import type { ReactNode, Ref } from "react";
import {
  Action,
  DetailDisclosure,
  InspectionDialog,
  Notice,
  Readout,
  SelectField,
  StatusBadge,
  Text,
} from "../../components";
import type { TextValues } from "../../i18n/presentation";
import { useRegionalFormat } from "../../i18n/regional-format";
import type { RegionalFormat } from "../../i18n/regional-format";
import type {
  LocaleAppScale,
  LocaleProfile,
  LocaleProfileChanges,
} from "../../ipc";
import "../../theme/locale.css";
import "../../theme/locale-settings.css";
import { dateExample, numberExample, offsetExample } from "./locale-examples";
import type { SizeUnitReadout } from "./use-size-unit";

type Issue = {
  cause: string;
  action?: string;
  code: string;
  reasonKey: string;
};

/**
 * Props of the appearance and language settings (F13 FR3.HD1.gui-web.DD1,
 * FR4.HD1.gui-web.DD1, FR6.HD1.gui-web.DD1, FR11.HD1, FR12.HD1, FR7.HD1).
 */
export type LocaleSettingsProps = {
  profile: LocaleProfile;
  /** Localized text for a message key. Parameters are formatted by the host. */
  text: (key: string, values?: TextValues) => string;
  save: (changes: LocaleProfileChanges) => void;
  errorKey?: string;
  issue?: Issue;
  /** The selected language lacks messages that the baseline language has. */
  translationIncomplete?: boolean;
  /** Saved size unit, shown read-only because F14 owns its editing. */
  sizeUnit?: SizeUnitReadout;
};

const scales = [
  100, 110, 125, 150, 175, 200,
] as const satisfies readonly LocaleAppScale[];

/** The last scale change the user made, so that it can be reverted. */
type ScaleChange = { from: LocaleAppScale; to: LocaleAppScale };

type TextFn = LocaleSettingsProps["text"];

/**
 * A button that opens the appearance and language table in a dialog.
 *
 * The same entry is used by the shell header and by the screens that have no
 * shell header, so the table is identical on every platform.
 */
export function LocaleSettings(props: LocaleSettingsProps) {
  const { text } = props;
  const [open, setOpen] = useState(false);
  const [scaleChange, setScaleChange] = useState<ScaleChange>();
  const title = text("ui.locale.title");
  return (
    <>
      <Action
        variant="quiet"
        icon="language"
        aria-label={title}
        compact
        onPress={() => setOpen(true)}
      >
        {title}
      </Action>
      {open && (
        <InspectionDialog
          title={text("ui.locale.panel_title")}
          onClose={() => setOpen(false)}
          closeLabel={text("ui.common.close")}
        >
          <LocaleSettingsBody
            {...props}
            scaleChange={scaleChange}
            onScaleChange={setScaleChange}
          />
        </InspectionDialog>
      )}
    </>
  );
}

function LocaleSettingsBody({
  profile,
  text,
  save,
  errorKey,
  issue,
  translationIncomplete = false,
  sizeUnit,
  scaleChange,
  onScaleChange,
}: LocaleSettingsProps & {
  scaleChange: ScaleChange | undefined;
  onScaleChange: (change: ScaleChange | undefined) => void;
}) {
  const format = useRegionalFormat();
  // One sample time for the whole table, taken when the dialog opens.
  const [now] = useState(() => Date.now());
  const timing = text("ui.locale.timing_immediate");
  return (
    <div className="du-locale-fields">
      <ErrorNotice text={text} errorKey={errorKey} issue={issue} />
      <div
        role="table"
        className="du-locale-table"
        aria-label={text("ui.locale.table_label")}
      >
        <div role="row" className="du-locale-head">
          <div role="columnheader">{text("ui.locale.column_setting")}</div>
          <div role="columnheader">{text("ui.locale.column_value")}</div>
          <div role="columnheader">{text("ui.locale.column_timing")}</div>
        </div>
        <LanguageRows
          profile={profile}
          text={text}
          save={save}
          timing={timing}
          incomplete={translationIncomplete}
        />
        <FormatRows
          profile={profile}
          text={text}
          save={save}
          timing={timing}
          format={format}
          now={now}
        />
        <ScaleRow
          profile={profile}
          text={text}
          save={save}
          timing={timing}
          change={scaleChange}
          onChange={onScaleChange}
        />
        <MotionRow profile={profile} text={text} save={save} timing={timing} />
        {sizeUnit && (
          <SettingRow name={text("ui.locale.size_unit")} timing={timing}>
            <Text>{sizeUnitText(sizeUnit, text)}</Text>
            <Text kind="muted">{text("ui.locale.read_only_here")}</Text>
          </SettingRow>
        )}
      </div>
    </div>
  );
}

function sizeUnitText(unit: SizeUnitReadout, text: TextFn): string {
  const system = unit.system === "si" ? "SI" : "IEC";
  const symbol =
    unit.unit === "auto" ? text("ui.analysis.automatic") : unit.unit;
  return `${system} · ${symbol}`;
}

type RowProps = {
  profile: LocaleProfile;
  text: TextFn;
  save: LocaleSettingsProps["save"];
  timing: string;
};

/** One table row: the setting name, its control with notes, and when it applies. */
function SettingRow({
  name,
  timing,
  children,
  ref,
}: {
  name: string;
  timing: string;
  children: ReactNode;
  ref?: Ref<HTMLDivElement>;
}) {
  return (
    <div role="row" className="du-locale-row" ref={ref}>
      <div role="rowheader" className="du-locale-row__name">
        <Text>{name}</Text>
      </div>
      <div role="cell" className="du-locale-row__value">
        {children}
      </div>
      <div role="cell" className="du-locale-row__timing">
        <Text>{timing}</Text>
      </div>
    </div>
  );
}

function ErrorNotice({
  text,
  errorKey,
  issue,
}: {
  text: TextFn;
  errorKey: string | undefined;
  issue: Issue | undefined;
}) {
  if (issue)
    return (
      <Notice tone="error">
        <Text>{issue.cause}</Text>
        {issue.action && <Text>{issue.action}</Text>}
        <DetailDisclosure title={text("ui.locale.error_details")}>
          <Readout
            label={text("ui.locale.error_details")}
            rows={[
              { label: text("ui.locale.error_code"), value: issue.code },
              {
                label: text("ui.locale.error_reason"),
                value: issue.reasonKey,
              },
            ]}
          />
        </DetailDisclosure>
      </Notice>
    );
  return errorKey ? <Notice tone="error">{text(errorKey)}</Notice> : null;
}

/**
 * The display language row. When the selected language lacks messages, a badge
 * sits beside the selector and a band under the row says that English fills in.
 */
function LanguageRows({
  profile,
  text,
  save,
  timing,
  incomplete,
}: RowProps & { incomplete: boolean }) {
  const label = text("ui.locale.language");
  return (
    <>
      <SettingRow name={label} timing={timing}>
        <div className="du-locale-value__line">
          <SelectField
            label={label}
            value={profile.displayLanguage}
            options={profile.supportedLanguages.map(
              ({ language, selfNameKey }) => ({
                value: language,
                label: text(selfNameKey),
              }),
            )}
            onChange={(displayLanguage) => save({ displayLanguage })}
          />
          {incomplete && (
            <StatusBadge tone="neutral">
              {text("ui.locale.translation_partial")}
            </StatusBadge>
          )}
        </div>
      </SettingRow>
      {incomplete && (
        <div role="row" className="du-locale-band">
          <div role="cell" aria-colspan={3}>
            <Notice tone="info">{text("ui.locale.translation_notice")}</Notice>
          </div>
        </div>
      )}
    </>
  );
}

/** Date, number and time zone rows, each with a sample made from the saved profile. */
function FormatRows({
  profile,
  text,
  save,
  timing,
  format,
  now,
}: RowProps & { format: RegionalFormat | undefined; now: number }) {
  const languageDefault = text("ui.locale.default");
  const sample = (value: string | undefined) =>
    value !== undefined && (
      <Text kind="muted">{text("ui.locale.example", { value })}</Text>
    );
  const zoneUnknown =
    profile.timeZonePolicy === "local" &&
    profile.localTimeZoneStatus === "unavailable";
  const dateLabel = text("ui.locale.date_format");
  const numberLabel = text("ui.locale.number_format");
  const zoneLabel = text("ui.locale.time_zone");
  return (
    <>
      <SettingRow name={dateLabel} timing={timing}>
        <SelectField
          label={dateLabel}
          value={profile.dateFormat}
          options={[
            { value: "lang_default", label: languageDefault },
            { value: "iso8601", label: "ISO 8601" },
          ]}
          onChange={(dateFormat) => save({ dateFormat })}
        />
        {format && sample(dateExample(now, format))}
      </SettingRow>
      <SettingRow name={numberLabel} timing={timing}>
        <SelectField
          label={numberLabel}
          value={profile.numberFormat}
          options={[
            { value: "lang_default", label: languageDefault },
            { value: "no_grouping", label: text("ui.locale.no_grouping") },
          ]}
          onChange={(numberFormat) => save({ numberFormat })}
        />
        {format && sample(numberExample(format))}
      </SettingRow>
      <SettingRow name={zoneLabel} timing={timing}>
        <SelectField
          label={zoneLabel}
          value={profile.timeZonePolicy}
          options={[
            { value: "local", label: text("ui.locale.local") },
            { value: "utc", label: "UTC" },
          ]}
          onChange={(timeZonePolicy) => save({ timeZonePolicy })}
        />
        {zoneUnknown ? (
          <Text kind="muted">{text("ui.locale.time_zone_unavailable")}</Text>
        ) : (
          format && sample(offsetExample(now, format))
        )}
      </SettingRow>
    </>
  );
}

/** The motion row says that an operating system request to reduce motion always wins. */
function MotionRow({ profile, text, save, timing }: RowProps) {
  const label = text("ui.locale.motion");
  return (
    <SettingRow name={label} timing={timing}>
      <SelectField
        label={label}
        value={profile.motion}
        options={[
          { value: "follow_os", label: text("ui.locale.follow_os") },
          { value: "reduce", label: text("ui.locale.reduce") },
        ]}
        onChange={(motion) => save({ motion })}
      />
      <Text kind="muted">{text("ui.locale.motion_note")}</Text>
    </SettingRow>
  );
}

/**
 * The scale row applies a choice at once and offers the previous value while
 * the saved scale is the one that choice produced.
 */
function ScaleRow({
  profile,
  text,
  save,
  timing,
  change,
  onChange,
}: RowProps & {
  change: ScaleChange | undefined;
  onChange: (change: ScaleChange | undefined) => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  const label = text("ui.locale.scale");
  const previous =
    change && change.to === profile.appScale ? change.from : undefined;
  return (
    <SettingRow name={label} timing={timing} ref={row}>
      <SelectField
        label={label}
        value={String(profile.appScale)}
        options={scales.map((scale) => ({
          value: String(scale),
          label: `${scale}%`,
        }))}
        onChange={(value) => {
          const appScale = scales.find((scale) => String(scale) === value);
          if (appScale === undefined) return;
          onChange({ from: profile.appScale, to: appScale });
          save({ appScale });
        }}
      />
      {previous !== undefined && (
        <div className="du-locale-revert">
          <Text kind="muted">
            {text("ui.locale.scale_previous", { scale: BigInt(previous) })}
          </Text>
          <Action
            variant="quiet"
            aria-label={text("ui.locale.scale_revert_to", {
              scale: BigInt(previous),
            })}
            onPress={() => {
              // The revert button disappears, so the focus moves to the scale selector first.
              row.current?.querySelector("select")?.focus();
              onChange(undefined);
              save({ appScale: previous });
            }}
          >
            {text("ui.locale.scale_revert")}
          </Action>
        </div>
      )}
    </SettingRow>
  );
}
