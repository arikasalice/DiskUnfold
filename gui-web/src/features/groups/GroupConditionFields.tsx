import {
  Action,
  ReviewField,
  SelectField,
  StatusBadge,
  Text,
} from "../../components";
import { useLocaleText } from "../../i18n/presentation";
import type { LocaleText } from "../../i18n/presentation";
import type {
  QueryItemType,
  QueryNameOp,
  QuerySizeBasis,
  QuerySizeOp,
} from "../../ipc";
import { ruleProblems, sizeUnits } from "../../state/groups-conditions";
import type {
  ConditionRow,
  DraftRule,
  RuleProblem,
  SizeRow,
  SizeSystem,
} from "../../state/groups-conditions";
import {
  conditionSummary,
  itemTypeLabel,
  nameOpLabel,
  sizeBasisChoiceLabel,
  sizeOpLabel,
} from "./group-condition-text";

const NAME_OPS: readonly QueryNameOp[] = [
  "equal",
  "contains",
  "starts_with",
  "ends_with",
];
const ITEM_TYPES: readonly QueryItemType[] = ["file", "directory", "link"];
const SIZE_BASES: readonly QuerySizeBasis[] = ["logical", "allocated"];
const SIZE_OPS: readonly QuerySizeOp[] = [
  "at_least",
  "greater_than",
  "at_most",
  "less_than",
];

function problemText(
  problems: readonly RuleProblem[],
  index: number,
  kind: "invalid_name" | "invalid_size",
  text: LocaleText,
): string | undefined {
  const found = problems.some(
    (problem) => problem.kind === kind && problem.row === index,
  );
  if (!found) return undefined;
  return kind === "invalid_name"
    ? text("ui.groups.problem_invalid_name", "Enter the name text to match.")
    : text(
        "ui.groups.problem_invalid_size",
        "Enter a size that is a whole number of bytes, such as 10 or 1.5.",
      );
}

type RowProps = {
  row: ConditionRow;
  index: number;
  problems: readonly RuleProblem[];
  system: SizeSystem;
  disabled: boolean;
  onChange: (row: ConditionRow) => void;
};

function SizeFields({
  row,
  index,
  problems,
  system,
  disabled,
  onChange,
}: RowProps & { row: SizeRow }) {
  const text = useLocaleText();
  const units = sizeUnits(system);
  return (
    <>
      <SelectField<QuerySizeBasis>
        label={text("ui.groups.cond_size_basis", "Size to compare")}
        value={row.basis}
        options={SIZE_BASES.map((value) => ({
          value,
          label: sizeBasisChoiceLabel(value, text),
        }))}
        onChange={(basis) => !disabled && onChange({ ...row, basis })}
      />
      <SelectField<QuerySizeOp>
        label={text("ui.groups.cond_size_op", "Comparison")}
        value={row.op}
        options={SIZE_OPS.map((value) => ({
          value,
          label: sizeOpLabel(value, text),
        }))}
        onChange={(op) => !disabled && onChange({ ...row, op })}
      />
      <ReviewField
        label={text("ui.groups.cond_size_amount", "Size")}
        value={row.amount}
        isDisabled={disabled}
        description={
          problemText(problems, index, "invalid_size", text) ??
          text(
            "ui.groups.cond_size_hint",
            "A whole number of bytes after the unit is applied.",
          )
        }
        onChange={(amount) => onChange({ ...row, amount })}
      />
      <SelectField<string>
        label={text("ui.groups.cond_size_unit", "Unit")}
        value={String(row.unit)}
        options={units.map((unit, position) => ({
          value: String(position),
          label: unit.symbol,
        }))}
        onChange={(unit) =>
          !disabled && onChange({ ...row, unit: Number(unit) })
        }
      />
    </>
  );
}

