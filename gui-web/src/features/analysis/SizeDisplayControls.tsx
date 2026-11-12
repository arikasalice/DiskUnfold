import { SelectField } from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import { SIZE_UNIT_RULES } from "../../ipc";
import { sizeLabels } from "./space-format";
import type { SizeBasisFilter, SizeDisplay } from "./space-format";

const unitSystems = [
  { value: "iec", label: "IEC (1024)" },
  { value: "si", label: "SI (1000)" },
] as const;

/** Changes presentation only; these controls never fetch another result. */
export function SizeDisplayControls({
  basis,
  display,
  onBasis,
  onDisplay,
}: {
  basis: SizeBasisFilter;
  display: SizeDisplay;
  onBasis: (basis: SizeBasisFilter) => void;
  onDisplay: (display: SizeDisplay) => void;
}) {
  const text = useLocaleText();
  const basisOptions = [
    {
      value: "logical",
      label: text("ui.analysis.logical_size", sizeLabels.logical),
    },
    {
      value: "allocated",
      label: text("ui.analysis.allocated_size", sizeLabels.allocated),
    },
    {
      value: "unique_physical",
      label: text(
        "ui.analysis.unique_physical_size",
        sizeLabels.unique_physical,
      ),
    },
  ] as const;
  const units = [
    { value: "auto", label: text("ui.analysis.automatic", "Automatic") },
    ...SIZE_UNIT_RULES[display.system].symbols.map((label, index) => ({
      value: String(index),
      label,
    })),
  ];
  return (
    <div className="du-size-display">
      <SelectField
        label={text("ui.analysis.size_basis", "Size basis")}
        value={basis}
        options={basisOptions}
        onChange={onBasis}
      />
      <SelectField
        label={text("ui.analysis.unit_system", "Unit system")}
        value={display.system}
        options={unitSystems}
        onChange={(system) => onDisplay({ ...display, system })}
      />
      <SelectField
        label={text("ui.analysis.size_unit", "Size unit")}
        value={String(display.unit)}
        options={units}
        onChange={(unit) =>
          onDisplay({ ...display, unit: unit === "auto" ? unit : Number(unit) })
        }
      />
    </div>
  );
}
