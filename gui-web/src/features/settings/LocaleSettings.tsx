import { useState } from "react";
import {
  Action,
  DetailDisclosure,
  InspectionDialog,
  Notice,
  Readout,
  SelectField,
  Text,
} from "../../components";
import type {
  LocaleAppScale,
  LocaleProfile,
  LocaleProfileChanges,
} from "../../ipc";
import "../../theme/locale.css";

export type LocaleSettingsProps = {
  profile: LocaleProfile;
  text: (key: string) => string;
  save: (changes: LocaleProfileChanges) => void;
  errorKey?: string;
  issue?: { cause: string; action?: string; code: string; reasonKey: string };
};

const scales = [
  100, 110, 125, 150, 175, 200,
] as const satisfies readonly LocaleAppScale[];

export function LocaleSettings({
  profile,
  text,
  save,
  errorKey,
  issue,
}: LocaleSettingsProps) {
  const [open, setOpen] = useState(false);
  const title = text("ui.locale.title");
  const languageDefault = text("ui.locale.default");
  return (
    <>
      <Action variant="quiet" onPress={() => setOpen(true)}>
        {title}
      </Action>
      {open && (
        <InspectionDialog
          title={title}
          onClose={() => setOpen(false)}
          closeLabel={text("ui.common.close")}
        >
          <div className="du-locale-fields">
            {issue ? (
              <Notice tone="error">
                <Text>{issue.cause}</Text>
                {issue.action && <Text>{issue.action}</Text>}
                <DetailDisclosure title={text("ui.locale.error_details")}>
                  <Readout
                    label={text("ui.locale.error_details")}
                    rows={[
                      {
                        label: text("ui.locale.error_code"),
                        value: issue.code,
                      },
                      {
                        label: text("ui.locale.error_reason"),
                        value: issue.reasonKey,
                      },
                    ]}
                  />
                </DetailDisclosure>
              </Notice>
            ) : (
              errorKey && <Notice tone="error">{text(errorKey)}</Notice>
            )}
            <SelectField
              label={text("ui.locale.language")}
              value={profile.displayLanguage}
              options={profile.supportedLanguages.map(
                ({ language, selfNameKey }) => ({
                  value: language,
                  label: text(selfNameKey),
                }),
              )}
              onChange={(displayLanguage) => save({ displayLanguage })}
            />
            <SelectField
              label={text("ui.locale.date_format")}
              value={profile.dateFormat}
              options={[
                { value: "lang_default", label: languageDefault },
                { value: "iso8601", label: "ISO 8601" },
              ]}
              onChange={(dateFormat) => save({ dateFormat })}
            />
            <SelectField
              label={text("ui.locale.number_format")}
              value={profile.numberFormat}
              options={[
                { value: "lang_default", label: languageDefault },
                { value: "no_grouping", label: text("ui.locale.no_grouping") },
              ]}
              onChange={(numberFormat) => save({ numberFormat })}
            />
            <SelectField
              label={text("ui.locale.time_zone")}
              value={profile.timeZonePolicy}
              options={[
                { value: "local", label: text("ui.locale.local") },
                { value: "utc", label: "UTC" },
              ]}
              onChange={(timeZonePolicy) => save({ timeZonePolicy })}
            />
            <SelectField
              label={text("ui.locale.motion")}
              value={profile.motion}
              options={[
                { value: "follow_os", label: text("ui.locale.follow_os") },
                { value: "reduce", label: text("ui.locale.reduce") },
              ]}
              onChange={(motion) => save({ motion })}
            />
            <SelectField
              label={text("ui.locale.scale")}
              value={String(profile.appScale)}
              options={scales.map((scale) => ({
                value: String(scale),
                label: `${scale}%`,
              }))}
              onChange={(value) => {
                const appScale = scales.find(
                  (scale) => String(scale) === value,
                );
                if (appScale !== undefined) save({ appScale });
              }}
            />
          </div>
        </InspectionDialog>
      )}
    </>
  );
}