function RowFields(props: RowProps) {
  const text = useLocaleText();
  const { row, index, problems, disabled, onChange } = props;
  if (row.kind === "name")
    return (
      <>
        <SelectField<QueryNameOp>
          label={text("ui.groups.cond_name_op", "Name comparison")}
          value={row.op}
          options={NAME_OPS.map((value) => ({
            value,
            label: nameOpLabel(value, text),
          }))}
          onChange={(op) => !disabled && onChange({ ...row, op })}
        />
        <ReviewField
          label={text("ui.groups.cond_name_text", "Name text")}
          value={row.text}
          isDisabled={disabled}
          description={
            problemText(problems, index, "invalid_name", text) ??
            text(
              "ui.groups.cond_name_hint",
              "Matched against the file or folder name.",
            )
          }
          onChange={(value) => onChange({ ...row, text: value })}
        />
      </>
    );
  if (row.kind === "type")
    return (
      <SelectField<QueryItemType>
        label={text("ui.groups.cond_type", "Item type")}
        value={row.value}
        options={ITEM_TYPES.map((value) => ({
          value,
          label: itemTypeLabel(value, text),
        }))}
        onChange={(value) => !disabled && onChange({ ...row, value })}
      />
    );
  return <SizeFields {...props} row={row} />;
}

/** The conditions of one rule: editable rows, or a read-only summary with its reason. */
export function GroupConditionFields({
  rule,
  system,
  disabled,
  onAdd,
  onChange,
  onRemove,
}: {
  rule: DraftRule;
  system: SizeSystem;
  disabled: boolean;
  onAdd: (kind: ConditionRow["kind"]) => void;
  onChange: (index: number, row: ConditionRow) => void;
  onRemove: (index: number) => void;
}) {
  const text = useLocaleText();
  if (rule.readOnly)
    return (
      <section
        className="du-groups-condition"
        aria-label={text("ui.groups.conditions_title", "Conditions")}
      >
        <StatusBadge tone="neutral">
          {rule.readOnly === "unregistered"
            ? text("ui.groups.readonly_unregistered_badge", "Cannot be decided")
            : text("ui.groups.readonly_unsupported_badge", "Not editable here")}
        </StatusBadge>
        <Text>
          {rule.readOnly === "unregistered"
            ? text(
                "ui.groups.readonly_unregistered_note",
                "This rule uses a condition that is not registered, so files cannot be decided by it. The condition is kept as it is and cannot be edited.",
              )
            : text(
                "ui.groups.readonly_unsupported_note",
                "This condition has a shape this editor cannot change. It is kept as it is.",
              )}
        </Text>
        {rule.preserved && (
          <Text kind="muted">
            {conditionSummary(rule.preserved, text, system)}
          </Text>
        )}
      </section>
    );
  const problems = ruleProblems(rule, system);
  return (
    <section
      className="du-groups-condition"
      aria-label={text("ui.groups.conditions_title", "Conditions")}
    >
      <Text kind="muted">
        {text(
          "ui.groups.conditions_note",
          "A file joins the group when all conditions match.",
        )}
      </Text>
      {problems.some((problem) => problem.kind === "empty_condition") && (
        <Text>
          {text(
            "ui.groups.problem_empty_condition",
            "Add at least one condition.",
          )}
        </Text>
      )}
      {rule.rows.map((row, index) => (
        <section
          key={index}
          className="du-groups-condition__row"
          aria-label={text("ui.groups.condition_number", "Condition {number}", {
            number: String(index + 1),
          })}
        >
          <RowFields
            row={row}
            index={index}
            problems={problems}
            system={system}
            disabled={disabled}
            onChange={(next) => onChange(index, next)}
          />
          <Action
            variant="quiet"
            isDisabled={disabled}
            aria-label={text(
              "ui.groups.remove_condition_number",
              "Remove condition {number}",
              { number: String(index + 1) },
            )}
            onPress={() => onRemove(index)}
          >
            {text("ui.groups.remove_condition", "Remove condition")}
          </Action>
        </section>
      ))}
      <div className="du-groups-badges">
        <Action isDisabled={disabled} onPress={() => onAdd("name")}>
          {text("ui.groups.add_condition_name", "Add name condition")}
        </Action>
        <Action isDisabled={disabled} onPress={() => onAdd("type")}>
          {text("ui.groups.add_condition_type", "Add type condition")}
        </Action>
        <Action isDisabled={disabled} onPress={() => onAdd("size")}>
          {text("ui.groups.add_condition_size", "Add size condition")}
        </Action>
      </div>
    </section>
  );
}
