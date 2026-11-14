import type { LocaleText } from "../../i18n/presentation";
import type {
  QueryCondition,
  QueryItemType,
  QueryNameOp,
  QueryPredicate,
  QuerySizeBasis,
  QuerySizeOp,
} from "../../ipc";
import { decodeNameOperand } from "../../state/groups-conditions";
import type { SizeSystem } from "../../state/groups-conditions";
import { sizeUnits } from "../../state/groups-conditions";

/** Label of a name comparison. */
export function nameOpLabel(op: QueryNameOp, text: LocaleText): string {
  if (op === "equal") return text("ui.groups.name_op_equal", "Equals");
  if (op === "contains") return text("ui.groups.name_op_contains", "Contains");
  if (op === "starts_with")
    return text("ui.groups.name_op_starts_with", "Starts with");
  return text("ui.groups.name_op_ends_with", "Ends with");
}

/** Label of an item type. */
export function itemTypeLabel(value: QueryItemType, text: LocaleText): string {
  if (value === "file") return text("ui.groups.type_file", "File");
  if (value === "directory") return text("ui.groups.type_directory", "Folder");
  return text("ui.groups.type_link", "Link");
}

/** Label of a size comparison. */
export function sizeOpLabel(op: QuerySizeOp, text: LocaleText): string {
  if (op === "at_least") return text("ui.groups.size_op_at_least", "At least");
  if (op === "greater_than")
    return text("ui.groups.size_op_greater_than", "More than");
  if (op === "at_most") return text("ui.groups.size_op_at_most", "At most");
  return text("ui.groups.size_op_less_than", "Less than");
}

/** Label of the size a condition compares. */
export function sizeBasisChoiceLabel(
  basis: QuerySizeBasis,
  text: LocaleText,
): string {
  return basis === "logical"
    ? text("ui.groups.basis_logical", "Logical size")
    : text("ui.groups.basis_allocated", "Allocated size");
}

function predicateSummary(
  predicate: QueryPredicate,
  text: LocaleText,
  system: SizeSystem,
): string {
  if (predicate.kind === "type")
    return text("ui.groups.cond_type_summary", "Type: {value}", {
      value: itemTypeLabel(predicate.value, text),
    });
  if (predicate.kind === "registered")
    return text("ui.groups.cond_registered", "Registered condition {id}", {
      id: predicate.id,
    });
  if (predicate.kind === "name")
    return text("ui.groups.cond_name_summary", "Name {op} \u201c{text}\u201d", {
      op: nameOpLabel(predicate.op, text).toLowerCase(),
      text:
        (predicate.encoding === "windows_utf16le"
          ? decodeNameOperand(predicate.value)
          : undefined) ?? text("ui.groups.cond_binary", "(bytes)"),
    });
  const unit = sizeUnits(system)[0]?.symbol ?? "B";
  return text("ui.groups.cond_size_summary", "{basis} {op} {size}", {
    basis: sizeBasisChoiceLabel(predicate.basis, text),
    op: sizeOpLabel(predicate.op, text).toLowerCase(),
    size: `${predicate.bytes} ${unit}`,
  });
}

/** A one-line description of a stored condition, for rules the editor cannot change. */
export function conditionSummary(
  condition: QueryCondition,
  text: LocaleText,
  system: SizeSystem,
): string {
  if (condition.kind === "predicate")
    return predicateSummary(condition.predicate, text, system);
  if (condition.kind === "not")
    return text("ui.groups.cond_not", "not ({condition})", {
      condition: conditionSummary(condition.child, text, system),
    });
  const joiner =
    condition.kind === "all"
      ? text("ui.groups.cond_and", " and ")
      : text("ui.groups.cond_or", " or ");
  return `(${condition.children
    .map((child) => conditionSummary(child, text, system))
    .join(joiner)})`;
}
