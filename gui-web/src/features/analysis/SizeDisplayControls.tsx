import { SelectField } from "../../components";
import { SIZE_UNIT_RULES } from "../../ipc";
import { sizeLabels } from "./space-format";
import type { SizeBasisFilter, SizeDisplay } from "./space-format";

const basisOptions = [
  { value: "logical", label: sizeLabels.logical },
  { value: "allocated", label: sizeLabels.allocated },
  { value: "unique_physical", label: sizeLabels.unique_physical },
] as const;
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
  const units = [
    { value: "auto", label: "Automatic" },
    ...SIZE_UNIT_RULES[display.system].symbols.map((label, index) => ({
      value: String(index),
      label,
    })),
  ];
  return (
    <div className="du-size-display">
      <SelectField
        label="Size basis"
        value={basis}
        options={basisOptions}
        onChange={onBasis}
      />
      <SelectField
        label="Unit system"
        value={display.system}
        options={unitSystems}
        onChange={(system) => onDisplay({ ...display, system })}
      />
      <SelectField
        label="Size unit"
        value={String(display.unit)}
        options={units}
        onChange={(unit) =>
          onDisplay({ ...display, unit: unit === "auto" ? unit : Number(unit) })
        }
      />
    </div>
  );
}
