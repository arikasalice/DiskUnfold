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
import { newRow, ruleProblems, sizeUnits } from "../../state/groups-conditions";
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

type Kind = ConditionRow["kind"];

/** 0.0.1 registers only these three predicates, so these are the kinds to pick. */
const KINDS: readonly Kind[] = ["name", "type", "size"];
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

function kindLabel(kind: Kind, text: LocaleText): string {
  if (kind === "name") return text("ui.groups.cond_kind_name", "Name");
  if (kind === "type") return text("ui.groups.cond_kind_type", "Type");
  return text("ui.groups.cond_kind_size", "Size");
}

function addLabel(kind: Kind, text: LocaleText): string {
  if (kind === "name")
    return text("ui.groups.add_condition_name", "Add name condition");
  if (kind === "type")
    return text("ui.groups.add_condition_type", "Add type condition");
  return text("ui.groups.add_condition_size", "Add size condition");
}

/** The problem that marks the value of one row, if any. */
function valueProblem(
  problems: readonly RuleProblem[],
  index: number,
): RuleProblem | undefined {
  return problems.find(
    (problem) =>
      (problem.kind === "empty_value" || problem.kind === "invalid_size") &&
      problem.row === index,
  );
}

function problemText(
  problem: RuleProblem | undefined,
  row: ConditionRow,
  text: LocaleText,
): string | undefined {
  if (problem?.kind === "empty_value")
    return text(
      "ui.groups.problem_empty_value",
      "Enter a value for the {kind} condition.",
      { kind: kindLabel(row.kind, text).toLowerCase() },
    );
  if (problem?.kind === "invalid_size")
    return text(
      "ui.groups.problem_invalid_size",
      "Enter a size that is a whole number of bytes, such as 10 or 1.5.",
    );
  return undefined;
}

type RowProps = {
  row: ConditionRow;
  message: string | undefined;
  system: SizeSystem;
  disabled: boolean;
  onChange: (row: ConditionRow) => void;
};

function OperationField({ row, disabled, onChange }: RowProps) {
  const text = useLocaleText();
  const label = text("ui.groups.cond_op", "Comparison");
  if (row.kind === "name")
    return (
      <SelectField<QueryNameOp>
        label={label}
        value={row.op}
        options={NAME_OPS.map((value) => ({
          value,
          label: nameOpLabel(value, text),
        }))}
        onChange={(op) => !disabled && onChange({ ...row, op })}
      />
    );
  if (row.kind === "size")
    return (
      <SelectField<QuerySizeOp>
        label={label}
        value={row.op}
        options={SIZE_OPS.map((value) => ({
          value,
          label: sizeOpLabel(value, text),
        }))}
        onChange={(op) => !disabled && onChange({ ...row, op })}
      />
    );
  // A type has one comparison; the choice stays so the three columns line up.
  return (
    <SelectField<"is">
      label={label}
      value="is"
      options={[{ value: "is", label: text("ui.groups.type_op_is", "Is") }]}
      onChange={() => {}}
    />
  );
}

function SizeValue({
  row,
  message,
  system,
  disabled,
  onChange,
}: RowProps & { row: SizeRow }) {
  const text = useLocaleText();
  const units = sizeUnits(system);
  return (
    <div className="du-groups-cond__size">
      <ReviewField
        label={text("ui.groups.cond_value", "Enter a value")}
        value={row.amount}
        isDisabled={disabled}
        description={message ?? ""}
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
    </div>
  );
}

function ValueField(props: RowProps) {
  const text = useLocaleText();
  const { row, message, disabled, onChange } = props;
  if (row.kind === "name")
    return (
      <ReviewField
        label={text("ui.groups.cond_value", "Enter a value")}
        value={row.text}
        isDisabled={disabled}
        description={message ?? ""}
        onChange={(value) => onChange({ ...row, text: value })}
      />
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
  return <SizeValue {...props} row={row} />;
}

function ConditionLine({
  row,
  index,
  problems,
  system,
  disabled,
  onChange,
  onRemove,
}: {
  row: ConditionRow;
  index: number;
  problems: readonly RuleProblem[];
  system: SizeSystem;
  disabled: boolean;
  onChange: (row: ConditionRow) => void;
  onRemove: () => void;
}) {
  const text = useLocaleText();
  const message = problemText(valueProblem(problems, index), row, text);
  const props = { row, message, system, disabled, onChange };
  return (
    <section
      className="du-groups-cond"
      data-invalid={message !== undefined || undefined}
      aria-label={text("ui.groups.condition_number", "Condition {number}", {
        number: String(index + 1),
      })}
    >
      <div className="du-groups-cond__row">
        <div className="du-groups-cond__kind">
          <SelectField<Kind>
            label={text("ui.groups.cond_kind", "Condition kind")}
            value={row.kind}
            options={KINDS.map((value) => ({
              value,
              label: kindLabel(value, text),
            }))}
            onChange={(kind) =>
              !disabled && kind !== row.kind && onChange(newRow(kind))
            }
          />
        </div>
        <div className="du-groups-cond__op">
          <OperationField {...props} />
        </div>
        <div className="du-groups-cond__value">
          <ValueField {...props} />
        </div>
        <div className="du-groups-cond__remove">
          <Action
            variant="quiet"
            isDisabled={disabled}
            aria-label={text(
              "ui.groups.remove_condition_number",
              "Remove condition {number}",
              { number: String(index + 1) },
            )}
            onPress={onRemove}
          >
            {"\u00d7"}
          </Action>
        </div>
      </div>
      {row.kind === "size" && (
        <div className="du-groups-cond__basis">
          <SelectField<QuerySizeBasis>
            label={text("ui.groups.cond_size_basis", "Size to compare")}
            value={row.basis}
            options={SIZE_BASES.map((value) => ({
              value,
              label: sizeBasisChoiceLabel(value, text),
            }))}
            onChange={(basis) => !disabled && onChange({ ...row, basis })}
          />
        </div>
      )}
      {message !== undefined && (
        // The same text is the value field's description for assistive
        // technology, so the visible copy is hidden from it.
        <div className="du-groups-cond__error" aria-hidden="true">
          <Text>{message}</Text>
        </div>
      )}
    </section>
  );
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
  onAdd: (kind: Kind) => void;
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
        {text("ui.groups.conditions_all", "Conditions \u00b7 all must match")}
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
        <ConditionLine
          key={index}
          row={row}
          index={index}
          problems={problems}
          system={system}
          disabled={disabled}
          onChange={(next) => onChange(index, next)}
          onRemove={() => onRemove(index)}
        />
      ))}
      <div className="du-groups-badges">
        {KINDS.map((kind) => (
          <Action
            key={kind}
            variant="quiet"
            isDisabled={disabled}
            onPress={() => onAdd(kind)}
          >
            {addLabel(kind, text)}
          </Action>
        ))}
      </div>
    </section>
  );
}
